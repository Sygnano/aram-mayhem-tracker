//! Champion identity: CommunityDragon's `champion-summary.json`, reduced to an alias → id map.
//!
//! Live Client Data reports the champion as a name; the augment statistics service is keyed by Riot
//! champion id, so something has to bridge the two. The locale-independent `champion_key()` from
//! `league-api` (`Yasuo`, `LeeSin`) matches CommunityDragon's `alias`, which makes this a lookup
//! rather than fuzzy matching.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One row of `champion-summary.json`. Only the fields we use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionSummary {
    pub id: i64,
    pub name: String,
    /// Locale-independent internal key, e.g. `Yasuo`, `LeeSin`, `MonkeyKing` (Wukong).
    pub alias: String,
}

/// Lowercased and stripped of anything that is not a letter or a digit, so `Lee Sin`, `LeeSin` and
/// `leesin` all land on the same key.
pub fn normalise_key(key: &str) -> String {
    key.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Champion id → display name, in whatever locale the summary was loaded in.
pub fn names(champions: &[ChampionSummary]) -> HashMap<i64, String> {
    champions.iter().filter(|c| c.id > 0 && !c.name.is_empty()).map(|c| (c.id, c.name.clone())).collect()
}

/// Alias → id, plus display name → id as a second chance for callers that only have a name.
///
/// The `-1` "None" champion CommunityDragon ships is skipped; it is not a champion.
pub fn index(champions: &[ChampionSummary]) -> HashMap<String, i64> {
    let mut out = HashMap::with_capacity(champions.len() * 2);
    for c in champions.iter().filter(|c| c.id > 0) {
        if !c.alias.is_empty() {
            out.insert(normalise_key(&c.alias), c.id);
        }
        // Only as a fallback: an alias must never be overwritten by someone else's display name.
        if !c.name.is_empty() {
            out.entry(normalise_key(&c.name)).or_insert(c.id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn champ(id: i64, name: &str, alias: &str) -> ChampionSummary {
        ChampionSummary { id, name: name.into(), alias: alias.into() }
    }

    #[test]
    fn aliases_and_names_both_resolve() {
        let index = index(&[
            champ(157, "Yasuo", "Yasuo"),
            champ(64, "Lee Sin", "LeeSin"),
            // Wukong's display name and internal alias disagree, which is the case that matters.
            champ(62, "Wukong", "MonkeyKing"),
            champ(-1, "None", "NONE"),
        ]);

        assert_eq!(index.get("yasuo"), Some(&157));
        assert_eq!(index.get("leesin"), Some(&64), "the live API reports LeeSin");
        assert_eq!(index.get("monkeyking"), Some(&62), "CommunityDragon's alias for Wukong");
        assert_eq!(index.get("wukong"), Some(&62), "and his display name still works");
        assert!(!index.values().any(|&id| id == -1), "the None champion is not a champion");
    }

    #[test]
    fn keys_ignore_case_and_punctuation() {
        assert_eq!(normalise_key("Lee Sin"), "leesin");
        assert_eq!(normalise_key("Cho'Gath"), "chogath");
        assert_eq!(normalise_key("Dr. Mundo"), "drmundo");
        assert_eq!(normalise_key("Kai'Sa"), "kaisa");
    }
}
