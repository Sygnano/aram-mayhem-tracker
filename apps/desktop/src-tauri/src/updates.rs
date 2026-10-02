//! Updates: asking GitHub Releases for a newer version, and installing it when asked.
//!
//! The update feed is `latest.json` on the newest GitHub release (`plugins.updater` in
//! `tauri.conf.json`). An update is only installed after its signature checks out against the public
//! key built into the app, and only one signed for the version the feed announces, so neither a
//! tampered installer nor an older genuine one can be slipped in.
//!
//! Nothing is installed without a click. The companion window asks for a check (or the app does at
//! start, if the setting is on), shows what came back, and offers a download only when there is
//! something newer. Installing runs the NSIS installer in passive mode, which closes the app, so it
//! is never started on its own: it would end whatever the user was doing, a game included.

use std::sync::Mutex;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

/// Where the update check stands, for the companion window.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UpdateStatus {
    /// Nothing asked yet.
    Idle,
    Checking,
    UpToDate {
        current: String,
    },
    Available {
        current: String,
        version: String,
        notes: Option<String>,
    },
    /// `total` is `None` when the server does not say how large the download is.
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    /// The installer is starting; the app is about to close.
    Installing {
        version: String,
    },
    Failed {
        message: String,
    },
}

/// The status, and the update found by the last check, kept so a download installs exactly what
/// the user was shown.
pub struct Updates {
    status: Mutex<UpdateStatus>,
    found: tokio::sync::Mutex<Option<Update>>,
}

impl Default for Updates {
    fn default() -> Self {
        Self { status: Mutex::new(UpdateStatus::Idle), found: tokio::sync::Mutex::new(None) }
    }
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, status: UpdateStatus) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
    }

    /// Asks the feed for a newer version. A check while a download is running is refused, so it
    /// cannot replace the update being installed.
    pub async fn check(&self, app: &AppHandle) -> UpdateStatus {
        let mut found = self.found.lock().await;
        if matches!(self.status(), UpdateStatus::Downloading { .. } | UpdateStatus::Installing { .. }) {
            return self.status();
        }
        self.set(UpdateStatus::Checking);
        let current = app.package_info().version.to_string();
        let result = match app.updater() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let status = match result {
            Ok(Some(update)) => {
                let status = UpdateStatus::Available {
                    current,
                    version: update.version.clone(),
                    notes: update.body.clone().filter(|b| !b.trim().is_empty()),
                };
                log::info!("update available: {}", update.version);
                *found = Some(update);
                status
            }
            Ok(None) => {
                *found = None;
                UpdateStatus::UpToDate { current }
            }
            Err(e) => {
                log::warn!("update check failed: {e}");
                *found = None;
                UpdateStatus::Failed { message: format!("Could not check for updates: {e}") }
            }
        };
        self.set(status.clone());
        status
    }

    /// Downloads the update the last check found, verifies it and runs its installer, which closes
    /// the app. Returns only if that fails.
    pub async fn install(&self) -> Result<(), String> {
        let mut found = self.found.lock().await;
        let Some(update) = found.take() else {
            return Err("no update to install: check for updates first".into());
        };
        let version = update.version.clone();
        self.set(UpdateStatus::Downloading { version: version.clone(), downloaded: 0, total: None });
        log::info!("downloading update {version}");

        let mut downloaded = 0u64;
        let result = update
            .download_and_install(
                |chunk, total| {
                    downloaded += chunk as u64;
                    self.set(UpdateStatus::Downloading { version: version.clone(), downloaded, total });
                },
                || {
                    log::info!("update {version} downloaded and verified; starting the installer");
                    self.set(UpdateStatus::Installing { version: version.clone() });
                },
            )
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(e) => {
                let message = format!("The update could not be installed: {e}");
                log::warn!("{message}");
                // Kept, so the button can try the same update again.
                *found = Some(update);
                self.set(UpdateStatus::Failed { message: message.clone() });
                Err(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `src/types.ts` reads.
    #[test]
    fn statuses_serialise_as_tagged_camel_case() {
        let json = |s: UpdateStatus| serde_json::to_value(s).unwrap();
        assert_eq!(json(UpdateStatus::Idle), serde_json::json!({ "state": "idle" }));
        assert_eq!(
            json(UpdateStatus::UpToDate { current: "1.0.0".into() }),
            serde_json::json!({ "state": "upToDate", "current": "1.0.0" })
        );
        assert_eq!(
            json(UpdateStatus::Available { current: "1.0.0".into(), version: "1.0.1".into(), notes: None }),
            serde_json::json!({ "state": "available", "current": "1.0.0", "version": "1.0.1", "notes": null })
        );
        assert_eq!(
            json(UpdateStatus::Downloading { version: "1.0.1".into(), downloaded: 10, total: Some(20) }),
            serde_json::json!({ "state": "downloading", "version": "1.0.1", "downloaded": 10, "total": 20 })
        );
    }
}
