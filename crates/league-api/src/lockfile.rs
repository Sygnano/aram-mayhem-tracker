//! LCU credential discovery: the `lockfile` the client writes next to its executable.
//!
//! **Nothing here opens a handle to the game process, or to any process but the client's own UI**
//!. The lockfile is a plain file read. When no known directory has one, the running client
//! is located from a process *name* list, which the system hands over without a handle to anything,
//! and only the `LeagueClientUx` process found there is then opened, with the least access there is,
//! to ask where its executable lives.
//!
//! An earlier version enumerated processes through `sysinfo`, which opens every process on the
//! machine with `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ` to read command lines. Run during a
//! game, that includes the game.

use std::path::{Path, PathBuf};

use base64::Engine;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub port: u16,
    pub password: String,
}

impl Credentials {
    pub fn base_url(&self) -> String {
        format!("https://127.0.0.1:{}", self.port)
    }

    pub fn authorization(&self) -> String {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("riot:{}", self.password));
        format!("Basic {token}")
    }
}

/// `ProcessName:PID:Port:Password:Protocol`, e.g. `LeagueClient:12345:54321:secret:https`.
pub fn parse_lockfile(text: &str) -> Option<Credentials> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() != 5 {
        return None;
    }
    Some(Credentials { port: parts[2].parse().ok()?, password: parts[3].to_owned() })
}

/// Default install locations, tried after any configured directory. The research machine has
/// League on `D:`, so both drives are listed.
pub fn default_install_dirs() -> Vec<PathBuf> {
    ["C:\\Riot Games\\League of Legends", "D:\\Riot Games\\League of Legends"].iter().map(PathBuf::from).collect()
}

/// What discovery found, including the install directory (needed for `Config/game.cfg`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub credentials: Credentials,
    pub install_dir: Option<PathBuf>,
}

/// Every lockfile that can be found, most likely first: `configured`, the default install
/// directories, then the directory of the running `LeagueClientUx` process.
///
/// More than one can come back, and the first is not necessarily live: a client that crashed leaves
/// its lockfile behind, pointing at a port nothing listens on. The caller tries them in order.
///
/// The known directories go first because reading a file there costs nothing and touches no
/// process. The process list is only consulted when none of them has a lockfile, which is when the
/// client is closed or installed somewhere unusual.
pub fn candidates(configured: Option<&Path>) -> Vec<Discovered> {
    let mut found: Vec<Discovered> =
        configured.map(Path::to_path_buf).into_iter().chain(default_install_dirs()).filter_map(read_lockfile).collect();
    if found.is_empty() {
        found.extend(client_ux_dirs().into_iter().filter_map(read_lockfile));
    }
    // The configured directory is often one of the defaults too.
    let mut unique: Vec<Discovered> = Vec::with_capacity(found.len());
    for d in found {
        if !unique.iter().any(|u| u.credentials == d.credentials) {
            unique.push(d);
        }
    }
    unique
}

fn read_lockfile(dir: PathBuf) -> Option<Discovered> {
    let text = std::fs::read_to_string(dir.join("lockfile")).ok()?;
    Some(Discovered { credentials: parse_lockfile(&text)?, install_dir: Some(dir) })
}

/// The directories holding a running `LeagueClientUx.exe`, which is where the lockfile is.
#[cfg(windows)]
fn client_ux_dirs() -> Vec<PathBuf> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut pids = Vec::new();
    // SAFETY: plain Win32 calls on a snapshot handle we own and close before leaving the block.
    // `entry` is a correctly sized, initialised `PROCESSENTRY32W` with `dwSize` set as the API
    // requires. The snapshot is a copy of the process table held by the system; taking it opens no
    // process.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return Vec::new() };
        let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            if is_client_ux(&String::from_utf16_lossy(&entry.szExeFile[..len])) {
                pids.push(entry.th32ProcessID);
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }

    let mut dirs = Vec::new();
    for pid in pids {
        // SAFETY: the handle is only used for the one query and closed straight after. `buffer` is
        // `MAX_PATH` wide and `len` says so; the call writes at most that and updates `len` to what
        // it wrote. `PROCESS_QUERY_LIMITED_INFORMATION` grants neither memory access nor control,
        // and the process it is asked of is the client's UI, never the game.
        let path = unsafe {
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { continue };
            let mut buffer = [0u16; MAX_PATH as usize];
            let mut len = buffer.len() as u32;
            let queried = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len);
            let _ = CloseHandle(process);
            if queried.is_err() {
                continue;
            }
            PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize]))
        };
        if let Some(dir) = path.parent() {
            if !dirs.iter().any(|d| d == dir) {
                dirs.push(dir.to_path_buf());
            }
        }
    }
    dirs
}

#[cfg(not(windows))]
fn client_ux_dirs() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn is_client_ux(exe_name: &str) -> bool {
    exe_name.eq_ignore_ascii_case("LeagueClientUx.exe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lockfile() {
        let c = parse_lockfile("LeagueClient:12345:54321:s3cr3t:https\n").unwrap();
        assert_eq!(c, Credentials { port: 54321, password: "s3cr3t".into() });
        assert_eq!(c.base_url(), "https://127.0.0.1:54321");
        // base64("riot:s3cr3t")
        assert_eq!(c.authorization(), "Basic cmlvdDpzM2NyM3Q=");
        assert!(parse_lockfile("garbage").is_none());
    }

    #[test]
    fn the_configured_directory_is_read_first_and_duplicates_collapse() {
        let dir = std::env::temp_dir().join(format!("league-api-lockfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("lockfile"), "LeagueClient:1:6000:abc:https").unwrap();

        let found = candidates(Some(&dir));
        assert_eq!(found[0].credentials, Credentials { port: 6000, password: "abc".into() });
        assert_eq!(found[0].install_dir.as_deref(), Some(dir.as_path()));
        assert_eq!(found.iter().filter(|d| d.credentials.port == 6000).count(), 1);

        // A directory with no lockfile, or with one that does not parse, is simply not a candidate.
        std::fs::write(dir.join("lockfile"), "garbage").unwrap();
        assert!(candidates(Some(&dir)).iter().all(|d| d.install_dir.as_deref() != Some(dir.as_path())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_client_ui_process_is_ever_looked_up() {
        assert!(is_client_ux("LeagueClientUx.exe"));
        assert!(is_client_ux("leagueclientux.exe"));
        assert!(!is_client_ux("League of Legends.exe"), "the game");
        assert!(!is_client_ux("LeagueClient.exe"));
        assert!(!is_client_ux("LeagueClientUxRender.exe"));
    }

    /// Runs the real process scan. It must not fail whatever is running, and on a machine with the
    /// client up it finds the install directory.
    #[test]
    fn the_process_scan_runs() {
        for dir in client_ux_dirs() {
            assert!(dir.is_absolute(), "{}", dir.display());
        }
    }
}
