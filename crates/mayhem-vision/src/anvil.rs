//! Reading stat anvil offers.
//!
//! A shard card sits in the augment card's frame, at the augment card's position, so the augment
//! pipeline finds it and crops its title unchanged. What differs is what the title is matched
//! against and what happens next:
//!
//! 1. The titles are matched against the augment names **and** the shard names. An offer is all
//!    augments or all shards, so whichever matches more cards decides which it is.
//! 2. A shard name only gives the **stat** — every tier has an "Armor Shard" — so the matcher's
//!    vocabulary is keyed by stat (`kind`), with every tier's spelling as an alias.
//! 3. For an anvil, the value lines are read and [`mayhem_core::anvils::decide`] settles the tier
//!    from the numbers.

use mayhem_core::anvils::{decide, numbers_in, AnvilDecision, CardEvidence, ShardInfo};
use mayhem_core::augments::OFFER_SLOTS;
use serde::{Deserialize, Serialize};

use crate::matcher::{AugmentMatcher, AugmentName, MatcherConfig, NameMatch};
use crate::ocr::{OcrEngine, OcrText};
use crate::{RgbaImage, VisionError};

/// Shard names, matched to the stat they grant.
///
/// Built on [`AugmentMatcher`] with one synthetic id per stat. Names that several tiers share are
/// one entry, and different spellings of one stat ("Magic Resist Shard" in game, "Magic Resistance
/// Shard" in the data for gold) are aliases of the same id, so they never compete with each other
/// and the matcher's runner-up margin only ever weighs genuinely different stats.
#[derive(Debug, Clone)]
pub struct ShardMatcher {
    inner: AugmentMatcher,
    kinds: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShardMatch {
    pub kind: String,
    pub score: f32,
}

impl ShardMatcher {
    pub fn new(cfg: MatcherConfig, shards: &[ShardInfo]) -> Self {
        let mut kinds: Vec<String> = Vec::new();
        let mut names: Vec<AugmentName> = Vec::new();
        for shard in shards {
            let index = match kinds.iter().position(|k| *k == shard.kind) {
                Some(i) => i,
                None => {
                    kinds.push(shard.kind.clone());
                    kinds.len() - 1
                }
            };
            let id = index as i64;
            if !names.iter().any(|n| n.id == id && n.name == shard.name) {
                names.push(AugmentName { id, name: shard.name.clone(), rarity: String::new() });
            }
        }
        Self { inner: AugmentMatcher::new(cfg, &names), kinds }
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// The stat on each card. The same stat is never offered twice, so a duplicate keeps the
    /// stronger reading, exactly as for augments.
    pub fn match_offer(&self, texts: &[String; OFFER_SLOTS]) -> [Option<ShardMatch>; OFFER_SLOTS] {
        self.inner.match_offer(texts).map(|m| {
            m.and_then(|m| {
                Some(ShardMatch { kind: self.kinds.get(usize::try_from(m.id).ok()?)?.clone(), score: m.score })
            })
        })
    }
}

/// Is this an anvil offer? Each card counts for whichever vocabulary matched it better, and the
/// offer is an anvil when more cards are shards than augments.
///
/// No augment is named "… Shard" today, so in practice one side matches and the other does not;
/// counting rather than requiring all three keeps one unreadable card from flipping the answer.
pub fn is_anvil_offer(augments: &[Option<NameMatch>; OFFER_SLOTS], shards: &[Option<ShardMatch>; OFFER_SLOTS]) -> bool {
    let (mut shard_cards, mut augment_cards) = (0, 0);
    for (a, s) in augments.iter().zip(shards) {
        match (a, s) {
            (Some(a), Some(s)) if s.score > a.score => shard_cards += 1,
            (Some(_), _) => augment_cards += 1,
            (None, Some(_)) => shard_cards += 1,
            (None, None) => {}
        }
    }
    shard_cards > augment_cards
}

/// One anvil offer as read, with the raw text kept for the settings screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnvilRead {
    pub title_texts: [String; OFFER_SLOTS],
    pub value_texts: [[String; 2]; OFFER_SLOTS],
    pub matches: [Option<ShardMatch>; OFFER_SLOTS],
    pub decision: AnvilDecision,
}

/// Reads the value lines (one OCR batch of six crops) and decides the tier.
///
/// `title_texts` and `matches` come from the title pass that already ran, so titles are never read
/// twice.
pub fn read_anvil(
    engine: &dyn OcrEngine,
    catalogue: &[ShardInfo],
    title_texts: &[String; OFFER_SLOTS],
    matches: [Option<ShardMatch>; OFFER_SLOTS],
    values: &[[RgbaImage; 2]; OFFER_SLOTS],
) -> Result<AnvilRead, VisionError> {
    let crops: Vec<RgbaImage> = values.iter().flat_map(|pair| pair.iter().cloned()).collect();
    let read = engine.recognize_all(&crops)?;
    let text = |i: usize| read.get(i).map(OcrText::joined).unwrap_or_default();
    let value_texts: [[String; 2]; OFFER_SLOTS] = std::array::from_fn(|card| [text(2 * card), text(2 * card + 1)]);

    let evidence: [CardEvidence; OFFER_SLOTS] = std::array::from_fn(|i| CardEvidence {
        kind: matches[i].as_ref().map(|m| m.kind.clone()),
        numbers: value_texts[i].iter().flat_map(|t| numbers_in(t)).collect(),
    });
    let decision = decide(catalogue, &evidence);
    Ok(AnvilRead { title_texts: title_texts.clone(), value_texts, matches, decision })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mayhem_core::anvils::AnvilTier;

    fn shard(id: &str, tier: AnvilTier, kind: &str, name: &str, values: &[f64]) -> ShardInfo {
        ShardInfo { id: id.into(), tier, kind: kind.into(), name: name.into(), values: values.to_vec() }
    }

    fn catalogue() -> Vec<ShardInfo> {
        use AnvilTier::*;
        vec![
            shard("S_MR", Silver, "MinMR", "Magic Resist Shard", &[14.0]),
            shard("G_MR", Gold, "MinMR", "Magic Resistance Shard", &[45.0]),
            shard("S_MPen", Silver, "MinMPen", "Magic Penetration Shard", &[8.0]),
            shard("G_MPen", Gold, "MinMPen", "Magic Penetration Shard", &[18.0]),
            shard("P_MPen", Prismatic, "MinMPen", "Magic Penetration Shard", &[17.5]),
            shard("S_Might", Silver, "MinAD+MinAP", "Might Shard", &[8.0, 12.0]),
            shard("G_Might", Gold, "MinAD+MinAP", "Might Shard", &[25.0, 25.0]),
        ]
    }

    fn texts(a: &str, b: &str, c: &str) -> [String; OFFER_SLOTS] {
        [a.into(), b.into(), c.into()]
    }

    #[test]
    fn a_name_shared_by_every_tier_still_matches_its_stat() {
        // Three entries named "Magic Penetration Shard" must not count as each other's runner-up.
        let m = ShardMatcher::new(MatcherConfig::default(), &catalogue());
        let got = m.match_offer(&texts("Magic Penetration Shard", "Magic Resist Shard", "Might Shard"));
        let kinds: Vec<_> = got.iter().map(|g| g.as_ref().map(|g| g.kind.as_str())).collect();
        assert_eq!(kinds, vec![Some("MinMPen"), Some("MinMR"), Some("MinAD+MinAP")]);
    }

    #[test]
    fn either_spelling_of_a_stat_lands_on_that_stat() {
        let m = ShardMatcher::new(MatcherConfig::default(), &catalogue());
        let got = m.match_offer(&texts("Magic Resistance Shard", "Magic Resist Shard", ""));
        assert_eq!(got[0].as_ref().unwrap().kind, "MinMR");
        // The same stat twice in one offer never happens, so the weaker duplicate is dropped.
        assert!(got[1].is_none());
    }

    #[test]
    fn an_offer_is_whichever_vocabulary_matches_more_cards() {
        let shard = |s| Some(ShardMatch { kind: "k".into(), score: s });
        let aug = |s| Some(NameMatch { id: 1, score: s, runner_up: 0.0 });
        assert!(is_anvil_offer(&[None, None, None], &[shard(0.9), shard(0.9), None]));
        assert!(!is_anvil_offer(&[aug(0.9), aug(0.9), None], &[None, None, shard(0.8)]));
        assert!(!is_anvil_offer(&[None, None, None], &[None, None, None]));
        // A card both vocabularies matched counts for the better match.
        assert!(is_anvil_offer(&[aug(0.75), None, None], &[shard(0.95), None, None]));
    }

    #[test]
    fn values_decide_the_tier_through_the_ocr_engine() {
        use crate::ocr::ScriptedOcr;
        let cat = catalogue();
        let m = ShardMatcher::new(MatcherConfig::default(), &cat);
        let titles = texts("Magic Penetration Shard", "Magic Resist Shard", "Might Shard");
        let matches = m.match_offer(&titles);
        // Six crops, card by card, line one then line two: the 1280×1024 gold screenshot's output.
        let ocr = ScriptedOcr::new([
            "+ 18 Magic Peneration.",
            "",
            "+O 45 Magic Resist.",
            "",
            "+ 25 Attack Damage.",
            "+ 2 Ability Power.",
        ]);
        let blank = || [RgbaImage::new(4, 4), RgbaImage::new(4, 4)];
        let values = [blank(), blank(), blank()];
        let read = read_anvil(&ocr, &cat, &titles, matches, &values).unwrap();
        assert_eq!(read.decision.tier, Some(AnvilTier::Gold));
        assert_eq!(read.decision.shards, [Some("G_MPen".into()), Some("G_MR".into()), Some("G_Might".into())]);
        assert_eq!(read.value_texts[2][1], "+ 2 Ability Power.");
    }
}
