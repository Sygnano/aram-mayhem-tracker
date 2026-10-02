//! Augment offers: *when* one is due (from the Live Client Data API, used only to label stages)
//! and *what* is on screen (from OCR readings), reduced to a small stream of events.

use serde::{Deserialize, Serialize};

use crate::GameSeconds;

/// Riot augment id, as in CommunityDragon `cherry-augments.json`.
pub type AugmentId = i64;

/// Number of cards in an offer.
pub const OFFER_SLOTS: usize = 3;

/// A point in the game at which an augment offer is unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AugmentTrigger {
    GameStart,
    Level { level: u8 },
}

/// When offers unlock: level 1 (game start), 7, 11 and 15 — confirmed in game (2026-09-29), with
/// one reroll per card. Only used to number stages; offers themselves are detected on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AugmentSchedule {
    pub triggers: Vec<AugmentTrigger>,
}

impl Default for AugmentSchedule {
    fn default() -> Self {
        Self {
            triggers: vec![
                AugmentTrigger::GameStart,
                AugmentTrigger::Level { level: 7 },
                AugmentTrigger::Level { level: 11 },
                AugmentTrigger::Level { level: 15 },
            ],
        }
    }
}

impl AugmentSchedule {
    /// How many offers a champion of `level` has unlocked so far.
    pub fn stages_unlocked(&self, level: u8) -> u8 {
        self.triggers
            .iter()
            .filter(|t| match t {
                AugmentTrigger::GameStart => true,
                AugmentTrigger::Level { level: l } => level >= *l,
            })
            .count() as u8
    }
}

/// One OCR pass over the three card title regions, after name matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OfferReading {
    pub slots: [Option<AugmentId>; OFFER_SLOTS],
}

impl OfferReading {
    pub fn recognised(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// 1-based offer number within the game.
    pub stage: u8,
    pub slots: [Option<AugmentId>; OFFER_SLOTS],
    pub shown_at: GameSeconds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum OfferEvent {
    Shown {
        offer: Offer,
    },
    /// One or more cards changed while the offer stayed open (a reroll).
    Changed {
        offer: Offer,
    },
    Closed {
        offer: Offer,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OfferTrackerConfig {
    /// Cards that must be recognised for a reading to count as "offer visible".
    pub min_recognised: usize,
    /// Consecutive visible readings before an offer is announced.
    pub show_after: u32,
    /// Consecutive non-visible readings before an offer is considered closed.
    pub close_after: u32,
    /// Consecutive readings that must agree before a single card is considered rerolled.
    pub change_after: u32,
}

impl Default for OfferTrackerConfig {
    fn default() -> Self {
        Self { min_recognised: 2, show_after: 2, close_after: 3, change_after: 2 }
    }
}

/// Turns noisy per-frame readings into offer events. Tolerates a card that fails to read on some
/// frames (the previous value is kept) and single-frame misreads (changes need agreement).
#[derive(Debug, Clone, Default)]
pub struct OfferTracker {
    cfg: OfferTrackerConfig,
    current: Option<Offer>,
    stages_seen: u8,
    visible_streak: u32,
    hidden_streak: u32,
    pending_change: [Option<(AugmentId, u32)>; OFFER_SLOTS],
    first_candidate: Option<OfferReading>,
    /// The last offer that closed. The in-game button hides and re-shows the cards without
    /// picking, so an offer that reappears with the same cards is the same stage, not a new one.
    last_closed: Option<Offer>,
    /// Every augment that has been on screen this game.
    ///
    /// An augment you rerolled away, and the two you did not take when you picked, are gone from
    /// the pool for the rest of the game - and so is the one you took, since you have it. So every
    /// id that has ever appeared can be struck off the list, which is why this does not need to
    /// know *which* card was picked. Nothing in the allowed data sources reports that anyway.
    seen: std::collections::BTreeSet<AugmentId>,
}

impl OfferTracker {
    pub fn new(cfg: OfferTrackerConfig) -> Self {
        Self { cfg, ..Default::default() }
    }

    pub fn current(&self) -> Option<&Offer> {
        self.current.as_ref()
    }

    pub fn stages_seen(&self) -> u8 {
        self.stages_seen
    }

    /// Every augment seen on screen this game, rerolled or not.
    pub fn seen_augments(&self) -> &std::collections::BTreeSet<AugmentId> {
        &self.seen
    }

    fn remember(&mut self, slots: &[Option<AugmentId>; OFFER_SLOTS]) {
        self.seen.extend(slots.iter().flatten().copied());
    }

    /// Feed a reading taken at `at`. `stages_unlocked` comes from [`AugmentSchedule`]; it only
    /// matters when the app was started mid-game and missed earlier offers.
    pub fn feed(&mut self, at: GameSeconds, reading: OfferReading, stages_unlocked: u8) -> Option<OfferEvent> {
        let visible = reading.recognised() >= self.cfg.min_recognised;
        if visible {
            self.visible_streak += 1;
            self.hidden_streak = 0;
        } else {
            self.hidden_streak += 1;
            self.visible_streak = 0;
        }

        match self.current.take() {
            None => {
                if !visible {
                    self.first_candidate = None;
                    return None;
                }
                let merged = match self.first_candidate {
                    Some(prev) => merge(prev, reading),
                    None => reading,
                };
                self.first_candidate = Some(merged);
                if self.visible_streak < self.cfg.show_after {
                    return None;
                }
                let reopened = self.last_closed.as_ref().filter(|last| same_cards(&last.slots, &merged.slots));
                let stage = match reopened {
                    Some(last) => last.stage,
                    None => (self.stages_seen + 1).max(stages_unlocked).max(1),
                };
                let offer = Offer { stage, slots: merged.slots, shown_at: at };
                self.first_candidate = None;
                self.pending_change = Default::default();
                self.remember(&offer.slots);
                self.current = Some(offer.clone());
                Some(OfferEvent::Shown { offer })
            }
            Some(mut offer) => {
                if !visible {
                    if self.hidden_streak >= self.cfg.close_after {
                        self.stages_seen = self.stages_seen.max(offer.stage);
                        self.pending_change = Default::default();
                        self.last_closed = Some(offer.clone());
                        return Some(OfferEvent::Closed { offer });
                    }
                    self.current = Some(offer);
                    return None;
                }
                let mut changed = false;
                for i in 0..OFFER_SLOTS {
                    match (offer.slots[i], reading.slots[i]) {
                        (_, None) => {}
                        (None, Some(new)) => {
                            offer.slots[i] = Some(new);
                            changed = true;
                        }
                        (Some(old), Some(new)) if old == new => self.pending_change[i] = None,
                        (Some(_), Some(new)) => {
                            let count = match self.pending_change[i] {
                                Some((id, n)) if id == new => n + 1,
                                _ => 1,
                            };
                            if count >= self.cfg.change_after {
                                offer.slots[i] = Some(new);
                                self.pending_change[i] = None;
                                changed = true;
                            } else {
                                self.pending_change[i] = Some((new, count));
                            }
                        }
                    }
                }
                self.remember(&offer.slots);
                self.current = Some(offer.clone());
                changed.then_some(OfferEvent::Changed { offer })
            }
        }
    }

    /// Forget everything; call on game end.
    pub fn reset(&mut self) {
        *self = Self::new(self.cfg);
    }
}

/// At least two cards identical in the same position.
fn same_cards(a: &[Option<AugmentId>; OFFER_SLOTS], b: &[Option<AugmentId>; OFFER_SLOTS]) -> bool {
    a.iter().zip(b).filter(|(x, y)| x.is_some() && x == y).count() >= 2
}

fn merge(prev: OfferReading, next: OfferReading) -> OfferReading {
    let mut out = next;
    for i in 0..OFFER_SLOTS {
        if out.slots[i].is_none() {
            out.slots[i] = prev.slots[i];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rerolled card is gone from the pool for the rest of the game, and so are the cards you did
    /// not take, so every id that has ever been on screen is remembered.
    #[test]
    fn every_augment_ever_shown_is_remembered() {
        let mut t = OfferTracker::new(OfferTrackerConfig::default());
        let show = |ids: [Option<AugmentId>; OFFER_SLOTS]| OfferReading { slots: ids };

        for _ in 0..2 {
            t.feed(10.0, show([Some(1), Some(2), Some(3)]), 1);
        }
        assert_eq!(t.seen_augments().iter().copied().collect::<Vec<_>>(), [1, 2, 3]);

        // Reroll the third card: both the old and the new id are out of the pool.
        for _ in 0..3 {
            t.feed(12.0, show([Some(1), Some(2), Some(9)]), 1);
        }
        assert_eq!(t.seen_augments().iter().copied().collect::<Vec<_>>(), [1, 2, 3, 9]);

        // A later stage adds to the set rather than replacing it.
        for _ in 0..4 {
            t.feed(30.0, show([None, None, None]), 1);
        }
        for _ in 0..2 {
            t.feed(40.0, show([Some(4), Some(5), Some(6)]), 2);
        }
        assert_eq!(t.seen_augments().iter().copied().collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6, 9]);

        t.reset();
        assert!(t.seen_augments().is_empty(), "a new game starts from nothing");
    }

    fn r(a: Option<i64>, b: Option<i64>, c: Option<i64>) -> OfferReading {
        OfferReading { slots: [a, b, c] }
    }

    #[test]
    fn schedule_counts_unlocked_stages() {
        let s = AugmentSchedule::default();
        assert_eq!(s.stages_unlocked(1), 1);
        assert_eq!(s.stages_unlocked(7), 2);
        assert_eq!(s.stages_unlocked(14), 3);
        assert_eq!(s.stages_unlocked(18), 4);
    }

    #[test]
    fn show_reroll_close_cycle() {
        let mut t = OfferTracker::new(OfferTrackerConfig::default());
        assert_eq!(t.feed(1.0, r(Some(1), None, Some(3)), 1), None);
        // Second visible reading fills the missing card and announces the offer.
        let ev = t.feed(1.5, r(None, Some(2), Some(3)), 1).unwrap();
        assert_eq!(
            ev,
            OfferEvent::Shown { offer: Offer { stage: 1, slots: [Some(1), Some(2), Some(3)], shown_at: 1.5 } }
        );

        // A single-frame misread of card 2 is ignored...
        assert_eq!(t.feed(2.0, r(Some(1), Some(9), Some(3)), 1), None);
        assert_eq!(t.feed(2.5, r(Some(1), Some(2), Some(3)), 1), None);
        // ...but a consistent change is a reroll.
        assert_eq!(t.feed(3.0, r(Some(1), Some(7), None), 1), None);
        match t.feed(3.5, r(Some(1), Some(7), Some(3)), 1) {
            Some(OfferEvent::Changed { offer }) => assert_eq!(offer.slots, [Some(1), Some(7), Some(3)]),
            other => panic!("expected Changed, got {other:?}"),
        }

        // Closing needs three misses; one blip does not close it.
        assert_eq!(t.feed(4.0, r(None, None, None), 1), None);
        assert_eq!(t.feed(4.5, r(Some(1), Some(7), Some(3)), 1), None);
        assert_eq!(t.feed(5.0, r(None, None, None), 1), None);
        assert_eq!(t.feed(5.5, r(None, None, None), 1), None);
        assert!(matches!(t.feed(6.0, r(None, None, None), 1), Some(OfferEvent::Closed { .. })));
        assert_eq!(t.stages_seen(), 1);
        assert!(t.current().is_none());
    }

    #[test]
    fn stage_numbering_catches_up_when_started_mid_game() {
        let mut t = OfferTracker::new(OfferTrackerConfig::default());
        t.feed(900.0, r(Some(1), Some(2), Some(3)), 3);
        match t.feed(900.5, r(Some(1), Some(2), Some(3)), 3) {
            Some(OfferEvent::Shown { offer }) => assert_eq!(offer.stage, 3),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hiding_and_reshowing_keeps_the_stage() {
        let mut t = OfferTracker::new(OfferTrackerConfig::default());
        let offer = r(Some(1), Some(2), Some(3));
        let hidden = r(None, None, None);
        t.feed(0.0, offer, 1);
        assert!(matches!(t.feed(0.5, offer, 1), Some(OfferEvent::Shown { offer: Offer { stage: 1, .. } })));
        for i in 0..3 {
            t.feed(1.0 + i as f64, hidden, 1);
        }
        assert!(t.current().is_none());
        // Same cards again (one misread): still stage 1.
        t.feed(10.0, r(Some(1), Some(2), None), 1);
        match t.feed(10.5, offer, 1) {
            Some(OfferEvent::Shown { offer }) => assert_eq!(offer.stage, 1),
            other => panic!("{other:?}"),
        }
        // Closed again, then different cards: the next stage.
        for i in 0..3 {
            t.feed(11.0 + i as f64, hidden, 1);
        }
        let next = r(Some(7), Some(8), Some(9));
        t.feed(400.0, next, 1);
        match t.feed(400.5, next, 1) {
            Some(OfferEvent::Shown { offer }) => assert_eq!(offer.stage, 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn one_recognised_card_is_not_an_offer() {
        let mut t = OfferTracker::new(OfferTrackerConfig::default());
        for i in 0..5 {
            assert_eq!(t.feed(i as f64, r(Some(1), None, None), 1), None);
        }
    }
}
