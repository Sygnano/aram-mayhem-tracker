//! The log file, and what happens when something panics.
//!
//! The code logs through the `log` crate throughout, but until this module nothing was listening:
//! every `log::warn!` was a no-op, and a release build has no console anyway. The only record was
//! the forty-line event ring in memory, gone with the process.
//!
//! [`init`] installs a logger that appends to `<app log dir>/mayhem.log`. The file is capped: past
//! [`MAX_BYTES`] it becomes `mayhem.log.old` (replacing the previous one) and a new file is started,
//! so the two together never exceed about twice that.
//!
//! A small logger of our own rather than a plugin: it is one file, needs no capability granted to
//! the webview, and has no dependency to keep in step with Tauri.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

const FILE_NAME: &str = "mayhem.log";
/// About three weeks of ordinary use; a file this size still opens instantly in any editor.
const MAX_BYTES: u64 = 1024 * 1024;

struct FileLogger {
    path: PathBuf,
    level: LevelFilter,
    /// The open file and how many bytes it holds.
    sink: Mutex<Option<(File, u64)>>,
}

impl FileLogger {
    fn open(path: &Path) -> Option<(File, u64)> {
        let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Some((file, len))
    }

    /// Moves the full file aside and starts a new one.
    fn rotate(&self, sink: &mut Option<(File, u64)>) {
        // Closed first: Windows will not rename a file that is open.
        *sink = None;
        let _ = std::fs::rename(&self.path, self.path.with_extension("log.old"));
        *sink = Self::open(&self.path);
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line =
            format!("{} {:<5} {}: {}\n", timestamp(SystemTime::now()), record.level(), record.target(), record.args());
        if cfg!(debug_assertions) {
            // `pnpm dev` has a terminal, and that is where a developer is looking.
            eprint!("{line}");
        }
        // A panic while logging must not take the logger with it.
        let mut sink = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if sink.as_ref().is_some_and(|(_, len)| *len >= MAX_BYTES) {
            self.rotate(&mut sink);
        }
        if let Some((file, len)) = sink.as_mut() {
            if file.write_all(line.as_bytes()).is_ok() {
                *len += line.len() as u64;
            }
            // Flushed per line for anything that matters: the lines worth having are the ones
            // written just before the process dies.
            if record.level() <= Level::Warn {
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Some((file, _)) = self.sink.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = file.flush();
        }
    }
}

/// Starts logging to `dir` and installs the panic hook. Returns the log file's path, or `None` when
/// the file could not be opened, in which case the app runs as it always did: without a log.
///
/// `MAYHEM_LOG=debug` (or `trace`) in the environment raises the level from the default `info`.
pub fn init(dir: &Path) -> Option<PathBuf> {
    let level = match std::env::var("MAYHEM_LOG").ok().as_deref().map(str::trim) {
        Some("trace") => LevelFilter::Trace,
        Some("debug") => LevelFilter::Debug,
        Some("warn") => LevelFilter::Warn,
        _ => LevelFilter::Info,
    };
    let path = dir.join(FILE_NAME);
    let opened = std::fs::create_dir_all(dir).ok().and_then(|()| FileLogger::open(&path));
    let usable = opened.is_some();
    let logger = FileLogger { path: path.clone(), level, sink: Mutex::new(opened) };
    if log::set_boxed_logger(Box::new(logger)).is_err() {
        return None;
    }
    log::set_max_level(level);
    install_panic_hook();
    log::info!("ARAM Mayhem Tracker {} started", env!("CARGO_PKG_VERSION"));
    usable.then_some(path)
}

/// Writes every panic to the log before the default hook runs.
///
/// A panic on the vision thread or in an engine task does not end the process, it ends that one
/// subsystem, and from the outside nothing looks different. The line written here is what says
/// which one stopped and where; the engine's own supervision (`engine::spawn_task`, the vision
/// thread's `catch_unwind`) is what tells the user.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        log::error!("panic on thread '{}': {info}", thread.name().unwrap_or("unnamed"));
        log::logger().flush();
        default(info);
    }));
}

/// `2026-10-01T14:03:27.512Z`. UTC, so two machines' logs line up without knowing either's zone.
fn timestamp(now: SystemTime) -> String {
    let since_epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (secs, millis) = (since_epoch.as_secs(), since_epoch.subsec_millis());
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn timestamps_are_utc_calendar_dates() {
        let at = |secs: u64, millis: u64| timestamp(UNIX_EPOCH + Duration::from_millis(secs * 1000 + millis));
        assert_eq!(at(0, 0), "1970-01-01T00:00:00.000Z");
        // A leap day, and the day after it.
        assert_eq!(at(1_709_164_800, 7), "2024-02-29T00:00:00.007Z");
        assert_eq!(at(1_709_251_199, 999), "2024-02-29T23:59:59.999Z");
        assert_eq!(at(1_709_251_200, 0), "2024-03-01T00:00:00.000Z");
        assert_eq!(at(1_790_863_407, 512), "2026-10-01T14:03:27.512Z");
    }

    #[test]
    fn a_full_log_is_set_aside_and_a_new_one_started() {
        let dir = std::env::temp_dir().join(format!("mayhem-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE_NAME);
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize]).unwrap();

        let logger =
            FileLogger { path: path.clone(), level: LevelFilter::Info, sink: Mutex::new(FileLogger::open(&path)) };
        logger.log(&Record::builder().args(format_args!("after the cap")).level(Level::Warn).target("test").build());
        logger.log(&Record::builder().args(format_args!("too quiet")).level(Level::Debug).target("test").build());
        logger.flush();

        let old = std::fs::metadata(path.with_extension("log.old")).unwrap().len();
        assert_eq!(old, MAX_BYTES, "the full file was kept as .old");
        let current = std::fs::read_to_string(&path).unwrap();
        assert!(current.ends_with("WARN  test: after the cap\n"), "{current:?}");
        assert_eq!(current.lines().count(), 1, "below the level is not written");

        drop(logger);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
