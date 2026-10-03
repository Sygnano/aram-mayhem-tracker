//! The whole dataset (`/v1/dataset`, D-090): every champion's bundle, the champion table and the
//! anvil rankings, downloaded at once and kept on disk, so nothing the overlay needs about a
//! champion waits on the network.
//!
//! On disk, under the folder the app gives it:
//!
//! - `manifest.json`: which version is current, and the folder holding it;
//! - `<patch>-<dataDate>-<etag>/`: one `champion-<id>.json` per champion (the champion, its twelve
//!   pools and its build archetypes), `champions.json` (the table champ select reads) and
//!   `anvil-rankings.json`.
//!
//! A download is written into a folder of its own, and only the manifest, replaced atomically,
//! makes it current. A download that fails or is cut short leaves the previous version untouched.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{AnvilRankings, AramkitClient, ChampionBundle, ChampionsResponse, ClientError};

const MANIFEST: &str = "manifest.json";
const TABLE: &str = "champions.json";
const ANVIL_RANKINGS: &str = "anvil-rankings.json";

/// Bumped when the layout on disk changes, so an older layout is downloaded again rather than read.
const LAYOUT: u32 = 1;

/// The whole download may take this long: 13 MB on a slow line.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// What the manifest records about the current version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub layout: u32,
    pub patch: String,
    pub data_date: String,
    /// What the service called this version, sent back so an unchanged dataset is a 304.
    pub etag: Option<String>,
    /// Unix seconds.
    pub downloaded_at: u64,
    /// The folder, next to the manifest, that holds this version.
    pub folder: String,
    pub champions: usize,
}

/// A dataset on disk, with the two documents every part of the app needs loaded.
#[derive(Debug, Clone)]
pub struct StoredDataset {
    pub manifest: Manifest,
    /// The folder holding the champion files.
    pub dir: PathBuf,
    pub table: ChampionsResponse,
    pub anvil_rankings: AnvilRankings,
}

impl StoredDataset {
    /// One champion's bundle, `None` when the dataset has none for it (aramkit ranks it but has no
    /// details). Read from disk: about 650 KB, a few milliseconds.
    pub fn champion(&self, champion_id: i64) -> Result<Option<ChampionBundle>, ClientError> {
        let path = self.dir.join(format!("champion-{champion_id}.json"));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut bundle: ChampionBundle = decode(&path, &bytes)?;
        // Stored without its envelope: the patch is the dataset's, and the anvil rankings are held
        // whole, by the app, rather than once per champion.
        bundle.patch = self.manifest.patch.clone();
        bundle.data_date = self.manifest.data_date.clone();
        Ok(Some(bundle))
    }
}

/// How a download ended.
#[derive(Debug)]
pub enum DatasetFetch {
    /// The service still has the version the app holds.
    NotModified,
    /// A new version: the uncompressed JSON, and the ETag to store with it.
    Fetched { json: Vec<u8>, etag: Option<String> },
}

impl AramkitClient {
    /// Downloads the dataset unless `etag` is still current. `progress` is told the bytes received
    /// so far and, when the service says, how many are coming.
    pub async fn dataset(
        &self,
        etag: Option<&str>,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<DatasetFetch, ClientError> {
        let url = format!("{}/v1/dataset", self.base);
        let http_err = |e: reqwest::Error| ClientError::Http { url: url.clone(), message: e.to_string() };
        // Its own client: the shared one decompresses on the fly, which hides the size of the body
        // and with it any progress to report.
        let http = reqwest::Client::builder()
            .no_gzip()
            .timeout(DOWNLOAD_TIMEOUT)
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("aram-mayhem-tracker/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(http_err)?;
        let mut request = http.get(&url).header(reqwest::header::ACCEPT_ENCODING, "gzip");
        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let mut response = request.send().await.map_err(http_err)?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(DatasetFetch::NotModified);
        }
        let header = |name: reqwest::header::HeaderName| {
            response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned)
        };
        let new_etag = header(reqwest::header::ETAG);
        let gzipped = header(reqwest::header::CONTENT_ENCODING).is_some_and(|e| e.eq_ignore_ascii_case("gzip"));
        let total = response.content_length();

        let mut body = Vec::with_capacity(total.unwrap_or(0) as usize);
        progress(0, total);
        while let Some(chunk) = response.chunk().await.map_err(http_err)? {
            body.extend_from_slice(&chunk);
            progress(body.len() as u64, total);
        }
        if !status.is_success() {
            return Err(crate::status_error(status.as_u16(), &url, &body));
        }

        let json = if gzipped {
            tokio::task::spawn_blocking(move || {
                let mut out = Vec::new();
                flate2::read::GzDecoder::new(body.as_slice()).read_to_end(&mut out).map(|_| out)
            })
            .await
            .map_err(|e| ClientError::Decode { url: url.clone(), message: e.to_string() })?
            .map_err(|e| ClientError::Decode { url: url.clone(), message: format!("gunzip: {e}") })?
        } else {
            body
        };
        Ok(DatasetFetch::Fetched { json, etag: new_etag })
    }
}

/// The response, split without decoding the champions: each is copied to its file as it came.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Incoming<'a> {
    #[serde(default)]
    patch: String,
    #[serde(default)]
    data_date: String,
    #[serde(borrow)]
    champions: HashMap<String, &'a RawValue>,
    #[serde(default)]
    champion_table: ChampionsResponse,
    #[serde(default)]
    anvil_rankings: AnvilRankings,
}

/// Writes a downloaded dataset under `root` and makes it current. Blocking: about 113 MB of files.
///
/// Every other version under `root` is deleted once this one is current, so there is only ever one
/// on disk (the owner accepted 113 MB, not twice that).
pub fn store(root: &Path, json: &[u8], etag: Option<String>) -> Result<StoredDataset, ClientError> {
    let incoming: Incoming = decode(Path::new("/v1/dataset"), json)?;
    if incoming.champions.is_empty() || incoming.patch.is_empty() {
        return Err(ClientError::Decode { url: "/v1/dataset".into(), message: "no champions in the dataset".into() });
    }
    let mut ids = Vec::with_capacity(incoming.champions.len());
    for key in incoming.champions.keys() {
        let id: i64 = key.parse().map_err(|_| ClientError::Decode {
            url: "/v1/dataset".into(),
            message: format!("{key:?} is not a champion id"),
        })?;
        ids.push(id);
    }

    let folder = folder_name(&incoming.patch, &incoming.data_date, etag.as_deref());
    std::fs::create_dir_all(root)?;
    let staging = root.join(format!(".incoming-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    let mut table = incoming.champion_table;
    table.patch = incoming.patch.clone();
    table.data_date = incoming.data_date.clone();
    let written = (|| -> Result<(), ClientError> {
        for (key, raw) in &incoming.champions {
            std::fs::write(staging.join(format!("champion-{key}.json")), raw.get())?;
        }
        std::fs::write(staging.join(TABLE), serde_json::to_vec(&table).expect("the table serialises"))?;
        std::fs::write(
            staging.join(ANVIL_RANKINGS),
            serde_json::to_vec(&incoming.anvil_rankings).expect("the rankings serialise"),
        )?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let dir = root.join(&folder);
    // The same version again (a manifest lost, say): the fresh copy replaces it.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&staging, &dir)?;

    let manifest = Manifest {
        layout: LAYOUT,
        patch: incoming.patch,
        data_date: incoming.data_date,
        etag,
        downloaded_at: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        folder: folder.clone(),
        champions: ids.len(),
    };
    write_atomically(&root.join(MANIFEST), &serde_json::to_vec_pretty(&manifest).expect("the manifest serialises"))?;
    remove_all_but(root, &folder);

    Ok(StoredDataset { manifest, dir, table, anvil_rankings: incoming.anvil_rankings })
}

/// The dataset under `root`, if there is a whole one. `None` on a first start, and for a layout an
/// older build wrote, which is downloaded again rather than misread.
pub fn open(root: &Path) -> Result<Option<StoredDataset>, ClientError> {
    let path = root.join(MANIFEST);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) else { return Ok(None) };
    if manifest.layout != LAYOUT || manifest.folder.is_empty() || manifest.folder.contains(['/', '\\', '.']) {
        return Ok(None);
    }
    let dir = root.join(&manifest.folder);
    let read = |name: &str| match std::fs::read(dir.join(name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ClientError::from(e)),
    };
    let (Some(table), Some(rankings)) = (read(TABLE)?, read(ANVIL_RANKINGS)?) else { return Ok(None) };
    Ok(Some(StoredDataset {
        table: decode(&dir.join(TABLE), &table)?,
        anvil_rankings: decode(&dir.join(ANVIL_RANKINGS), &rankings)?,
        manifest,
        dir,
    }))
}

/// `16.19-2026-09-29-1a2b3c4d`: readable, and unique per ETag, since the anvil rankings can change
/// without the data date.
fn folder_name(patch: &str, data_date: &str, etag: Option<&str>) -> String {
    let tag: String = etag.unwrap_or("").chars().filter(char::is_ascii_alphanumeric).take(8).collect();
    let name = format!("{patch}-{data_date}-{tag}");
    // Dots and anything else a path could misread become dashes: `16.19` is `16-19`.
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

/// Deletes every version and leftover staging folder under `root` except `keep`.
fn remove_all_but(root: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir && entry.file_name() != keep {
            if let Err(e) = std::fs::remove_dir_all(entry.path()) {
                log::warn!("could not remove an old statistics folder {}: {e}", entry.path().display());
            }
        }
    }
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), ClientError> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn decode<'a, T: Deserialize<'a>>(path: &Path, bytes: &'a [u8]) -> Result<T, ClientError> {
    serde_json::from_slice(bytes)
        .map_err(|e| ClientError::Decode { url: path.display().to_string(), message: e.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aramkit-dataset-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn dataset(date: &str) -> String {
        let pools = r#"[{"rarity":"gold","stage":1,"augments":[{"id":1058,"deltaPp":2.5,"source":"stage"}]}]"#;
        format!(
            r#"{{"patch":"16.19","dataDate":"{date}","champions":{{
                "157":{{"champion":{{"id":157,"winRate":0.5}},"pools":{pools},"archetypes":[]}},
                "103":{{"champion":{{"id":103}},"pools":[],"archetypes":[]}}}},
              "championTable":{{"poolSize":3,"champions":[{{"id":157,"rank":1}}]}},
              "anvilRankings":{{"savedAt":5,"version":1,"groups":[{{"name":"Swordsmen","champions":[157]}}]}}}}"#
        )
    }

    #[test]
    fn a_stored_dataset_reads_back_champion_by_champion() {
        let root = root("roundtrip");
        let stored = store(&root, dataset("2026-09-29").as_bytes(), Some("\"abc123def456\"".into())).unwrap();
        assert_eq!(stored.manifest.folder, "16-19-2026-09-29-abc123de");
        assert_eq!(stored.manifest.champions, 2);
        assert_eq!(stored.table.pool_size, 3);
        assert_eq!(stored.table.patch, "16.19", "the table carries the dataset's patch");

        let opened = open(&root).unwrap().expect("the manifest names a whole dataset");
        assert_eq!(opened.manifest, stored.manifest);
        assert_eq!(opened.anvil_rankings.group_of(157).map(|g| g.name.as_str()), Some("Swordsmen"));

        let yasuo = opened.champion(157).unwrap().unwrap();
        assert_eq!((yasuo.patch.as_str(), yasuo.data_date.as_str()), ("16.19", "2026-09-29"));
        assert_eq!(yasuo.pools[0].augments[0].delta_pp, Some(2.5));
        assert_eq!(opened.champion(432).unwrap(), None, "not in the dataset is not an error");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_version_replaces_the_old_one_only_once_it_is_whole() {
        let root = root("replace");
        let first = store(&root, dataset("2026-09-29").as_bytes(), Some("\"aaaa\"".into())).unwrap();

        // A download that does not parse changes nothing.
        assert!(store(&root, b"{\"patch\":\"16.19\",\"champions\":{}}", None).is_err());
        assert!(store(&root, b"<html>", None).is_err());
        assert_eq!(open(&root).unwrap().unwrap().manifest, first.manifest);

        let second = store(&root, dataset("2026-09-30").as_bytes(), Some("\"bbbb\"".into())).unwrap();
        assert_eq!(open(&root).unwrap().unwrap().manifest, second.manifest);
        let folders: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        assert_eq!(
            folders,
            std::slice::from_ref(&second.manifest.folder),
            "one version on disk, no staging left behind"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A stand-in `/v1/dataset`: gzipped, with an ETag, and a 304 for a client that already has it.
    async fn serve_dataset(body: String) -> String {
        use std::io::Write;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(body.as_bytes()).unwrap();
        let gzipped = gz.finish().unwrap();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = vec![0u8; 4096];
                let n = socket.read(&mut request).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..n]).to_ascii_lowercase();
                let head = if request.contains("if-none-match: \"v1\"") {
                    "HTTP/1.1 304 Not Modified\r\netag: \"v1\"\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_owned()
                } else {
                    let length = gzipped.len();
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-encoding: gzip\r\n\
                         etag: \"v1\"\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n"
                    )
                };
                let _ = socket.write_all(head.as_bytes()).await;
                if !head.starts_with("HTTP/1.1 304") {
                    let _ = socket.write_all(&gzipped).await;
                }
            }
        });
        base
    }

    #[tokio::test]
    async fn a_download_unpacks_reports_progress_and_is_skipped_when_unchanged() {
        let body = dataset("2026-09-29");
        let client = AramkitClient::new(serve_dataset(body.clone()).await, root("download-cache"));

        let mut seen = Vec::new();
        let fetched = client.dataset(None, |got, total| seen.push((got, total))).await.unwrap();
        let DatasetFetch::Fetched { json, etag } = fetched else { panic!("expected a download") };
        assert_eq!(String::from_utf8(json).unwrap(), body, "unpacked as it was sent");
        assert_eq!(etag.as_deref(), Some("\"v1\""));
        let (last, total) = *seen.last().unwrap();
        assert_eq!(Some(last), total, "progress ends at the compressed size the service announced");
        assert!(total.unwrap() < body.len() as u64);

        let again = client.dataset(Some("\"v1\""), |_, _| {}).await.unwrap();
        assert!(matches!(again, DatasetFetch::NotModified));
    }

    #[test]
    fn nothing_or_something_else_on_disk_is_no_dataset() {
        let root = root("empty");
        assert!(open(&root).unwrap().is_none());
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(MANIFEST), br#"{"layout":0,"patch":"16.18"}"#).unwrap();
        assert!(open(&root).unwrap().is_none(), "an older layout is downloaded again");
        std::fs::write(
            root.join(MANIFEST),
            br#"{"layout":1,"patch":"16.19","dataDate":"x","etag":null,"downloadedAt":1,"folder":"gone","champions":1}"#,
        )
        .unwrap();
        assert!(open(&root).unwrap().is_none(), "a manifest whose folder is missing");
        let _ = std::fs::remove_dir_all(&root);
    }
}
