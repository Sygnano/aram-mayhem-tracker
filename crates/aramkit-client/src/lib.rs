//! Client for our aramkit caching service.
//!
//! The app never talks to aramkit directly; it talks to the service, which owns the caching and the
//! parsing. These types mirror the service's wire contract. They are deliberately duplicated rather
//! than shared, because `service/` is an independent workspace — so everything here is lenient, and
//! a field the service stops sending degrades a row instead of failing the response.
//!
//! Every successful response worth replaying is written to disk. An offer during a game must never
//! render empty, so when the service is unreachable the last good copy is served and flagged stale.

mod cache;
mod types;

pub use cache::Cache;
pub use types::*;

use std::{path::PathBuf, time::Duration};

/// The service. Override for local development against `pnpm dev` in `service/`.
pub const DEFAULT_BASE: &str = "https://mayhemcache.drawyoursword.lol";

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("HTTP error for {url}: {message}")]
    Http { url: String, message: String },
    /// The service answered, but not with data: 503 while it warms up, 404 for a champion it has
    /// nothing for.
    #[error("service returned {status} for {url}: {message}")]
    Status { status: u16, url: String, message: String },
    #[error("unexpected response from {url}: {message}")]
    Decode { url: String, message: String },
    #[error("cache I/O: {0}")]
    Io(#[from] std::io::Error),
}

/// How a payload reached us, so the UI can be honest about what it is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Freshness {
    /// Straight from the service.
    Live,
    /// The service was unreachable; this is the last copy we stored.
    Stale,
}

#[derive(Debug, Clone)]
pub struct Fetched<T> {
    pub value: T,
    pub freshness: Freshness,
}

pub struct AramkitClient {
    http: reqwest::Client,
    base: String,
    cache: Cache,
}

impl AramkitClient {
    pub fn new(base: impl Into<String>, cache_dir: impl Into<PathBuf>) -> Self {
        let http = reqwest::Client::builder()
            // Short: this runs during a game, and a slow answer is worse than a stale one.
            .timeout(Duration::from_secs(6))
            .connect_timeout(Duration::from_secs(3))
            .user_agent(concat!("aram-mayhem-tracker/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static HTTP client configuration");
        Self { http, base: base.into().trim_end_matches('/').to_owned(), cache: Cache::new(cache_dir) }
    }

    /// Everything about one champion in one request: the ranked pool for every rarity at
    /// every stage, from which the overlay opens the list for the offer on screen and picks the
    /// offer's own numbers; the build archetypes for the item sets; and the champion's anvil
    /// ranking group.
    pub async fn champion(&self, champion_id: i64) -> Result<Fetched<ChampionBundle>, ClientError> {
        let url = format!("{}/v1/champion?champion={champion_id}", self.base);
        self.get(&url, &format!("champion-{champion_id}")).await
    }

    /// Every champion's rank, tier and rates, for the champ-select overlay.
    ///
    /// One request per patch: the response is the whole table, so a bench reroll is answered from
    /// memory rather than over the network. Cached to disk under a fixed key like everything else,
    /// which means a champ select entered offline still shows numbers.
    pub async fn champions(&self) -> Result<Fetched<ChampionsResponse>, ClientError> {
        let url = format!("{}/v1/champions", self.base);
        self.get(&url, "champions").await
    }

    /// The stat anvil shards, named in `locale` (`en_us`, `fr_fr`, …). Once per patch.
    pub async fn anvils(&self, locale: &str) -> Result<Fetched<AnvilCatalogue>, ClientError> {
        let url = format!("{}/v1/anvils?locale={locale}", self.base);
        self.get(&url, &format!("anvils-{locale}")).await
    }

    /// How these champions deal their damage, one by one and together as a team. Asked about the
    /// enemy team, to decide between Armor and Magic Resist.
    ///
    /// Not kept on disk. The answer is about one particular enemy team, which will never be met
    /// again, so a stored copy could not be replayed and would only pile up, one file a game.
    pub async fn damage(&self, champion_ids: &[i64]) -> Result<DamageResponse, ClientError> {
        let ids = champion_ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let url = format!("{}/v1/damage?champions={ids}", self.base);
        let bytes = self.get_bytes(&url).await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::Decode { url, message: e.to_string() })
    }

    /// Deletes stored copies no request reads any more: per-team damage answers and per-offer
    /// answers, which earlier builds wrote one file at a time and never removed, and the separate
    /// pool, build and anvil-ranking answers the champion bundle replaced. Returns how many went.
    pub async fn remove_obsolete_copies(&self) -> usize {
        self.cache.remove_with_prefixes(&["damage-", "offer-", "pool-", "build-", "anvil-rankings"]).await
    }

    /// Fetches and caches, falling back to the stored copy when the service cannot be reached.
    async fn get<T>(&self, url: &str, cache_key: &str) -> Result<Fetched<T>, ClientError>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        match self.get_bytes(url).await {
            Ok(bytes) => match serde_json::from_slice::<T>(&bytes) {
                Ok(value) => {
                    if let Err(e) = self.cache.put(cache_key, &bytes).await {
                        // Not fatal: we have the answer, we just could not keep it.
                        log::warn!("could not cache {cache_key}: {e}");
                    }
                    Ok(Fetched { value, freshness: Freshness::Live })
                }
                Err(e) => Err(ClientError::Decode { url: url.into(), message: e.to_string() }),
            },
            Err(e) => {
                // A 4xx is an answer, not an outage: serving a stale copy would hide a real problem
                // such as an augment id we should not have sent. The exception is 429: the service
                // is merely busy, which is an outage as far as this request is concerned.
                if let ClientError::Status { status, .. } = &e {
                    if *status < 500 && *status != 429 {
                        return Err(e);
                    }
                }
                match self.cache.get(cache_key).await? {
                    Some(bytes) => {
                        let value = serde_json::from_slice::<T>(&bytes)
                            .map_err(|err| ClientError::Decode { url: url.into(), message: err.to_string() })?;
                        log::warn!("serving a cached copy of {cache_key}: {e}");
                        Ok(Fetched { value, freshness: Freshness::Stale })
                    }
                    None => Err(e),
                }
            }
        }
    }

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, ClientError> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| ClientError::Http { url: url.into(), message: e.to_string() })?;

        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| ClientError::Http { url: url.into(), message: e.to_string() })?;

        if !status.is_success() {
            // The service reports errors as JSON, so surface its message rather than a bare code.
            let message = serde_json::from_slice::<ServiceError>(&bytes)
                .map(|e| e.error)
                .unwrap_or_else(|_| String::from_utf8_lossy(&bytes).chars().take(200).collect());
            return Err(ClientError::Status { status: status.as_u16(), url: url.into(), message });
        }
        Ok(bytes.to_vec())
    }
}

#[derive(Debug, serde::Deserialize)]
struct ServiceError {
    #[serde(default)]
    error: String,
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// A stand-in service: answers every request with `status` and `body`, one connection each.
    async fn serve(status: u16, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        base
    }

    fn cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aramkit-client-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const TABLE: &str = r#"{"patch":"16.19","poolSize":2,"champions":[{"id":157,"rank":1}]}"#;
    const OLDER: &str = r#"{"patch":"16.18","poolSize":1,"champions":[]}"#;

    #[tokio::test]
    async fn a_live_answer_is_served_and_kept() {
        let dir = cache_dir("live");
        let client = AramkitClient::new(serve(200, TABLE).await, &dir);

        let fetched = client.champions().await.unwrap();
        assert_eq!(fetched.freshness, Freshness::Live);
        assert_eq!(fetched.value.patch, "16.19");
        assert_eq!(client.cache.get("champions").await.unwrap().unwrap(), TABLE.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_outage_is_answered_from_the_stored_copy_and_says_so() {
        // Nothing listening, a 500, a 503 while the service warms up, and a 429 from its rate limit
        // are all the same thing to a game in progress: the service cannot answer right now.
        let unreachable = "http://127.0.0.1:9".to_owned();
        for (name, base) in [
            ("refused", unreachable),
            ("500", serve(500, r#"{"error":"internal error"}"#).await),
            ("503", serve(503, r#"{"error":"data not available yet"}"#).await),
            ("429", serve(429, r#"{"error":"too many requests"}"#).await),
        ] {
            let dir = cache_dir(&format!("outage-{name}"));
            let client = AramkitClient::new(base, &dir);

            assert!(client.champions().await.is_err(), "{name}: nothing stored, so nothing to serve");

            client.cache.put("champions", OLDER.as_bytes()).await.unwrap();
            let fetched = client.champions().await.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(fetched.freshness, Freshness::Stale, "{name}");
            assert_eq!(fetched.value.patch, "16.18", "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[tokio::test]
    async fn a_refusal_is_an_answer_and_hides_nothing_behind_a_stored_copy() {
        let dir = cache_dir("refusal");
        let client = AramkitClient::new(serve(404, r#"{"error":"not found: no champion"}"#).await, &dir);
        client.cache.put("champion-999999", br#"{"patch":"16.18"}"#).await.unwrap();

        match client.champion(999_999).await {
            Err(ClientError::Status { status: 404, message, .. }) => {
                assert_eq!(message, "not found: no champion", "the service's own words are passed on")
            }
            other => panic!("expected the 404, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn an_answer_that_does_not_parse_is_an_error_and_is_not_stored() {
        let dir = cache_dir("garbled");
        let client = AramkitClient::new(serve(200, "<html>gateway</html>").await, &dir);

        assert!(matches!(client.champions().await, Err(ClientError::Decode { .. })));
        assert!(client.cache.get("champions").await.unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn damage_answers_are_not_kept_and_old_ones_are_cleared_out() {
        let dir = cache_dir("damage");
        let client = AramkitClient::new(serve(200, r#"{"patch":"16.19","missing":[7]}"#).await, &dir);

        // What earlier builds left behind, next to something still in use.
        for key in [
            "damage-1,2,3,4,5",
            "offer-157-2-1058,1134",
            "pool-157-gold-2",
            "build-157",
            "anvil-rankings",
            "champion-157",
        ] {
            client.cache.put(key, b"{}").await.unwrap();
        }
        assert_eq!(client.remove_obsolete_copies().await, 5);
        assert!(client.cache.get("champion-157").await.unwrap().is_some());

        assert_eq!(client.damage(&[1, 2, 3, 4, 5]).await.unwrap().missing, [7]);
        let stored: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        assert_eq!(stored.len(), 1, "only the champion bundle is left: {stored:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
