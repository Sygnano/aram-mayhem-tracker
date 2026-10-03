//! What the app remembers, and what it is tuned with. Two files in `%LOCALAPPDATA%\<identifier>`
//! (in `data` beside the executable for a portable copy), with opposite rules:
//!
//! - **`config.json`** holds the user's [`Settings`] and nothing else. The app writes it whenever
//!   one changes.
//! - **`tuning.json`** is optional and hand-written. It overrides fields of [`Tuning`], is read once
//!   at start, and is **never written by the app**.
//!
//! They used to be one struct in one file, written whole the first time the client was found. That
//! froze every fitted constant at whatever the build of that day shipped: a later build with a
//! corrected layout or threshold was ignored by every existing install, because the old number was
//! sitting in `config.json` looking like a choice. Tuning now lives in code, so a fix shipped in
//! code reaches everyone, and an override exists only where somebody wrote one on purpose.

use std::path::{Path, PathBuf};

use mayhem_core::augments::{AugmentSchedule, OfferTrackerConfig};
use mayhem_core::champselect::ChampSelectLayout;
use mayhem_core::layout::AugmentLayout;
use mayhem_vision::button::ButtonDetectorConfig;
use mayhem_vision::frames::FrameDetectorConfig;
use mayhem_vision::matcher::MatcherConfig;
use mayhem_vision::reroll::RerollDetectorConfig;
use serde::{Deserialize, Serialize};

/// The settings the companion window edits. Persisted in `config.json`; mirrored in `src/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// League install directory. Typed in by the user, or remembered from the first time discovery
    /// found the client, so it is known while the client is closed.
    pub league_dir: Option<PathBuf>,
    /// Write item sets to the League client on champion lock-in.
    ///
    /// **Doing so deletes every item page on the account**, because the LCU endpoint replaces the
    /// whole collection. On by default, at your instruction; the setting is there to turn it off.
    pub manage_item_sets: bool,
    /// Off hides the overlay everywhere, in game and on the client. Nothing else stops: the screen
    /// is still read and item sets are still written, so turning it back on mid-game picks up at once.
    pub overlay_enabled: bool,
    /// Every block and panel shows its tier badge and nothing else.
    pub simple_mode: bool,
    /// Draw the ranked augment list top right during an offer.
    pub show_augment_list: bool,
    /// Accept the ready check one second after a game is found. Any queue.
    pub auto_accept: bool,
    /// Closing the companion window hides it to the tray instead of quitting.
    pub keep_in_tray: bool,
    /// Launch at Windows sign-in, with the companion window out of the way.
    pub start_minimized: bool,
    /// Ask GitHub Releases for a newer version when the app starts. Nothing is installed
    /// without a click either way.
    pub check_updates_on_startup: bool,
    /// Look for newer statistics every 24 hours while the app runs. They are checked at every start
    /// whatever this says, and *Update now* checks at once (D-090).
    pub auto_update_statistics: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            league_dir: None,
            manage_item_sets: true,
            overlay_enabled: true,
            simple_mode: false,
            show_augment_list: true,
            auto_accept: false,
            keep_in_tray: false,
            start_minimized: false,
            check_updates_on_startup: true,
            auto_update_statistics: true,
        }
    }
}

impl Settings {
    /// A missing or unreadable file yields defaults; a corrupt one is kept aside, not overwritten.
    ///
    /// A `config.json` from before the split still carries the tuning fields. They are ignored
    /// here, and gone from the file the next time a setting is saved.
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                log::warn!("config at {} is invalid ({e}); keeping it as .bad and using defaults", path.display());
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).expect("settings serialise"))?;
        std::fs::rename(tmp, path)
    }
}

/// Everything fitted, measured or meant for development. The values are the code's defaults unless
/// `tuning.json` overrides them; nothing here is ever written to disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tuning {
    /// Samples per second for the vision worker. A tick costs about 10 ms when no cards are on
    /// screen and about 100 ms when they are (frame matching plus one parallel OCR pass), so the
    /// rate mostly sets how quickly a rerolled card is noticed rather than how much work is done.
    pub vision_rate_hz: f64,
    pub augment_schedule: AugmentSchedule,
    pub offer_tracker: OfferTrackerConfig,
    /// Where the cards are (fitted from screenshots).
    pub augment_layout: AugmentLayout,
    pub frame_detector: FrameDetectorConfig,
    /// How the "hide augments" button is recognised: the fallback gate, for an offer whose cards
    /// have been put away.
    pub button_detector: ButtonDetectorConfig,
    /// How the reroll buttons are recognised. They are the first and cheapest gate, and the only
    /// one the game's augment tooltip cannot cover.
    pub reroll_detector: RerollDetectorConfig,
    /// Where the champ-select furniture is, fitted from real screenshots.
    pub champ_select_layout: ChampSelectLayout,
    pub matcher: MatcherConfig,
    /// Our aramkit caching service. Overridable so a local `pnpm dev` in `service/` can be
    /// pointed at during development.
    pub service_base: String,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            vision_rate_hz: 10.0,
            augment_schedule: AugmentSchedule::default(),
            offer_tracker: OfferTrackerConfig::default(),
            augment_layout: AugmentLayout::default(),
            frame_detector: FrameDetectorConfig::default(),
            button_detector: ButtonDetectorConfig::default(),
            reroll_detector: RerollDetectorConfig::default(),
            champ_select_layout: ChampSelectLayout::default(),
            matcher: MatcherConfig::default(),
            service_base: aramkit_client::DEFAULT_BASE.to_owned(),
        }
    }
}

/// How [`Tuning::load`] went, so the engine can say so where the user will see it.
#[derive(Debug, Clone, PartialEq)]
pub enum TuningSource {
    /// No `tuning.json`: the normal case.
    Defaults,
    /// `tuning.json` was read and applied.
    Overridden,
    /// `tuning.json` exists but could not be used, so the defaults stand.
    Invalid(String),
}

impl Tuning {
    /// The override file next to `config.json`.
    pub fn path_beside(config_path: &Path) -> PathBuf {
        config_path.with_file_name("tuning.json")
    }

    /// The defaults, with the fields `path` names replaced. An invalid file is left exactly where
    /// it is (somebody wrote it by hand) and ignored whole, so a typo cannot half-apply.
    pub fn load(path: &Path) -> (Self, TuningSource) {
        let Ok(bytes) = std::fs::read(path) else { return (Self::default(), TuningSource::Defaults) };
        match serde_json::from_slice(&bytes) {
            Ok(tuning) => (tuning, TuningSource::Overridden),
            Err(e) => (Self::default(), TuningSource::Invalid(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mayhem-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.json")
    }

    /// The bug this split exists for: a `config.json` written by an older build carries every
    /// tuning value of its day, and none of them may outlive that build.
    #[test]
    fn tuning_frozen_into_an_old_config_is_ignored_and_dropped_on_the_next_save() {
        let path = temp("frozen");
        std::fs::write(
            &path,
            r#"{ "leagueDir": "D:\\Riot Games\\League of Legends", "autoAccept": true, "visionRateHz": 6.0,
                 "matcher": { "minScore": 0.01 }, "serviceBase": "http://old.example", "calibration": { "version": 1 } }"#,
        )
        .unwrap();

        let settings = Settings::load(&path);
        assert!(settings.auto_accept, "the user's own settings are kept");
        assert_eq!(settings.league_dir.as_deref(), Some(Path::new("D:\\Riot Games\\League of Legends")));
        assert!(settings.manage_item_sets, "and the ones the file does not mention default");

        let (tuning, source) = Tuning::load(&Tuning::path_beside(&path));
        assert_eq!(source, TuningSource::Defaults);
        assert_eq!(tuning, Tuning::default(), "nothing in config.json reaches the tuning");

        settings.save(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let mut keys: Vec<&str> = saved.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "autoAccept",
                "autoUpdateStatistics",
                "checkUpdatesOnStartup",
                "keepInTray",
                "leagueDir",
                "manageItemSets",
                "overlayEnabled",
                "showAugmentList",
                "simpleMode",
                "startMinimized"
            ],
            "only settings are ever written"
        );
        assert_eq!(Settings::load(&path), settings);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_tuning_file_overrides_only_what_it_names() {
        let path = Tuning::path_beside(&temp("override"));
        std::fs::write(&path, r#"{ "visionRateHz": 4.0, "serviceBase": "http://127.0.0.1:8080" }"#).unwrap();

        let (tuning, source) = Tuning::load(&path);
        assert_eq!(source, TuningSource::Overridden);
        assert_eq!(tuning.vision_rate_hz, 4.0);
        assert_eq!(tuning.service_base, "http://127.0.0.1:8080");
        assert_eq!(tuning.frame_detector, Tuning::default().frame_detector);
        assert_eq!(tuning.augment_layout, Tuning::default().augment_layout);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_tuning_file_that_does_not_parse_is_ignored_whole_and_left_in_place() {
        let path = Tuning::path_beside(&temp("invalid"));
        std::fs::write(&path, r#"{ "visionRateHz": "fast" }"#).unwrap();

        let (tuning, source) = Tuning::load(&path);
        assert!(matches!(source, TuningSource::Invalid(_)), "{source:?}");
        assert_eq!(tuning, Tuning::default());
        assert!(path.exists(), "a hand-written file is never moved or rewritten");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_corrupt_settings_file_is_set_aside_not_overwritten() {
        let path = temp("corrupt");
        std::fs::write(&path, "{ not json").unwrap();

        assert_eq!(Settings::load(&path), Settings::default());
        assert!(!path.exists());
        assert!(path.with_extension("json.bad").exists());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
