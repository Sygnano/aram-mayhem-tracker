//! Matches OCR output against the closed vocabulary of augment names.
//!
//! This is what makes OCR usable at all: we never need to *read* an arbitrary string, only decide
//! which of ~223 known names a noisy reading is closest to. Stylised fonts, a stray glyph from the
//! card frame, or two words run together still land on the right name, and anything that is not
//! clearly one name is rejected rather than guessed.

use mayhem_core::augments::{AugmentId, OFFER_SLOTS};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// One vocabulary entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AugmentName {
    pub id: AugmentId,
    /// Display name in the game's locale.
    pub name: String,
    /// Rarity as in `cherry-augments.json` (`kSilver`, `kGold`, `kPrismatic`, ...).
    pub rarity: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MatcherConfig {
    /// Minimum similarity (0..1) for a match.
    pub min_score: f32,
    /// Minimum lead over the runner-up, unless the match is exact.
    pub min_margin: f32,
    /// Names shorter than this (after normalisation) must match the whole reading, not a part of
    /// it, so "Tank" cannot be found inside unrelated text.
    pub min_len_for_partial: usize,
    /// Score bonus for candidates in the tier the offer is believed to be, used only to break ties
    /// on a reading that plain name matching could not settle.
    pub rarity_bonus: f32,
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self { min_score: 0.72, min_margin: 0.06, min_len_for_partial: 7, rarity_bonus: 0.04 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameMatch {
    pub id: AugmentId,
    pub score: f32,
    pub runner_up: f32,
}

#[derive(Debug, Clone)]
struct Entry {
    id: AugmentId,
    rarity: String,
    key: Vec<char>,
}

#[derive(Debug, Clone)]
pub struct AugmentMatcher {
    cfg: MatcherConfig,
    entries: Vec<Entry>,
}

/// Canonical form for comparison: NFKD with combining marks dropped (so "É" matches an OCR'd
/// "E"), lowercase, common OCR confusions folded, and only letters and digits kept. Spaces are
/// dropped too, since OCR splits and joins words unreliably. Ligatures, which NFKD leaves whole, are
/// spelt out ("cœur" → "coeur").
pub fn normalize(text: &str) -> Vec<char> {
    text.nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .flat_map(|c| match c {
            'œ' => ['o', 'e'].into_iter().take(2),
            'æ' => ['a', 'e'].into_iter().take(2),
            c => [c, c].into_iter().take(1),
        })
        .filter_map(|c| match c {
            '0' => Some('o'),
            '1' | '|' | '!' | 'í' | 'ı' => Some('l'),
            '5' => Some('s'),
            '’' | '\'' | '`' => None,
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .map(|c| if c == 'i' { 'l' } else { c })
        .collect()
}

impl AugmentMatcher {
    pub fn new(cfg: MatcherConfig, names: &[AugmentName]) -> Self {
        let entries = names
            .iter()
            .map(|n| Entry { id: n.id, rarity: n.rarity.clone(), key: normalize(&n.name) })
            .filter(|e| !e.key.is_empty())
            .collect();
        Self { cfg, entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How well `entry` matches `text`. With `inside`, a name long enough may also be found as a
    /// part of the reading, which tolerates extra glyphs around it at a small discount.
    fn score(&self, entry: &Entry, text: &[char], inside: bool) -> f32 {
        let full = similarity(&entry.key, text);
        if inside && entry.key.len() >= self.cfg.min_len_for_partial && text.len() > entry.key.len() {
            full.max(partial_similarity(&entry.key, text) * 0.97)
        } else {
            full
        }
    }

    /// Scores every entry against `text`, best first. `rarity_hint` favours one tier; nothing is
    /// ever excluded by rarity (see [`match_offer_in`](Self::match_offer_in)).
    ///
    /// **A name that accounts for the whole reading beats one found inside it.** Names contain
    /// other names - "Vampirisme" is in "Expertise en omnivampirisme", "Recursion" in "Infinite
    /// Recursion" - and the short one is found inside any reading of the long one at a flat 0.97,
    /// which a single misread letter in the long name falls below. So names are looked for inside
    /// a reading only when no name explains all of it.
    fn ranked(&self, text: &[char], rarity_hint: Option<&str>) -> Vec<(usize, f32)> {
        let explained = self.entries.iter().any(|e| similarity(&e.key, text) >= self.cfg.min_score);
        let mut scored: Vec<(usize, f32)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let bonus = match rarity_hint {
                    Some(r) if r == e.rarity => self.cfg.rarity_bonus,
                    _ => 0.0,
                };
                (i, self.score(e, text, !explained) + bonus)
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        scored
    }

    fn accept(&self, ranked: &[(usize, f32)], exclude: &[AugmentId]) -> Option<NameMatch> {
        let mut it = ranked.iter().filter(|(i, _)| !exclude.contains(&self.entries[*i].id));
        let &(best, score) = it.next()?;
        // The runner-up is the best *different name*; aliases sharing an id do not compete.
        let best_id = self.entries[best].id;
        let runner_up = it.find(|(i, _)| self.entries[*i].id != best_id).map_or(0.0, |r| r.1);
        let exact = score >= 1.0 - f32::EPSILON;
        (score >= self.cfg.min_score && (exact || score - runner_up >= self.cfg.min_margin)).then_some(NameMatch {
            id: best_id,
            score: score.min(1.0),
            runner_up,
        })
    }

    /// Best match for a single reading.
    pub fn match_text(&self, text: &str) -> Option<NameMatch> {
        self.match_text_hinted(text, None)
    }

    /// [`match_text`](Self::match_text), nudged towards one rarity tier by a score bonus.
    ///
    /// The nudge is never a filter. Rarity read off a *card frame* used to restrict the vocabulary
    /// to that tier, which meant one misread frame made the right name unreachable - and the
    /// Prismatic frame misreads exactly when its glow peaks. Frames are no longer asked about
    /// rarity at all; the only hint that reaches here now comes from the other two cards'
    /// own matched names.
    fn match_text_hinted(&self, text: &str, rarity: Option<&str>) -> Option<NameMatch> {
        let key = normalize(text);
        if key.len() < 3 {
            return None;
        }
        self.accept(&self.ranked(&key, rarity), &[])
    }

    /// Matches all three cards together.
    ///
    /// **The names decide the tier.** An offer is one tier, and every name in the vocabulary
    /// carries its own rarity, so the cards that read cleanly say what the others must be - a far
    /// stronger signal than a hue histogram off a frame the game is busy making glow. Nothing is
    /// ever excluded by rarity; the consensus only breaks ties.
    ///
    /// That is also what lets a golden reroll through: its card is one tier above the
    /// other two, and pass one names it with no tier involved.
    ///
    /// The same augment is never offered twice, so a duplicate reading keeps the stronger card.
    pub fn match_offer(&self, texts: &[String; OFFER_SLOTS]) -> [Option<NameMatch>; OFFER_SLOTS] {
        // Pass one: plain name matching, with no rarity involved at all.
        let mut out: [Option<NameMatch>; OFFER_SLOTS] = std::array::from_fn(|i| self.match_text(&texts[i]));

        // Resolve duplicates: keep the stronger reading.
        for i in 0..OFFER_SLOTS {
            for j in (i + 1)..OFFER_SLOTS {
                if let (Some(a), Some(b)) = (out[i], out[j]) {
                    if a.id == b.id {
                        if a.score >= b.score {
                            out[j] = None
                        } else {
                            out[i] = None
                        }
                    }
                }
            }
        }

        // Pass two: retry whatever did not match, nudged towards the tier the others agree on.
        let hint = self.tier_of_matches(&out);
        for i in 0..OFFER_SLOTS {
            if out[i].is_some() {
                continue;
            }
            let key = normalize(&texts[i]);
            if key.len() < 3 {
                continue;
            }
            let taken: Vec<AugmentId> = out.iter().flatten().map(|m| m.id).collect();
            out[i] = self.accept(&self.ranked(&key, hint), &taken);
        }
        out
    }

    /// The tier the matched names agree on, when at least two of them do and none dissents.
    fn tier_of_matches(&self, matches: &[Option<NameMatch>; OFFER_SLOTS]) -> Option<&str> {
        let tiers: Vec<&str> = matches.iter().flatten().filter_map(|m| self.rarity_of(m.id)).collect();
        tiers.first().filter(|r| tiers.len() >= 2 && tiers.iter().all(|x| x == *r)).copied()
    }

    fn rarity_of(&self, id: AugmentId) -> Option<&str> {
        self.entries.iter().find(|e| e.id == id).map(|e| e.rarity.as_str())
    }
}

/// 1 - Levenshtein distance / longer length.
fn similarity(a: &[char], b: &[char]) -> f32 {
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 1.0;
    }
    1.0 - levenshtein(a, b) as f32 / longest as f32
}

/// How well `needle` matches *some substring* of `hay`: semi-global edit distance, where skipping
/// characters at either end of `hay` is free.
fn partial_similarity(needle: &[char], hay: &[char]) -> f32 {
    if needle.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = vec![0; hay.len() + 1];
    let mut cur = vec![0; hay.len() + 1];
    for (i, &n) in needle.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &h) in hay.iter().enumerate() {
            let sub = prev[j] + usize::from(n != h);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let best = *prev.iter().min().unwrap_or(&needle.len());
    1.0 - best as f32 / needle.len() as f32
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> AugmentMatcher {
        let n = |id, name: &str, rarity: &str| AugmentName { id, name: name.into(), rarity: rarity.into() };
        AugmentMatcher::new(
            MatcherConfig::default(),
            &[
                n(1205, "ADAPt", "kSilver"),
                n(1141, "All For You", "kGold"),
                n(1004, "Back To Basics", "kPrismatic"),
                n(2103, "From Downtown", "kGold"),
                n(1134, "Draw Your Sword", "kPrismatic"),
                n(1204, "Stackosaurus Rex", "kSilver"),
                n(1238, "Transmute: Prismatic", "kGold"),
                n(1239, "Transmute: Gold", "kSilver"),
                n(1005, "Wee Woo Wee Woo", "kGold"),
                n(2031, "DropBear", "kPrismatic"),
                n(3001, "Épée de l'aube", "kGold"),
            ],
        )
    }

    #[test]
    fn exact_and_noisy_readings() {
        let m = vocab();
        assert_eq!(m.match_text("Back To Basics").unwrap().id, 1004);
        assert_eq!(m.match_text("BACK T0 BAS1CS").unwrap().id, 1004);
        assert_eq!(m.match_text("Drawyour Sword").unwrap().id, 1134);
        assert_eq!(m.match_text("« Stackosaurus Rex »").unwrap().id, 1204);
        assert_eq!(m.match_text("Epee de laube").unwrap().id, 3001);
        assert_eq!(m.match_text("From Dowmtown").unwrap().id, 2103);
    }

    /// Real `fr_fr` names from CommunityDragon, read the way an OCR without the accent would.
    #[test]
    fn french_names_match_without_their_accents() {
        let n = |id, name: &str| AugmentName { id, name: name.into(), rarity: "kGold".into() };
        let m = AugmentMatcher::new(
            MatcherConfig::default(),
            &[
                n(1, "Un cœur d'acier"),
                n(2, "Âme infernale"),
                n(3, "Ça boomerang\u{a0}?"),
                n(4, "Coûte que coûte"),
                n(5, "Aïe, mes pièces\u{a0}!"),
                n(6, "Maîtrise du A"),
                n(7, "Maîtrise du E"),
                n(8, "Quête de hâte"),
            ],
        );
        assert_eq!(m.match_text("Un coeur d'acier").unwrap().id, 1);
        assert_eq!(m.match_text("Un cœur d'acier").unwrap().id, 1);
        assert_eq!(m.match_text("Ame infernale").unwrap().id, 2);
        assert_eq!(m.match_text("Ca boomerang ?").unwrap().id, 3);
        assert_eq!(m.match_text("Coute que coute").unwrap().id, 4);
        assert_eq!(m.match_text("Aie, mes pieces !").unwrap().id, 5);
        assert_eq!(m.match_text("Maitrise du E").unwrap().id, 7);
        assert_eq!(m.match_text("Quete de hate").unwrap().id, 8);
    }

    #[test]
    fn extra_text_around_long_names_is_tolerated() {
        let m = vocab();
        assert_eq!(m.match_text("PRISMATIC Draw Your Sword").unwrap().id, 1134);
    }

    /// Real `fr_fr` and `en_us` pool names that contain another pool name. A reading of the long
    /// one with a letter wrong must not lose to the short one found whole inside it - which is
    /// what the bundled OCR's "Expert is en omnivampirisme" did.
    #[test]
    fn a_name_inside_a_longer_name_does_not_steal_its_reading() {
        let n = |id, name: &str| AugmentName { id, name: name.into(), rarity: "kSilver".into() };
        let m = AugmentMatcher::new(
            MatcherConfig::default(),
            &[
                n(1, "Vampirisme"),
                n(2, "Expertise en omnivampirisme"),
                n(3, "Recursion"),
                n(4, "Infinite Recursion"),
                n(5, "Stats on Stats!"),
                n(6, "Stats on Stats on Stats!"),
            ],
        );
        assert_eq!(m.match_text("Expert is en omnivampirisme").unwrap().id, 2);
        assert_eq!(m.match_text("Expertis en omnivampirisme").unwrap().id, 2);
        assert_eq!(m.match_text("lnfinite Recursiom").unwrap().id, 4);
        assert_eq!(m.match_text("Stats on Stats on Stat5").unwrap().id, 6);
        // The short names are still themselves, exact or noisy, and with glyphs around them.
        assert_eq!(m.match_text("Vampirisme").unwrap().id, 1);
        assert_eq!(m.match_text("Vampirlsne").unwrap().id, 1);
        assert_eq!(m.match_text("Recursion").unwrap().id, 3);
        assert_eq!(m.match_text("Stats on Stats!").unwrap().id, 5);
        assert_eq!(m.match_text("« Vampirisme » ///").unwrap().id, 1);
    }

    #[test]
    fn ambiguous_or_garbage_is_rejected() {
        let m = vocab();
        assert!(m.match_text("").is_none());
        assert!(m.match_text("zz").is_none());
        assert!(m.match_text("Level up to continue").is_none());
        // Short names must match the whole reading.
        assert!(m.match_text("ADAPt your playstyle quickly").is_none());
        // Between two near-identical names, a reading equidistant to both is refused.
        assert!(m.match_text("Transmute:").is_none());
        assert_eq!(m.match_text("Transmute: Gold").unwrap().id, 1239);
    }

    #[test]
    fn offer_matching_dedupes_and_uses_rarity() {
        let m = vocab();
        let texts = ["All For You".to_string(), "All For Yuo".to_string(), "Wee Woo Wee Woo".to_string()];
        let out = m.match_offer(&texts);
        assert_eq!(out[0].unwrap().id, 1141);
        assert!(out[1].is_none(), "the same augment cannot be offered twice");
        assert_eq!(out[2].unwrap().id, 1005);
    }

    /// The regression this design is for. A Prismatic frame whose glow peaked used to be read as
    /// Silver, and that reading *restricted* the vocabulary to Silver, so the Prismatic names
    /// actually on screen could only get through at a near-perfect similarity. Rarity now comes
    /// from the matched names and no frame colour can veto anything, so this reads cleanly.
    #[test]
    fn names_alone_resolve_an_offer_whatever_the_frames_look_like() {
        let m = vocab();
        let texts = ["Bak To Basix".to_string(), "Draw Youur Sword".to_string(), "Dropbaer".to_string()];
        let out = m.match_offer(&texts);
        assert!(out.iter().all(|s| s.is_some()), "{out:?}");
    }

    /// A golden reroll puts a card one tier above the other two. It is named like any
    /// other, noisy reading included: the two Gold cards agreeing on a tier vetoes nothing.
    #[test]
    fn a_card_one_tier_above_the_others_is_still_named() {
        let m = vocab();
        let texts = ["All For You".to_string(), "Draw Youur Sword".to_string(), "Wee Woo Wee Woo".to_string()];
        let out = m.match_offer(&texts);
        assert_eq!(out.map(|s| s.map(|m| m.id)), [Some(1141), Some(1134), Some(1005)]);
    }

    /// The tier consensus is a nudge from the *other cards' names*, and it only breaks ties: it
    /// cannot conjure a match that the score threshold would refuse.
    #[test]
    fn a_tier_consensus_cannot_invent_a_match() {
        let m = vocab();
        let texts = ["zz".to_string(), "qqqq".to_string(), "xxxx".to_string()];
        assert_eq!(m.match_offer(&texts), [None, None, None]);
    }

    #[test]
    fn similarity_helpers() {
        let c = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(levenshtein(&c("kitten"), &c("sitting")), 3);
        assert_eq!(partial_similarity(&c("sword"), &c("drawyoursword")), 1.0);
        assert!((similarity(&c("abcd"), &c("abcx")) - 0.75).abs() < 1e-6);
    }
}
