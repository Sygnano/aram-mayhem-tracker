//! Adapters for League's local APIs.
//!
//! Why not Irelia: it is maintained and pins the same Riot root certificate, but its
//! in-game types are strict (fixed-size item arrays, closed enums for terrain), so a single
//! unexpected Mayhem field would fail the whole `/allgamedata` parse. We use a handful of
//! endpoints, so we keep thin adapters with *lenient* types: every field defaults and only what we
//! read is modelled.

pub mod lcd;
pub mod lcu;
pub mod lockfile;
mod tls;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Nothing is listening: the normal idle state, not a failure.
    #[error("not running")]
    NotRunning,
    #[error("HTTP {status} from {path}")]
    Status { status: u16, path: String },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("unexpected response shape: {0}")]
    Decode(String),
}

impl ApiError {
    fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_connect() {
            ApiError::NotRunning
        } else if e.is_decode() {
            ApiError::Decode(e.to_string())
        } else {
            ApiError::Transport(e.to_string())
        }
    }
}
