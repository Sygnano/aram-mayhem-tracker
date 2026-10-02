//! How an augment's numbers are presented: its rarity, the block it is ranked in, its place there
//! and its grade.
//!
//! These rules used to live in the overlay's TypeScript, where nothing tested them, next to a copy
//! of the ordering the service applies. They are here so the overlay only draws: every judgement
//! it shows was made, and is tested, on this side.

use serde::{Deserialize, Serialize};

/// An augment's rarity. The cards in an offer share one, except a golden reroll, which is
/// one step up.
///
/// CommunityDragon spells these `kGold`; our service takes and returns `gold`. Both parse, and the
/// service's spelling is the one written, so the conversion happens once, here, instead of each
/// side rebuilding the other's string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Rarity {
    Silver,
    Gold,
    Prismatic,
}

impl Rarity {
    /// Lowest first, which is also the order a golden reroll climbs.
    pub const ALL: [Rarity; 3] = [Rarity::Silver, Rarity::Gold, Rarity::Prismatic];

    /// `kGold`, `gold` and `Gold` are all Gold. Anything else (`kEventChoice`, an empty string) is
    /// not one of the three an offer can be.
    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let name = raw.strip_prefix('k').unwrap_or(raw);
        Self::ALL.into_iter().find(|r| r.as_str().eq_ignore_ascii_case(name))
    }

    /// The service's spelling: `silver`, `gold`, `prismatic`.
    pub fn as_str(self) -> &'static str {
        match self {
            Rarity::Silver => "silver",
            Rarity::Gold => "gold",
            Rarity::Prismatic => "prismatic",
        }
    }

    /// The rarity a golden reroll turns this one into. Prismatic has nothing above it.
    pub fn above(self) -> Option<Self> {
        match self {
            Rarity::Silver => Some(Rarity::Gold),
            Rarity::Gold => Some(Rarity::Prismatic),
            Rarity::Prismatic => None,
        }
    }
}

/// Which baseline an augment's delta is measured against, and so which list it is ranked in.
///
/// A delta against this champion's own win rate and one against the 50% population average are not
/// the same quantity: they are ranked separately and never mixed in one ordering. Declared
/// best first, so sorting by it puts champion-specific numbers ahead of all-champion ones, and
/// those ahead of nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Block {
    /// Numbers about this champion: the service's `stage` and `champion` sources.
    Champion,
    /// The all-champion fallback: `global`.
    Global,
    /// No data: `none`, or a source this build does not know.
    None,
}

impl Block {
    /// From the service's `source` field.
    pub fn from_source(source: &str) -> Self {
        match source {
            "stage" | "champion" => Block::Champion,
            "global" => Block::Global,
            _ => Block::None,
        }
    }
}

/// S to D, from the win-rate delta against the baseline. `Unknown` when there is no delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    S,
    A,
    B,
    C,
    D,
    #[serde(rename = "?")]
    Unknown,
}

impl Grade {
    /// The bands, in percentage points: S at +4.0 and above, A from +1.0, B strictly inside ±1.0,
    /// C down to but not including -4.0, D from there. So exactly -1.0 is a C and exactly -4.0 is
    /// a D.
    pub fn from_delta(delta_pp: Option<f64>) -> Self {
        match delta_pp {
            None => Grade::Unknown,
            Some(d) if d.is_nan() => Grade::Unknown,
            Some(d) if d >= 4.0 => Grade::S,
            Some(d) if d >= 1.0 => Grade::A,
            Some(d) if d > -1.0 => Grade::B,
            Some(d) if d > -4.0 => Grade::C,
            Some(_) => Grade::D,
        }
    }
}

/// Each entry's place within its own block, as `(rank, out of)`, given a list already in ranked
/// order. `None` for an entry with no data.
///
/// Ranking across blocks would make the denominator meaningless: "#41 of 80" reads like a
/// champion-specific standing when the row is really an all-champion fallback. So the fourth
/// champion-specific augment is #4 of however many champion-specific ones there are.
pub fn ranks_within_blocks(blocks: &[Block]) -> Vec<Option<(u32, u32)>> {
    let count = |block: Block| blocks.iter().filter(|b| **b == block).count() as u32;
    let (champion_total, global_total) = (count(Block::Champion), count(Block::Global));
    let (mut champion_seen, mut global_seen) = (0, 0);
    blocks
        .iter()
        .map(|block| match block {
            Block::Champion => {
                champion_seen += 1;
                Some((champion_seen, champion_total))
            }
            Block::Global => {
                global_seen += 1;
                Some((global_seen, global_total))
            }
            Block::None => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_spellings_of_a_rarity_parse_and_the_services_is_written() {
        assert_eq!(Rarity::parse("kGold"), Some(Rarity::Gold));
        assert_eq!(Rarity::parse("gold"), Some(Rarity::Gold));
        assert_eq!(Rarity::parse(" kSilver "), Some(Rarity::Silver));
        assert_eq!(Rarity::parse("kPrismatic"), Some(Rarity::Prismatic));
        assert_eq!(Rarity::parse("kEventChoice"), None);
        assert_eq!(Rarity::parse(""), None);
        assert_eq!(Rarity::parse("k"), None);
        // The service only accepts these three spellings; anything else is a 400.
        assert_eq!(Rarity::ALL.map(Rarity::as_str), ["silver", "gold", "prismatic"]);
        assert_eq!(serde_json::to_string(&Rarity::Prismatic).unwrap(), "\"prismatic\"");
    }

    #[test]
    fn only_the_next_rarity_up_can_be_a_golden_reroll() {
        assert_eq!(Rarity::Silver.above(), Some(Rarity::Gold));
        assert_eq!(Rarity::Gold.above(), Some(Rarity::Prismatic));
        assert_eq!(Rarity::Prismatic.above(), None);
        assert!(Rarity::Silver < Rarity::Gold && Rarity::Gold < Rarity::Prismatic, "the lowest sorts first");
    }

    #[test]
    fn grade_bands_have_their_edges_where_they_were_decided() {
        let grade = |d: f64| Grade::from_delta(Some(d));
        assert_eq!(grade(9.96), Grade::S);
        assert_eq!(grade(4.0), Grade::S, "S starts at +4.0");
        assert_eq!(grade(3.99), Grade::A);
        assert_eq!(grade(1.0), Grade::A, "A starts at +1.0");
        assert_eq!(grade(0.99), Grade::B);
        assert_eq!(grade(0.0), Grade::B);
        assert_eq!(grade(-0.99), Grade::B);
        assert_eq!(grade(-1.0), Grade::C, "exactly -1.0 is a C");
        assert_eq!(grade(-3.99), Grade::C);
        assert_eq!(grade(-4.0), Grade::D, "exactly -4.0 is a D");
        assert_eq!(grade(-12.0), Grade::D);
        assert_eq!(Grade::from_delta(None), Grade::Unknown);
        assert_eq!(Grade::from_delta(Some(f64::NAN)), Grade::Unknown);
        assert_eq!(serde_json::to_string(&Grade::Unknown).unwrap(), "\"?\"");
        assert_eq!(serde_json::to_string(&Grade::S).unwrap(), "\"S\"");
    }

    #[test]
    fn sources_fall_into_the_block_whose_baseline_they_share() {
        assert_eq!(Block::from_source("stage"), Block::Champion);
        assert_eq!(Block::from_source("champion"), Block::Champion);
        assert_eq!(Block::from_source("global"), Block::Global);
        assert_eq!(Block::from_source("none"), Block::None);
        assert_eq!(Block::from_source(""), Block::None, "a source nobody sent is no data");
        assert!(Block::Champion < Block::Global && Block::Global < Block::None, "best baseline first");
    }

    #[test]
    fn a_rank_is_a_place_within_its_own_block() {
        use Block::*;
        // The order a pool arrives in: champion-specific rows, then the fallbacks, then nothing.
        let ranks = ranks_within_blocks(&[Champion, Champion, Champion, Global, Global, None]);
        assert_eq!(ranks, [Some((1, 3)), Some((2, 3)), Some((3, 3)), Some((1, 2)), Some((2, 2)), Option::None]);

        // Interleaved (an offer merged from two pools): each block still counts only its own.
        let ranks = ranks_within_blocks(&[Global, Champion, None, Champion]);
        assert_eq!(ranks, [Some((1, 1)), Some((1, 2)), Option::None, Some((2, 2))]);
        assert!(ranks_within_blocks(&[]).is_empty());
    }
}
