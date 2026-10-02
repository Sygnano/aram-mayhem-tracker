//! CommunityDragon static data.
//!
//! - The Mayhem pool is `augment-lists.json` (`modeName` `KIWI` / `KIWI_JADE`) joined to
//!   `cherry-augments.json` by name id. Never `cdragon/arena/en_us.json`, which has no Mayhem
//!   augments at all.
//! - Everything is pinned by patch and cached on disk per patch and locale;
//!   `latest` is only used to discover the current patch.
//! - Pools come from data, never from a hardcoded list.

pub mod augments;
pub mod cdragon;
pub mod champions;

pub use augments::{build_pool, AugmentList, CherryAugment, PoolAugment};
pub use cdragon::{CDragon, StaticData};
pub use champions::ChampionSummary;

#[derive(Debug, thiserror::Error)]
pub enum StaticDataError {
    #[error("HTTP error fetching {url}: {message}")]
    Http { url: String, message: String },
    #[error("unexpected JSON in {file}: {message}")]
    Decode { file: String, message: String },
    #[error("cache I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("mode pool {0} not found in augment-lists.json")]
    MissingPool(String),
}
