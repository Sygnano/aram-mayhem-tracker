//! The portable copy: the same executable as the installed one, run from a folder the user unzipped.
//!
//! A file named [`MARKER`] next to the executable turns it on; the release zip ships one, the
//! installer does not. A portable copy keeps its settings, its cache and its logs in a `data` folder
//! beside the executable, so the folder can be moved or carried and only WebView2's browser data
//! is left in `%LOCALAPPDATA%`. If that folder cannot be written (a zip opened in place, or a folder
//! under Program Files), the copy falls back to the installed copy's folder rather than lose every
//! setting change.
//!
//! The NSIS updater cannot update a portable copy: it would install a second copy beside it. A
//! portable copy still checks for a newer version, and offers the release page instead.

use std::path::{Path, PathBuf};

/// The marker file's name, next to the executable.
pub const MARKER: &str = "portable";
/// Where a portable copy sends the user for a newer version.
pub const RELEASES_URL: &str = "https://github.com/Sygnano/aram-mayhem-tracker/releases/latest";

/// How this copy was started, decided once at launch.
#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Installed,
    /// Portable, with everything under `data`.
    Portable {
        data: PathBuf,
    },
    /// Portable, but `data` could not be written, so the installed copy's folders are used.
    PortableReadOnly,
}

impl Mode {
    pub fn detect() -> Self {
        match std::env::current_exe() {
            Ok(exe) => match exe.parent() {
                Some(dir) => Self::in_dir(dir),
                None => Self::Installed,
            },
            Err(_) => Self::Installed,
        }
    }

    fn in_dir(dir: &Path) -> Self {
        if !dir.join(MARKER).is_file() {
            return Self::Installed;
        }
        let data = dir.join("data");
        if writable(&data) {
            Self::Portable { data }
        } else {
            Self::PortableReadOnly
        }
    }

    pub fn is_portable(&self) -> bool {
        !matches!(self, Self::Installed)
    }

    /// The folder for `config.json` and `tuning.json`, the cache and the logs, when it is not the
    /// installed copy's.
    pub fn data_dir(&self) -> Option<&Path> {
        match self {
            Self::Portable { data } => Some(data),
            _ => None,
        }
    }
}

/// Creates the folder if needed and proves a file can be written in it.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".write-test");
    let ok = std::fs::create_dir_all(dir).is_ok() && std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

/// Opens the release page in the default browser. The address is fixed here, never taken from the
/// webview.
pub fn open_releases_page() -> Result<(), String> {
    // explorer.exe hands a URL to the default browser. Its exit code is 1 even when it works, so
    // only a failure to start it counts.
    std::process::Command::new("explorer").arg(RELEASES_URL).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mayhem-portable-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn no_marker_means_installed() {
        let dir = scratch("installed");
        assert_eq!(Mode::in_dir(&dir), Mode::Installed);
        assert!(!dir.join("data").exists(), "an installed copy creates nothing beside itself");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn marker_puts_data_beside_the_exe() {
        let dir = scratch("portable");
        std::fs::write(dir.join(MARKER), b"").unwrap();
        let mode = Mode::in_dir(&dir);
        assert_eq!(mode, Mode::Portable { data: dir.join("data") });
        assert!(dir.join("data").is_dir());
        assert!(!dir.join("data/.write-test").exists(), "the probe is cleaned up");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unwritable_data_falls_back() {
        let dir = scratch("readonly");
        std::fs::write(dir.join(MARKER), b"").unwrap();
        // A file where the folder should be: it can be neither created nor written into.
        std::fs::write(dir.join("data"), b"").unwrap();
        assert_eq!(Mode::in_dir(&dir), Mode::PortableReadOnly);
        let _ = std::fs::remove_dir_all(dir);
    }
}
