use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::augments::{build_pool, AugmentList, CherryAugment, PoolAugment};
use crate::champions::ChampionSummary;
use crate::StaticDataError;

pub const CDRAGON_BASE: &str = "https://raw.communitydragon.org";
const GAME_DATA: &str = "plugins/rcp-be-lol-game-data/global";

/// The Mayhem static data for one patch and locale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticData {
    pub patch: String,
    pub locale: String,
    /// `KIWI`: ARAM: Mayhem.
    pub kiwi: Vec<PoolAugment>,
    /// `KIWI_JADE`: the Classic-ish variant.
    pub kiwi_jade: Vec<PoolAugment>,
    pub unmatched: Vec<String>,
    /// Champion alias (`Yasuo`, locale-independent) → Riot champion id. Needed because the augment
    /// statistics service is keyed by champion id while Live Client Data reports a name.
    pub champion_ids: std::collections::HashMap<String, i64>,
    /// Riot champion id → display name, so a badge can say who it is sitting on.
    #[serde(default)]
    pub champion_names: std::collections::HashMap<i64, String>,
    /// True when the patch could not be resolved online and a cached one was used.
    pub offline: bool,
}

impl StaticData {
    /// Riot champion id for a champion key as Live Client Data reports it (`LeeSin`, `Yasuo`).
    ///
    /// Matched case-insensitively and ignoring punctuation, since the live API's key and
    /// CommunityDragon's alias agree on spelling but not always on case.
    pub fn champion_id(&self, champion_key: &str) -> Option<i64> {
        let wanted = crate::champions::normalise_key(champion_key);
        self.champion_ids.get(&wanted).copied()
    }
}

/// What `champion-summary.json` reduces to: [`StaticData::champion_ids`] and
/// [`StaticData::champion_names`].
pub type ChampionIndex = (HashMap<String, i64>, HashMap<i64, String>);

pub struct CDragon {
    http: reqwest::Client,
    base: String,
    cache_dir: PathBuf,
}

/// `16.19.712.1234` → `16.19`, the form CommunityDragon uses for patch directories.
pub fn patch_from_version(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    let (major, minor) = (parts.next()?, parts.next()?);
    (major.parse::<u32>().is_ok() && minor.parse::<u32>().is_ok()).then(|| format!("{major}.{minor}"))
}

/// Game locale (`en_US`, `fr_FR`) → CommunityDragon locale directory (`default`, `fr_fr`).
pub fn locale_dir(game_locale: Option<&str>) -> String {
    match game_locale.map(str::trim) {
        None | Some("") => "default".into(),
        Some(l) if l.eq_ignore_ascii_case("en_US") => "default".into(),
        Some(l) => l.to_ascii_lowercase(),
    }
}

fn patch_key(p: &str) -> Vec<u32> {
    p.split('.').map(|x| x.parse().unwrap_or(0)).collect()
}

impl CDragon {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self::with_base(CDRAGON_BASE, cache_dir)
    }

    pub fn with_base(base: &str, cache_dir: impl Into<PathBuf>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("aram-mayhem-tracker/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static HTTP client configuration");
        Self { http, base: base.trim_end_matches('/').into(), cache_dir: cache_dir.into() }
    }

    fn icon_base(&self, patch: &str) -> String {
        format!("{}/{patch}/{GAME_DATA}/default", self.base)
    }

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, StaticDataError> {
        let http_err = |e: reqwest::Error| StaticDataError::Http { url: url.into(), message: e.to_string() };
        let resp = self.http.get(url).send().await.map_err(http_err)?.error_for_status().map_err(http_err)?;
        Ok(resp.bytes().await.map_err(http_err)?.to_vec())
    }

    /// The current patch from `latest/content-metadata.json`.
    pub async fn current_patch(&self) -> Result<String, StaticDataError> {
        let url = format!("{}/latest/content-metadata.json", self.base);
        let bytes = self.get_bytes(&url).await?;
        let v: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|e| StaticDataError::Decode { file: "content-metadata.json".into(), message: e.to_string() })?;
        v.get("version").and_then(|v| v.as_str()).and_then(patch_from_version).ok_or_else(|| StaticDataError::Decode {
            file: "content-metadata.json".into(),
            message: "no version".into(),
        })
    }

    /// Newest patch that has anything in the cache.
    fn newest_cached_patch(&self) -> Option<String> {
        std::fs::read_dir(&self.cache_dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| patch_from_version(&format!("{n}.0")).is_some())
            .max_by_key(|n| patch_key(n))
    }

    fn cache_path(&self, patch: &str, locale: &str, file: &str) -> PathBuf {
        self.cache_dir.join(patch).join(locale).join(file)
    }

    /// A game-data file, from the cache when present (files for a pinned patch never change).
    ///
    /// A cached copy that does not decode is deleted and fetched again, once. Without that a file
    /// damaged on disk, or stored from a bad response, would fail every load for the rest of the
    /// patch, since nothing else ever replaces it.
    async fn file<T: for<'de> Deserialize<'de>>(
        &self,
        patch: &str,
        locale: &str,
        file: &str,
    ) -> Result<T, StaticDataError> {
        let decode = |bytes: &[u8]| {
            serde_json::from_slice(bytes)
                .map_err(|e| StaticDataError::Decode { file: file.into(), message: e.to_string() })
        };
        let path = self.cache_path(patch, locale, file);
        if let Ok(bytes) = tokio::fs::read(&path).await {
            match decode(&bytes) {
                Ok(value) => return Ok(value),
                Err(e) => {
                    log::warn!("cached {} is unreadable ({e}); fetching it again", path.display());
                    let _ = tokio::fs::remove_file(&path).await;
                }
            }
        }
        let url = format!("{}/{patch}/{GAME_DATA}/{locale}/v1/{file}", self.base);
        let bytes = self.get_bytes(&url).await?;
        // Decoded before it is stored, so a bad response is never what the cache holds.
        let value = decode(&bytes)?;
        write_atomically(&path, &bytes).await?;
        Ok(value)
    }

    /// Deletes cached patches older than the one before `current`. Only called once `current` has
    /// been confirmed online, so an offline start never throws away the copy it is about to use.
    fn prune_old_patches(&self, current: &str) {
        let Ok(entries) = std::fs::read_dir(&self.cache_dir) else { return };
        let mut patches: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| patch_from_version(&format!("{n}.0")).is_some() && n != current)
            .collect();
        patches.sort_by_key(|n| std::cmp::Reverse(patch_key(n)));
        // The newest of the rest stays: it is what a rollback, or a client a patch behind, would use.
        for stale in patches.into_iter().skip(1) {
            match std::fs::remove_dir_all(self.cache_dir.join(&stale)) {
                Ok(()) => log::info!("removed the cached static data for patch {stale}"),
                Err(e) => log::warn!("could not remove the cached static data for patch {stale}: {e}"),
            }
        }
    }

    /// The champion index for `patch`: alias → id, and id → display name. Locale-independent, since
    /// it is keyed on the alias, not the display name.
    ///
    /// Separate from [`CDragon::load`] so that a failure here can be retried alone: the rest of the
    /// static data is fine without it, and reloading all of it to get this back would be wasteful.
    pub async fn champions(&self, patch: &str) -> Result<ChampionIndex, StaticDataError> {
        let champions: Vec<ChampionSummary> = self.file(patch, "default", "champion-summary.json").await?;
        Ok((crate::champions::index(&champions), crate::champions::names(&champions)))
    }

    /// Loads both Mayhem pools for the current patch, in the game's locale (augment names must be
    /// in the language OCR will read). Falls back to the newest cached patch when offline.
    pub async fn load(&self, game_locale: Option<&str>) -> Result<StaticData, StaticDataError> {
        let (patch, offline) = match self.current_patch().await {
            Ok(p) => (p, false),
            Err(e) => (self.newest_cached_patch().ok_or(e)?, true),
        };
        let locale = locale_dir(game_locale);
        // Pool membership is locale-independent; names are not.
        let lists: Vec<AugmentList> = self.file(&patch, "default", "augment-lists.json").await?;
        let augments: Vec<CherryAugment> = self.file(&patch, &locale, "cherry-augments.json").await?;
        let icon_base = self.icon_base(&patch);
        let (kiwi, mut unmatched) = build_pool(&augments, &lists, "KIWI", &icon_base)?;
        let (kiwi_jade, jade_unmatched) = build_pool(&augments, &lists, "KIWI_JADE", &icon_base)?;
        unmatched.extend(jade_unmatched);
        // Best-effort, because this only enables the statistics lookup — without it the offer is
        // still read and named, so a missing champion-summary.json must not fail the whole load. An
        // empty index is retried on its own with [`CDragon::champions`].
        let (champion_ids, champion_names) = match self.champions(&patch).await {
            Ok(champions) => champions,
            Err(e) => {
                log::warn!("champion ids unavailable, augment statistics will be skipped until they load: {e}");
                Default::default()
            }
        };
        if !offline {
            self.prune_old_patches(&patch);
        }
        Ok(StaticData { patch, locale, kiwi, kiwi_jade, unmatched, champion_ids, champion_names, offline })
    }
}

async fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), StaticDataError> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_locale_mapping() {
        assert_eq!(patch_from_version("16.19.712.1234").as_deref(), Some("16.19"));
        assert_eq!(patch_from_version("latest"), None);
        assert_eq!(locale_dir(None), "default");
        assert_eq!(locale_dir(Some("en_US")), "default");
        assert_eq!(locale_dir(Some("fr_FR")), "fr_fr");
    }

    #[tokio::test]
    async fn loads_from_cache_when_offline() {
        let dir = std::env::temp_dir().join(format!("mayhem-static-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (patch, locale, file, body) in [
            ("16.18", "default", "augment-lists.json", "[]"),
            ("16.19", "default", "augment-lists.json", include_str!("../tests/fixtures/augment-lists.synthetic.json")),
            (
                "16.19",
                "fr_fr",
                "cherry-augments.json",
                include_str!("../tests/fixtures/cherry-augments.synthetic.json"),
            ),
        ] {
            let p = dir.join(patch).join(locale);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(file), body).unwrap();
        }
        // Nothing listens on port 9: every network call fails.
        let cd = CDragon::with_base("http://127.0.0.1:9", &dir);
        let data = cd.load(Some("fr_FR")).await.unwrap();
        assert!(data.offline);
        assert_eq!(data.patch, "16.19");
        assert_eq!(data.kiwi.len(), 4);
        assert_eq!(data.kiwi_jade.len(), 1);
        assert_eq!(data.unmatched, vec!["DoesNotExist".to_string()]);
        assert!(data.kiwi[0].icon_url.starts_with("http://127.0.0.1:9/16.19/plugins/"));
        // champion-summary.json is not in the cache and cannot be fetched: the pool still loads and
        // champion ids are simply absent.
        assert!(data.champion_ids.is_empty());
        assert_eq!(data.champion_id("Yasuo"), None);
        // ...and can be had on their own once the file is there, without reloading the rest.
        std::fs::write(
            dir.join("16.19").join("default").join("champion-summary.json"),
            r#"[{"id":157,"name":"Yasuo","alias":"Yasuo"},{"id":-1,"name":"None","alias":"None"}]"#,
        )
        .unwrap();
        let (ids, names) = cd.champions("16.19").await.unwrap();
        assert_eq!(ids.get("yasuo"), Some(&157));
        assert_eq!(names.get(&157).map(String::as_str), Some("Yasuo"));

        // A locale that is not cached is a fetch, which fails offline.
        assert!(cd.load(Some("de_DE")).await.is_err());
        // Offline nothing is pruned: the older patch may be all there is next time.
        assert!(dir.join("16.18").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_cached_file_that_does_not_decode_is_deleted_so_it_can_be_fetched_again() {
        let dir = std::env::temp_dir().join(format!("mayhem-static-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cached = dir.join("16.19").join("default").join("augment-lists.json");
        std::fs::create_dir_all(cached.parent().unwrap()).unwrap();
        std::fs::write(&cached, "{ truncated").unwrap();

        let cd = CDragon::with_base("http://127.0.0.1:9", &dir);
        // Offline the refetch fails, but the bad copy is gone rather than failing every later load.
        assert!(cd.file::<Vec<AugmentList>>("16.19", "default", "augment-lists.json").await.is_err());
        assert!(!cached.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pruning_keeps_the_current_patch_and_the_one_before() {
        let dir = std::env::temp_dir().join(format!("mayhem-static-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for name in ["16.9", "16.17", "16.18", "16.19", "not-a-patch"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        CDragon::with_base("http://127.0.0.1:9", &dir).prune_old_patches("16.19");

        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
        left.sort();
        // 16.9 is older than 16.17: patches compare as numbers, not as text.
        assert_eq!(left, ["16.18", "16.19", "not-a-patch"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
