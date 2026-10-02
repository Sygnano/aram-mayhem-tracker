//! The service's wire types, mirrored. Lenient throughout: `#[serde(default)]` everywhere, so a
//! field the service stops sending leaves a row degraded rather than failing the whole response.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionInfo {
    pub id: i64,
    pub win_rate: f64,
    pub pick_rate: f64,
    pub sample_count: i64,
    pub tier: Option<String>,
}

/// One augment, with the numbers the overlay shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AugmentInfo {
    pub id: i64,
    pub name_id: Option<String>,
    pub name: Option<String>,
    pub rarity: Option<String>,
    pub icon_url: Option<String>,

    pub win_rate: Option<f64>,
    /// Percentage points against the baseline named by `delta_basis`. This is what drives the
    /// ranking.
    pub delta_pp: Option<f64>,
    pub pick_rate: Option<f64>,
    pub augment_win_rate: Option<f64>,
    pub sample_count: i64,
    pub tier: Option<String>,
    pub rank: Option<i64>,
    /// `stage` / `champion` / `global` / `none`.
    pub source: String,
    /// `high` / `low` / `fallback` / `none`.
    pub confidence: String,
    /// Fewer than 200 games behind the number: show it, but warn.
    pub low_sample: bool,
    /// `champion` when the delta is against this champion's own win rate, `global` when it is
    /// against the 50% population average, which is not comparable.
    pub delta_basis: String,
    pub by_stage: Vec<StageInfo>,
    /// The stages this augment can be offered at (an augment that starts at level 11 lists [3, 4]).
    /// Empty when upstream does not say, which the UI treats as "available everywhere".
    pub available_stages: Vec<i64>,
}

impl AugmentInfo {
    /// True when the numbers describe this champion specifically.
    pub fn is_champion_specific(&self) -> bool {
        self.source == "stage" || self.source == "champion"
    }

    pub fn has_data(&self) -> bool {
        self.source != "none" && self.win_rate.is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StageInfo {
    /// aramkit's stage number, 1-4.
    pub stage: u8,
    /// The in-game level: 1, 7, 11, 15.
    pub level: u8,
    pub win_rate: f64,
    pub delta_pp: f64,
    pub pick_rate: f64,
    pub sample_count: i64,
    pub low_sample: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PoolResponse {
    pub patch: String,
    pub data_date: String,
    pub champion: ChampionInfo,
    pub rarity: String,
    pub stage: Option<u8>,
    pub augments: Vec<AugmentInfo>,
}

/// Everything about one champion, in one response (`/v1/champion`): the twelve ranked pools,
/// the build archetypes, and the anvil rankings reduced to the champion's group.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionBundle {
    pub patch: String,
    pub data_date: String,
    pub champion: ChampionInfo,
    /// One per rarity and stage.
    pub pools: Vec<BundlePool>,
    pub archetypes: Vec<ArchetypeBuild>,
    /// One group, or none when the champion is in no group.
    pub anvil_rankings: AnvilRankings,
}

/// One rarity's pool at one stage, as `/v1/pool` would answer it without its envelope.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BundlePool {
    pub rarity: String,
    pub stage: u8,
    pub augments: Vec<AugmentInfo>,
}

impl ChampionBundle {
    /// The pools in the shape the rest of the app reads, each with the bundle's envelope.
    pub fn pool_responses(&self) -> impl Iterator<Item = PoolResponse> + '_ {
        self.pools.iter().map(|p| PoolResponse {
            patch: self.patch.clone(),
            data_date: self.data_date.clone(),
            champion: self.champion.clone(),
            rarity: p.rarity.clone(),
            stage: Some(p.stage),
            augments: p.augments.clone(),
        })
    }

    /// The build, in the shape the item-set writer reads.
    pub fn build(&self) -> BuildResponse {
        BuildResponse {
            patch: self.patch.clone(),
            data_date: self.data_date.clone(),
            champion_id: self.champion.id,
            archetypes: self.archetypes.clone(),
        }
    }
}

/// One champion's standing in ARAM: Mayhem — what a champ-select block shows.
///
/// Distinct from [`ChampionInfo`] because of `rank`, which only means anything against the
/// `pool_size` on [`ChampionsResponse`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionRanking {
    pub id: i64,
    /// aramkit's own rank, 1 is best.
    pub rank: i64,
    pub tier: Option<String>,
    /// A fraction, as everywhere in this contract: 0.5778 is 57.78%.
    pub win_rate: f64,
    pub pick_rate: f64,
    pub sample_count: i64,
}

/// Every champion aramkit ranks. Fetched once per patch and looked up locally, because champ select
/// asks about up to eighteen champions and the bench changes as it is rerolled.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionsResponse {
    pub patch: String,
    pub data_date: String,
    /// What a `rank` is out of. A rank without it is not interpretable, so the two travel together.
    pub pool_size: i64,
    /// Best rank first.
    pub champions: Vec<ChampionRanking>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildResponse {
    pub patch: String,
    pub data_date: String,
    pub champion_id: i64,
    pub archetypes: Vec<ArchetypeBuild>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArchetypeBuild {
    /// aramkit's key: `crit`, `bruiser`, `ad`, `on_hit`, `tank`...
    pub key: String,
    pub rank: i64,
    pub win_rate: f64,
    pub pick_rate: f64,
    pub sample_count: i64,
    pub starters: Vec<StarterSet>,
    pub boots: Vec<ItemStat>,
    /// Core items in the order they are bought.
    pub core: Vec<i64>,
    pub situational: Vec<ItemStat>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StarterSet {
    pub items: Vec<i64>,
    pub win_rate: f64,
    pub pick_rate: f64,
    pub sample_count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemStat {
    pub id: i64,
    pub win_rate: f64,
    pub pick_rate: f64,
    pub sample_count: i64,
}

/// A damage split: fractions that sum to 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DamageSplit {
    pub physical: f64,
    pub magic: f64,
    pub true_damage: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionDamage {
    pub id: i64,
    #[serde(flatten)]
    pub split: DamageSplit,
    /// Average damage to champions per game: what `team` weights each champion by.
    pub damage_per_game: f64,
    pub sample_count: i64,
}

/// How a set of champions deal their damage, from `/v1/damage`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DamageResponse {
    pub patch: String,
    pub data_date: String,
    pub champions: Vec<ChampionDamage>,
    /// Ids the service has no data for.
    pub missing: Vec<i64>,
    /// The listed champions together. `None` when none of them was found.
    pub team: Option<DamageSplit>,
}

/// One stat anvil shard in one tier, from `/v1/anvils`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnvilShard {
    /// `ARAM_GoldStatAnvil_MR`.
    pub id: String,
    /// `silver`, `gold` or `prismatic`.
    pub tier: String,
    /// The stat, shared by the same shard across tiers.
    pub kind: String,
    pub name: String,
    /// The numbers the card prints, percentages already ×100.
    pub values: Vec<f64>,
    pub icon_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnvilCatalogue {
    pub patch: String,
    pub locale: String,
    pub shards: Vec<AnvilShard>,
}

/// The one saved anvil rankings document, authored in the service's `/admin` editor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnvilRankings {
    /// `None` before the first save.
    pub saved_at: Option<i64>,
    pub version: u32,
    pub groups: Vec<AnvilGroup>,
}

impl AnvilRankings {
    /// The group a champion belongs to. At most one, and none is a normal answer: there is no
    /// fallback group.
    pub fn group_of(&self, champion_id: i64) -> Option<&AnvilGroup> {
        self.groups.iter().find(|g| g.champions.contains(&champion_id))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnvilGroup {
    pub name: String,
    pub champions: Vec<i64>,
    pub tiers: AnvilTiers,
}

/// Each tier is an ordered list of tie buckets; bucket `i` is rank `i + 1`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnvilTiers {
    pub silver: Vec<Vec<String>>,
    pub gold: Vec<Vec<String>>,
    pub prismatic: Vec<Vec<String>>,
}

impl AnvilTiers {
    pub fn get(&self, tier: &str) -> &[Vec<String>] {
        match tier {
            "silver" => &self.silver,
            "gold" => &self.gold,
            "prismatic" => &self.prismatic,
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundle as the service sends it, cut to one pool, and the shapes the app reads it in.
    #[test]
    fn a_champion_bundle_parses_and_splits_into_pools_and_a_build() {
        let body = r#"{"patch":"16.19","dataDate":"2026-09-29",
            "champion":{"id":157,"winRate":0.5773,"pickRate":0.1,"sampleCount":9,"tier":"S"},
            "pools":[{"rarity":"gold","stage":2,"augments":[{"id":1058,"name":"Mystic Punch","deltaPp":8.9,
                "source":"champion","byStage":[],"availableStages":[1,2,3,4]}]}],
            "archetypes":[{"key":"crit","rank":1,"core":[3031]}],
            "anvilRankings":{"savedAt":12,"version":1,"groups":[{"name":"Swordsmen","champions":[157],
                "tiers":{"silver":[["ARAM_StatAnvil_AR"]],"gold":[],"prismatic":[]}}]}}"#;
        let bundle: ChampionBundle = serde_json::from_str(body).unwrap();

        let pools: Vec<PoolResponse> = bundle.pool_responses().collect();
        assert_eq!(pools.len(), 1);
        assert_eq!((pools[0].rarity.as_str(), pools[0].stage), ("gold", Some(2)));
        assert_eq!(pools[0].champion.id, 157, "each pool carries the bundle's envelope");
        assert_eq!(pools[0].patch, "16.19");
        assert_eq!(pools[0].augments[0].delta_pp, Some(8.9));

        let build = bundle.build();
        assert_eq!(build.champion_id, 157);
        assert_eq!(build.archetypes[0].core, [3031]);

        assert_eq!(bundle.anvil_rankings.saved_at, Some(12));
        assert_eq!(bundle.anvil_rankings.group_of(157).map(|g| g.name.as_str()), Some("Swordsmen"));
    }

    /// What the service answered for a real team on 16.19, cut to two champions.
    #[test]
    fn a_damage_response_parses_as_the_service_sends_it() {
        let body = r#"{"patch":"16.19","dataDate":"2026-09-29","champions":[
            {"id":157,"physical":0.8303,"magic":0.1201,"trueDamage":0.0496,"damagePerGame":42288.0,"sampleCount":3138960},
            {"id":99,"physical":0.0367,"magic":0.9172,"trueDamage":0.0462,"damagePerGame":32149.0,"sampleCount":1819191}],
            "missing":[999999],"team":{"physical":0.4489,"magic":0.5015,"trueDamage":0.0496}}"#;
        let r: DamageResponse = serde_json::from_str(body).unwrap();
        assert_eq!(r.champions[0].split, DamageSplit { physical: 0.8303, magic: 0.1201, true_damage: 0.0496 });
        assert_eq!(r.champions[1].sample_count, 1819191);
        assert_eq!(r.missing, [999999]);
        assert_eq!(r.team, Some(DamageSplit { physical: 0.4489, magic: 0.5015, true_damage: 0.0496 }));

        // No champion found: the team is null, and that parses as "nothing to go on".
        let none: DamageResponse = serde_json::from_str(r#"{"champions":[],"missing":[1],"team":null}"#).unwrap();
        assert_eq!(none.team, None);
    }
}
