use serde::{Deserialize, Serialize};

use crate::StaticDataError;

/// A row of `cherry-augments.json`. Only the fields we use; everything else is ignored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CherryAugment {
    pub id: i64,
    pub augment_name_id: String,
    #[serde(rename = "nameTRA")]
    pub name_tra: String,
    pub augment_small_icon_path: String,
    pub rarity: String,
}

/// A row of `augment-lists.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AugmentList {
    pub mode_name: String,
    pub augment_list: Vec<String>,
}

/// One augment of a mode's pool, in our normalised shape (keyed by Riot id).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolAugment {
    pub id: i64,
    pub name_id: String,
    pub name: String,
    pub rarity: String,
    pub icon_url: String,
}

/// The pool for `mode` (e.g. `KIWI`), with the pool entries that did not resolve. Research found
/// zero unresolved entries on 16.19; a non-empty list is worth surfacing in the debug window.
pub fn build_pool(
    augments: &[CherryAugment],
    lists: &[AugmentList],
    mode: &str,
    icon_base: &str,
) -> Result<(Vec<PoolAugment>, Vec<String>), StaticDataError> {
    let list = lists.iter().find(|l| l.mode_name == mode).ok_or_else(|| StaticDataError::MissingPool(mode.into()))?;
    let mut pool = Vec::with_capacity(list.augment_list.len());
    let mut unmatched = Vec::new();
    for entry in &list.augment_list {
        let name_id = entry.rsplit('/').next().unwrap_or(entry);
        match augments.iter().find(|a| !a.augment_name_id.is_empty() && a.augment_name_id == name_id) {
            Some(a) => pool.push(PoolAugment {
                id: a.id,
                name_id: a.augment_name_id.clone(),
                name: a.name_tra.clone(),
                rarity: a.rarity.clone(),
                icon_url: icon_url(icon_base, &a.augment_small_icon_path),
            }),
            None => unmatched.push(name_id.to_owned()),
        }
    }
    Ok((pool, unmatched))
}

/// `/lol-game-data/assets/ASSETS/UX/...png` → `<icon_base>/assets/ux/...png`, lowercased.
/// `icon_base` is
/// `https://raw.communitydragon.org/<patch>/plugins/rcp-be-lol-game-data/global/default`.
pub fn icon_url(icon_base: &str, lcu_path: &str) -> String {
    let rest = lcu_path.strip_prefix("/lol-game-data/assets/").unwrap_or(lcu_path.trim_start_matches('/'));
    if rest.is_empty() {
        return String::new();
    }
    format!("{}/{}", icon_base.trim_end_matches('/'), rest.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> (Vec<CherryAugment>, Vec<AugmentList>) {
        (
            serde_json::from_str(include_str!("../tests/fixtures/cherry-augments.synthetic.json")).unwrap(),
            serde_json::from_str(include_str!("../tests/fixtures/augment-lists.synthetic.json")).unwrap(),
        )
    }

    const BASE: &str = "https://raw.communitydragon.org/16.19/plugins/rcp-be-lol-game-data/global/default";

    #[test]
    fn builds_the_kiwi_pool() {
        let (augments, lists) = fixtures();
        let (pool, unmatched) = build_pool(&augments, &lists, "KIWI", BASE).unwrap();
        assert!(unmatched.is_empty());
        let ids: Vec<i64> = pool.iter().map(|a| a.id).collect();
        assert_eq!(ids, vec![1205, 1141, 1004, 2103]);
        assert_eq!(pool[3].name, "From Downtown");
        assert_eq!(
            pool[3].icon_url,
            "https://raw.communitydragon.org/16.19/plugins/rcp-be-lol-game-data/global/default/assets/ux/kiwi/augments/icons/questbangbang_small.png"
        );
    }

    #[test]
    fn reports_unmatched_entries_and_missing_pools() {
        let (augments, lists) = fixtures();
        let (pool, unmatched) = build_pool(&augments, &lists, "KIWI_JADE", BASE).unwrap();
        assert_eq!(pool.len(), 1);
        assert_eq!(unmatched, vec!["DoesNotExist".to_string()]);
        assert!(matches!(build_pool(&augments, &lists, "NOPE", BASE), Err(StaticDataError::MissingPool(_))));
    }

    #[test]
    fn empty_icon_path_gives_empty_url() {
        assert_eq!(icon_url(BASE, ""), "");
    }
}
