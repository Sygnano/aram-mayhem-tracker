//! Last-good responses on disk.
//!
//! An augment offer lasts seconds and the player cannot wait for a service that is down, so every
//! successful response is kept and replayed when the network is not there. Writes are atomic, since
//! a half-written file read back during the next game would be worse than no file at all.

use std::path::{Path, PathBuf};

use crate::ClientError;

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{}.json", sanitise(key)))
    }

    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ClientError> {
        match tokio::fs::read(self.path(key)).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), ClientError> {
        let path = self.path(key);
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        write_atomically(&path, bytes).await
    }

    /// Deletes every stored copy whose key starts with one of `prefixes`, and says how many went.
    /// Best effort: a file that cannot be removed is left for next time.
    pub async fn remove_with_prefixes(&self, prefixes: &[&str]) -> usize {
        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else { return 0 };
        let mut removed = 0;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if prefixes.iter().any(|p| name.starts_with(p)) && tokio::fs::remove_file(entry.path()).await.is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

async fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), ClientError> {
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}

/// Keeps a cache key to characters that are safe in a filename on every platform. Keys are built
/// from champion ids, rarities and augment ids, so this is a guard rather than a transformation.
fn sanitise(key: &str) -> String {
    key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_and_reports_a_miss() {
        let dir = std::env::temp_dir().join(format!("aramkit-client-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cache = Cache::new(&dir);

        assert!(cache.get("offer-157-2-1058").await.unwrap().is_none());
        cache.put("offer-157-2-1058", b"{\"ok\":true}").await.unwrap();
        assert_eq!(cache.get("offer-157-2-1058").await.unwrap().unwrap(), b"{\"ok\":true}");

        // No stray temporary file is left behind by an atomic write.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
            .collect();
        assert!(leftovers.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_cannot_escape_the_cache_directory() {
        assert_eq!(sanitise("offer-157-2-1058,1134"), "offer-157-2-1058_1134");
        assert_eq!(sanitise("../../etc/passwd"), "______etc_passwd");
    }
}
