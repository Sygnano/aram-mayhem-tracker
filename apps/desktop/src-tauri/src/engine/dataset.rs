//! The statistics the app works from: every champion's numbers, downloaded at once and kept on disk
//! next to the settings (D-090).
//!
//! Downloaded when the app starts, every 24 hours while *Update statistics automatically* is on, and
//! when the user clicks *Update now*. An unchanged dataset costs one request answered with a 304. A
//! failed update keeps the copy already on disk and the app carries on with it; only a first start
//! with nothing on disk leaves the app waiting, and that is retried every minute.
//!
//! Once a dataset is loaded, nothing about a champion touches the network: the locked champion's
//! numbers are read from disk (`champion.rs`), and the champion table and the anvil rankings are
//! held here in memory.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aramkit_client::dataset::{self, StoredDataset};
use aramkit_client::{AramkitClient, DatasetFetch, Manifest};

use super::rankings::Rankings;
use super::{Engine, EngineState};

const TICK: Duration = Duration::from_secs(1);
/// How often to look for a newer dataset while the automatic update is on.
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How soon to try again when there is nothing on disk at all, since nothing is shown until there is.
const RETRY_WITHOUT_DATA: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default)]
pub struct DatasetState {
    /// The dataset in use. `None` until one is on disk.
    pub current: Option<Arc<StoredDataset>>,
    /// Bytes received and, when known, expected, while a download runs.
    pub downloading: Option<(u64, Option<u64>)>,
    /// Why the last update failed. Kept until the next one succeeds; it does not stop a dataset
    /// already on disk from being used.
    pub error: Option<String>,
    /// When the service last confirmed what we hold, or sent something newer.
    pub checked_at: Option<std::time::SystemTime>,
    /// *Update now* was clicked and the task has not picked it up yet.
    pub update_requested: bool,
}

impl DatasetState {
    pub fn manifest(&self) -> Option<&Manifest> {
        self.current.as_deref().map(|d| &d.manifest)
    }
}

/// Where the dataset lives: beside `config.json`, in the installed or the portable data folder.
pub fn root(engine: &Engine) -> PathBuf {
    engine.config_path.parent().map_or_else(|| PathBuf::from("statistics"), |dir| dir.join("statistics"))
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the statistics download", run(engine.clone()));
}

/// Asks for an update at the next tick. What the *Update now* button calls.
pub fn request_update(st: &mut EngineState) {
    st.dataset.update_requested = true;
}

async fn run(engine: Arc<Engine>) {
    let root = root(&engine);
    let opened = {
        let root = root.clone();
        tokio::task::spawn_blocking(move || dataset::open(&root)).await
    };
    match opened {
        Ok(Ok(Some(stored))) => {
            let mut st = engine.lock();
            let m = &stored.manifest;
            let note = format!("statistics on disk: {} ({}), {} champions", m.patch, m.data_date, m.champions);
            st.log_event(note);
            apply(&mut st, stored);
        }
        Ok(Ok(None)) => engine.lock().log_event("no statistics on disk yet: downloading them".into()),
        Ok(Err(e)) => {
            engine.lock().log_event(format!("the statistics on disk could not be read, downloading them: {e}"))
        }
        Err(e) => engine.lock().log_event(format!("the statistics on disk could not be read: {e}")),
    }

    let client = {
        let st = engine.lock();
        AramkitClient::new(st.tuning.service_base.clone(), engine.cache_dir.join("aramkit"))
    };
    // Due at once: the app checks at every start.
    let mut next: Instant = Instant::now();
    loop {
        let (requested, automatic, have_data) = {
            let st = engine.lock();
            (st.dataset.update_requested, st.settings.auto_update_statistics, st.dataset.current.is_some())
        };
        let now = Instant::now();
        // Without anything on disk the retry runs whatever the setting says: the app shows nothing
        // until it has statistics, and the setting is about keeping them fresh.
        if requested || (now >= next && (automatic || !have_data)) {
            let succeeded = update(&engine, &client, &root).await;
            let have_data = engine.lock().dataset.current.is_some();
            next = now + if succeeded || have_data { UPDATE_EVERY } else { RETRY_WITHOUT_DATA };
        }
        tokio::time::sleep(TICK).await;
    }
}

/// One check, and a download if there is something new. True when the service answered.
async fn update(engine: &Arc<Engine>, client: &AramkitClient, root: &std::path::Path) -> bool {
    let etag = {
        let mut st = engine.lock();
        st.dataset.update_requested = false;
        st.dataset.downloading = Some((0, None));
        st.dataset.manifest().and_then(|m| m.etag.clone())
    };
    let started = Instant::now();
    let progress = {
        let engine = engine.clone();
        move |received: u64, total: Option<u64>| engine.lock().dataset.downloading = Some((received, total))
    };
    let fetched = client.dataset(etag.as_deref(), progress).await;

    let outcome = match fetched {
        Ok(DatasetFetch::NotModified) => Ok(None),
        Ok(DatasetFetch::Fetched { json, etag }) => {
            let root = root.to_path_buf();
            let size = json.len();
            match tokio::task::spawn_blocking(move || dataset::store(&root, &json, etag)).await {
                Ok(Ok(stored)) => Ok(Some((stored, size))),
                Ok(Err(e)) => Err(e.to_string()),
                Err(e) => Err(e.to_string()),
            }
        }
        Err(e) => Err(e.to_string()),
    };

    let mut st = engine.lock();
    st.dataset.downloading = None;
    match outcome {
        Ok(None) => {
            st.dataset.error = None;
            st.dataset.checked_at = Some(std::time::SystemTime::now());
            let note = match st.dataset.manifest() {
                Some(m) => format!("statistics up to date: {} ({})", m.patch, m.data_date),
                None => "statistics up to date".to_owned(),
            };
            st.log_event(note);
            true
        }
        Ok(Some((stored, size))) => {
            st.dataset.error = None;
            st.dataset.checked_at = Some(std::time::SystemTime::now());
            let m = &stored.manifest;
            let note = format!(
                "statistics downloaded: {} ({}), {} champions, {} MB in {:.1} s",
                m.patch,
                m.data_date,
                m.champions,
                size / 1_000_000,
                started.elapsed().as_secs_f32()
            );
            st.log_event(note);
            apply(&mut st, stored);
            true
        }
        Err(e) => {
            let kept = if st.dataset.current.is_some() { "; keeping the statistics already on disk" } else { "" };
            st.log_event(format!("statistics update failed: {e}{kept}"));
            st.dataset.error = Some(e);
            false
        }
    }
}

/// Makes `stored` the dataset in use: the champion table and the anvil rankings from it, and the
/// locked champion's numbers read again from it.
fn apply(st: &mut EngineState, stored: StoredDataset) {
    st.rankings = Rankings::from_table(&stored.table);
    st.anvils.rankings = Some(stored.anvil_rankings.clone());
    st.anvils.rankings_error = None;
    // `champion.rs` sees the new folder and reads the locked champion again on its next tick,
    // keeping the numbers it has until then.
    st.dataset.current = Some(Arc::new(stored));
}
