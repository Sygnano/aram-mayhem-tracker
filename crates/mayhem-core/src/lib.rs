//! Domain logic for ARAM Mayhem Tracker: queues, the game clock, augment offers.
//!
//! Everything in this crate is pure: no I/O, no clocks, no platform APIs. Time is always passed in
//! explicitly as game seconds, so every state machine here is deterministic and unit-testable
//! against recorded observations.

pub mod anvils;
pub mod augments;
pub mod champselect;
pub mod clock;
pub mod geometry;
pub mod layout;
pub mod queues;
pub mod ranking;

/// Seconds on the in-game clock (`gameData.gameTime` in the Live Client Data API).
pub type GameSeconds = f64;
