//! Stat anvil offers: deciding the tier from the numbers on the cards, and ranking.
//!
//! An anvil offer is three shard cards of one tier. The card's **name says which stat** it is but
//! never the tier: silver and gold share every name, and the game's display names do not even
//! match the data's. The **numbers settle the tier**, because Mayhem's shard values are fixed per
//! tier and differ between tiers for every stat.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::augments::OFFER_SLOTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnvilTier {
    Silver,
    Gold,
    Prismatic,
}

impl AnvilTier {
    pub const ALL: [AnvilTier; 3] = [AnvilTier::Silver, AnvilTier::Gold, AnvilTier::Prismatic];
}

/// One shard in one tier, as the service's `/v1/anvils` describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShardInfo {
    /// `ARAM_GoldStatAnvil_MR`. Rankings are stored against it.
    pub id: String,
    pub tier: AnvilTier,
    /// The stat, shared by the same shard across tiers (`MinMR`).
    pub kind: String,
    /// Display name in the game's locale.
    pub name: String,
    /// The numbers the card prints, in order: `[25, 25]` for a gold Might Shard.
    pub values: Vec<f64>,
}

/// What was read off one card: the stat its name matched, and every number on its value lines.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardEvidence {
    pub kind: Option<String>,
    pub numbers: Vec<f64>,
}

/// The outcome for a whole offer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnvilDecision {
    /// `None` when the cards did not agree on one. No labels are drawn then: a wrong rank is worse
    /// than none.
    pub tier: Option<AnvilTier>,
    /// The shard on each card, once the tier is known.
    pub shards: [Option<String>; OFFER_SLOTS],
    /// Why there is no tier, for the debug view.
    pub problem: Option<String>,
}

/// Every number in an OCR'd value line, in order.
///
/// `+O 45 Magic Resist.` gives `[45]`, `+ 15.0% Move Speed and -10% Size` gives `[15, 10]` (signs
/// are dropped: the card's minus is a dash OCR reads unreliably), and the French decimal comma in
/// `17,5 %` is read as a point.
pub fn numbers_in(text: &str) -> Vec<f64> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut s = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            s.push(chars[i]);
            i += 1;
        }
        // One decimal separator, only when a digit follows it (so "45." at a sentence end is 45).
        if i + 1 < chars.len() && matches!(chars[i], '.' | ',') && chars[i + 1].is_ascii_digit() {
            s.push('.');
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                s.push(chars[i]);
                i += 1;
            }
        }
        if let Ok(v) = s.parse() {
            out.push(v);
        }
    }
    out
}

fn same(a: f64, b: f64) -> bool {
    (a.abs() - b.abs()).abs() < 0.05
}

/// How many of a shard's values were read on its card.
fn evidence(shard: &ShardInfo, numbers: &[f64]) -> usize {
    shard.values.iter().filter(|v| numbers.iter().any(|n| same(**v, *n))).count()
}

/// The tier one card votes for, if it can tell.
///
/// A stat that exists in only one tier (Tenacity is prismatic-only) votes on its name alone.
/// Otherwise the shard whose values best match the numbers read wins, and a draw between tiers is
/// no vote at all. A stray number cannot outvote the real one: the real values score as well or
/// better, and an equal score is a draw.
fn card_vote(catalogue: &[ShardInfo], card: &CardEvidence) -> Option<AnvilTier> {
    let kind = card.kind.as_deref()?;
    let candidates: Vec<&ShardInfo> = catalogue.iter().filter(|s| s.kind == kind).collect();
    let first = candidates.first()?;
    if candidates.iter().all(|s| s.tier == first.tier) {
        return Some(first.tier);
    }
    let best = candidates.iter().map(|s| evidence(s, &card.numbers)).max()?;
    if best == 0 {
        return None;
    }
    let mut tiers: Vec<AnvilTier> =
        candidates.iter().filter(|s| evidence(s, &card.numbers) == best).map(|s| s.tier).collect();
    tiers.dedup();
    (tiers.len() == 1).then(|| tiers[0])
}

/// Decides the offer's tier from the three cards, then names the shard on each.
///
/// An offer is one tier, so the cards vote and the majority wins. One misread card (a `2` read for
/// a `25`) is outvoted by the other two. A draw, or no votes at all, decides nothing.
pub fn decide(catalogue: &[ShardInfo], cards: &[CardEvidence; OFFER_SLOTS]) -> AnvilDecision {
    let votes: Vec<AnvilTier> = cards.iter().filter_map(|c| card_vote(catalogue, c)).collect();
    let count = |t: AnvilTier| votes.iter().filter(|v| **v == t).count();
    let best = AnvilTier::ALL.iter().copied().max_by_key(|t| count(*t)).filter(|t| count(*t) > 0);
    let tier = best.filter(|b| AnvilTier::ALL.iter().all(|t| t == b || count(*t) < count(*b)));

    let Some(tier) = tier else {
        let problem = if votes.is_empty() {
            "no card's numbers matched a tier".to_owned()
        } else {
            format!("the cards disagree on the tier: {votes:?}")
        };
        return AnvilDecision { tier: None, shards: Default::default(), problem: Some(problem) };
    };

    let shards = std::array::from_fn(|i| {
        let kind = cards[i].kind.as_deref()?;
        catalogue.iter().find(|s| s.kind == kind && s.tier == tier).map(|s| s.id.clone())
    });
    AnvilDecision { tier: Some(tier), shards, problem: None }
}

/// Dense ranks from tie buckets: bucket `i` is rank `i + 1`, so `[[a], [b, c], [d]]` ranks a 1,
/// b and c 2, d 3. A shard in no bucket is unranked.
pub fn dense_ranks(buckets: &[Vec<String>]) -> HashMap<String, u32> {
    buckets.iter().enumerate().flat_map(|(i, bucket)| bucket.iter().map(move |id| (id.clone(), i as u32 + 1))).collect()
}

/// The two shards whose worth depends on who you are facing. Kinds, so the rule holds in every
/// tier that has them (silver and gold on 16.19).
pub const ARMOR_KIND: &str = "MinAR";
pub const MAGIC_RESIST_KIND: &str = "MinMR";

/// Which of Armor and Magic Resist the enemy team makes worth more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Resist {
    Armor,
    MagicResist,
}

impl Resist {
    /// From the enemy team's damage split. Magic Resist needs strictly more magic than physical,
    /// by any margin; an exact tie goes to Armor. True damage is ignored: neither stat stops it.
    pub fn against(physical: f64, magic: f64) -> Resist {
        if magic > physical {
            Resist::MagicResist
        } else {
            Resist::Armor
        }
    }
}

/// Splits every bucket that ties an Armor shard with a Magic Resist shard, putting `first` ahead.
///
/// The one that loses moves into a bucket of its own directly behind, so everything else in the
/// bucket keeps its rank beside the winner and every later rank moves down by one. A bucket holding
/// only one of the two is left alone: the rankings already said which they prefer.
pub fn break_resist_ties(buckets: &[Vec<String>], catalogue: &[ShardInfo], first: Resist) -> Vec<Vec<String>> {
    let (ahead, behind) = match first {
        Resist::Armor => (ARMOR_KIND, MAGIC_RESIST_KIND),
        Resist::MagicResist => (MAGIC_RESIST_KIND, ARMOR_KIND),
    };
    let is = |id: &String, kind: &str| catalogue.iter().any(|s| s.id == *id && s.kind == kind);
    let mut out = Vec::with_capacity(buckets.len() + 1);
    for bucket in buckets {
        let tied = bucket.iter().any(|id| is(id, ahead)) && bucket.iter().any(|id| is(id, behind));
        if !tied {
            out.push(bucket.clone());
            continue;
        }
        let (losers, rest): (Vec<String>, Vec<String>) = bucket.iter().cloned().partition(|id| is(id, behind));
        out.push(rest);
        out.push(losers);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shard(id: &str, tier: AnvilTier, kind: &str, values: &[f64]) -> ShardInfo {
        ShardInfo { id: id.into(), tier, kind: kind.into(), name: id.into(), values: values.to_vec() }
    }

    /// The shards on the reference screenshots, with their 16.19 values.
    fn catalogue() -> Vec<ShardInfo> {
        use AnvilTier::*;
        vec![
            shard("S_AR", Silver, "MinAR", &[12.0]),
            shard("G_AR", Gold, "MinAR", &[45.0]),
            shard("S_HP", Silver, "MinHP", &[110.0]),
            shard("G_HP", Gold, "MinHP", &[375.0]),
            shard("S_AP", Silver, "MinAP", &[15.0]),
            shard("G_AP", Gold, "MinAP", &[50.0]),
            shard("S_MR", Silver, "MinMR", &[14.0]),
            shard("G_MR", Gold, "MinMR", &[45.0]),
            shard("S_Might", Silver, "MinAD+MinAP", &[8.0, 12.0]),
            shard("G_Might", Gold, "MinAD+MinAP", &[25.0, 25.0]),
            shard("S_MPen", Silver, "MinMPen", &[8.0]),
            shard("G_MPen", Gold, "MinMPen", &[18.0]),
            shard("P_MPen", Prismatic, "MinMPen", &[17.5]),
            shard("P_HS", Prismatic, "MinHS", &[25.0]),
            shard("P_MS", Prismatic, "MinMS+SizeMod", &[15.0, -10.0]),
            shard("P_Ten", Prismatic, "MinTenacity", &[30.0]),
        ]
    }

    fn card(kind: &str, value_lines: &[&str]) -> CardEvidence {
        CardEvidence { kind: Some(kind.into()), numbers: value_lines.iter().flat_map(|l| numbers_in(l)).collect() }
    }

    #[test]
    fn numbers_are_read_through_ocr_noise() {
        assert_eq!(numbers_in("+O 45 Magic Resist."), vec![45.0]);
        assert_eq!(numbers_in("+ 18Magic Penetrtion."), vec![18.0]);
        assert_eq!(numbers_in("+ 15.0% Move Speed and -10% Size"), vec![15.0, 10.0]);
        assert_eq!(numbers_in("+M 25% Crit Chance. :"), vec![25.0]);
        assert_eq!(numbers_in("+ 17,5 % de pénétration magique"), vec![17.5]);
        assert_eq!(numbers_in("版商"), Vec::<f64>::new());
    }

    /// The OCR output measured on the 1280×1024 gold screenshot, including its misread second line.
    #[test]
    fn the_gold_screenshot_reads_as_gold() {
        let cards = [
            card("MinMPen", &["+ 18 Magic Peneration.", ""]),
            card("MinMR", &["+O 45 Magic Resist.", ""]),
            card("MinAD+MinAP", &["+ 25 Attack Damage.", "+ 2 Ability Power."]),
        ];
        let d = decide(&catalogue(), &cards);
        assert_eq!(d.tier, Some(AnvilTier::Gold));
        assert_eq!(d.shards, [Some("G_MPen".into()), Some("G_MR".into()), Some("G_Might".into())]);
    }

    #[test]
    fn the_silver_and_prismatic_screenshots_read_as_theirs() {
        let silver = [
            card("MinAR", &["+12 Armor.", "版商"]),
            card("MinHP", &["+110 Health.", "版南"]),
            card("MinAP", &["+ 15 Ability Power.", "版南"]),
        ];
        assert_eq!(decide(&catalogue(), &silver).tier, Some(AnvilTier::Silver));

        let prismatic = [
            card("MinHS", &["+25% Heal & Shield Power", ""]),
            card("MinMS+SizeMod", &["+ 15.0% Move Speed and -10% Size", ""]),
            card("MinTenacity", &["+ 30.0% Tenacity", ""]),
        ];
        let d = decide(&catalogue(), &prismatic);
        assert_eq!(d.tier, Some(AnvilTier::Prismatic));
        assert_eq!(d.shards, [Some("P_HS".into()), Some("P_MS".into()), Some("P_Ten".into())]);
    }

    #[test]
    fn prismatic_magic_pen_is_told_apart_by_its_decimal() {
        let cards = [card("MinMPen", &["+ 17.5% Magic Penetration"]), Default::default(), Default::default()];
        assert_eq!(decide(&catalogue(), &cards).tier, Some(AnvilTier::Prismatic));
    }

    #[test]
    fn one_misread_card_is_outvoted() {
        // The middle card's 45 was read as 14, a silver value; the other two say gold.
        let cards = [
            card("MinAR", &["+ 45 Armor"]),
            card("MinMR", &["+ 14 Magic Resist"]),
            card("MinAP", &["+ 50 Ability Power"]),
        ];
        let d = decide(&catalogue(), &cards);
        assert_eq!(d.tier, Some(AnvilTier::Gold));
        assert_eq!(d.shards[1], Some("G_MR".into()), "the outvoted card still names its gold shard");
    }

    #[test]
    fn a_draw_or_nothing_readable_decides_nothing() {
        let draw = [card("MinAR", &["+ 45"]), card("MinMR", &["+ 14"]), Default::default()];
        let d = decide(&catalogue(), &draw);
        assert_eq!(d.tier, None);
        assert_eq!(d.shards, [None, None, None]);
        assert!(d.problem.is_some());

        let blank = [card("MinAR", &[""]), card("MinMR", &[""]), card("MinAP", &[""])];
        assert_eq!(decide(&catalogue(), &blank).tier, None);
    }

    #[test]
    fn magic_resist_needs_strictly_more_magic_and_a_tie_goes_to_armor() {
        assert_eq!(Resist::against(0.4489, 0.5015), Resist::MagicResist);
        assert_eq!(Resist::against(0.4999, 0.5001), Resist::MagicResist);
        assert_eq!(Resist::against(0.475, 0.475), Resist::Armor);
        assert_eq!(Resist::against(0.6, 0.35), Resist::Armor);
    }

    #[test]
    fn an_armor_and_magic_resist_tie_is_split_by_the_enemy_team() {
        let ids = |b: &[&str]| b.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // The shape of the saved Mages ranking: Health, then Armor and Magic Resist tied, then AP.
        let buckets = vec![ids(&["G_HP"]), ids(&["G_AR", "G_MR"]), ids(&["G_AP"])];

        let r = dense_ranks(&break_resist_ties(&buckets, &catalogue(), Resist::MagicResist));
        assert_eq!((r["G_HP"], r["G_MR"], r["G_AR"], r["G_AP"]), (1, 2, 3, 4));
        let r = dense_ranks(&break_resist_ties(&buckets, &catalogue(), Resist::Armor));
        assert_eq!((r["G_HP"], r["G_AR"], r["G_MR"], r["G_AP"]), (1, 2, 3, 4));

        // A third shard in the tie stays level with the winner.
        let three = vec![ids(&["G_AR", "G_HP", "G_MR"])];
        let r = dense_ranks(&break_resist_ties(&three, &catalogue(), Resist::Armor));
        assert_eq!((r["G_AR"], r["G_HP"], r["G_MR"]), (1, 1, 2));

        // Ranked apart on purpose: the enemy team does not overrule the rankings.
        let apart = vec![ids(&["G_MR"]), ids(&["G_AR"])];
        assert_eq!(break_resist_ties(&apart, &catalogue(), Resist::Armor), apart);
    }

    #[test]
    fn ties_share_a_rank_and_ranks_stay_dense() {
        let buckets = vec![vec!["a".to_owned()], vec!["b".into(), "c".into()], vec!["d".into()]];
        let r = dense_ranks(&buckets);
        assert_eq!((r["a"], r["b"], r["c"], r["d"]), (1, 2, 2, 3));
        assert!(!r.contains_key("e"));
    }
}
