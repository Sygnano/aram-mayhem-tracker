//! Maps a local monotonic clock onto the in-game clock.
//!
//! The Live Client Data API is polled a few times a second, but screen samples are taken on their
//! own schedule. Every sample has to be stamped in *game* seconds, so we keep the most recent
//! `(local, game)` pair and extrapolate between polls.

use crate::GameSeconds;

#[derive(Debug, Clone, Copy, Default)]
pub struct GameClock {
    anchor: Option<(f64, GameSeconds)>,
}

impl GameClock {
    /// Records a fresh reading. `local` is any monotonic seconds counter.
    pub fn observe(&mut self, local: f64, game_time: GameSeconds) {
        // A game time that jumps backwards means a new game (or a replay); just re-anchor.
        self.anchor = Some((local, game_time));
    }

    pub fn reset(&mut self) {
        self.anchor = None;
    }

    /// Game time at local instant `local`, extrapolated from the last reading.
    pub fn game_time_at(&self, local: f64) -> Option<GameSeconds> {
        self.anchor.map(|(l, g)| g + (local - l).max(0.0))
    }

    /// How stale the last reading is, in local seconds.
    pub fn staleness(&self, local: f64) -> Option<f64> {
        self.anchor.map(|(l, _)| local - l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrapolates_between_polls() {
        let mut c = GameClock::default();
        assert_eq!(c.game_time_at(10.0), None);
        c.observe(100.0, 60.0);
        assert_eq!(c.game_time_at(100.5), Some(60.5));
        // Never extrapolates backwards.
        assert_eq!(c.game_time_at(99.0), Some(60.0));
        assert_eq!(c.staleness(101.0), Some(1.0));
    }
}
