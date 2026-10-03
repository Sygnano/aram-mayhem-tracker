//! Stat anvils: the shard catalogue the vision worker matches card names against, and the
//! rankings authored in the service's `/admin` editor.
//!
//! Three documents, with three different rhythms:
//!
//! - **The catalogue** is fixed for a patch and a language, so it is fetched once for the game's
//!   locale and kept. It is what builds the shard matcher.
//! - **The rankings** are edited by hand in the service's `/admin` editor. They come whole with the
//!   downloaded dataset (`super::dataset`, D-090), so an edit reaches the app at its next update:
//!   within 24 hours, or at once with *Update now*.
//! - **The enemy team's damage split** can only be asked for in game, because an ARAM champ select
//!   never shows the other team. It is fetched once, when the Live Client first lists them, and it
//!   breaks a tie between Armor and Magic Resist.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aramkit_client::{AnvilCatalogue, AnvilRankings, AramkitClient, DamageSplit, Freshness};
use mayhem_core::anvils::{AnvilTier, ShardInfo};
use mayhem_vision::anvil::ShardMatcher;

use super::{Engine, EngineState};

const TICK: Duration = Duration::from_secs(1);
/// The catalogue only changes with a patch; the service itself polls every six hours.
const CATALOGUE_REFRESH: Duration = Duration::from_secs(6 * 60 * 60);
/// How soon to try again after a failure, while we have nothing.
const RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct AnvilData {
    /// The service locale the catalogue is in (`en_us`, `fr_fr`).
    pub locale: Option<String>,
    pub patch: String,
    pub catalogue: Arc<Vec<ShardInfo>>,
    pub matcher: Option<Arc<ShardMatcher>>,
    /// Written by the dataset (`dataset.rs`), which holds them whole.
    pub rankings: Option<AnvilRankings>,
    pub freshness: Option<Freshness>,
    /// Why the catalogue is missing or old. Kept apart from the rankings' error: they are fetched
    /// on different rhythms, and one succeeding says nothing about the other.
    pub catalogue_error: Option<String>,
    pub rankings_error: Option<String>,
    /// How the enemy team deals its damage. `None` outside a game and until the answer arrives,
    /// and then an Armor / Magic Resist tie is simply left as a tie.
    pub enemy_damage: Option<EnemyDamage>,
}

impl AnvilData {
    /// Everything currently wrong with the anvil data, for the settings screen.
    pub fn error(&self) -> Option<String> {
        let problems: Vec<&str> =
            [&self.catalogue_error, &self.rankings_error].into_iter().flatten().map(String::as_str).collect();
        (!problems.is_empty()).then(|| problems.join(" · "))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnemyDamage {
    /// The enemy champions this was asked about, sorted. A different list means a different game.
    pub champions: Vec<i64>,
    pub split: DamageSplit,
}

/// The enemy team's champion ids in a Mayhem game, sorted. Empty when there is no game, or the
/// Live Client has not said who we are yet.
fn enemy_champions(st: &EngineState) -> Vec<i64> {
    let Some(game) = st.game.as_ref().filter(|g| g.is_mayhem) else { return Vec::new() };
    let (Some(me), Some(data)) = (game.data.me(), st.static_data.as_ref()) else { return Vec::new() };
    let mut ids: Vec<i64> = game
        .data
        .all_players
        .iter()
        .filter(|p| p.team != me.team)
        .filter_map(|p| data.champion_id(p.champion_key()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the anvil data", run(engine.clone()));
}

/// The static data's locale folder, as the service names locales: `default` is English.
fn service_locale(static_locale: &str) -> String {
    match static_locale {
        "" | "default" => "en_us".to_owned(),
        other => other.to_ascii_lowercase(),
    }
}

/// The service's shards as the vision crate wants them. A tier the app does not know is skipped,
/// so a future service change degrades one shard rather than the whole list.
pub fn shards_from(catalogue: &AnvilCatalogue) -> Vec<ShardInfo> {
    catalogue
        .shards
        .iter()
        .filter_map(|s| {
            let tier = match s.tier.as_str() {
                "silver" => AnvilTier::Silver,
                "gold" => AnvilTier::Gold,
                "prismatic" => AnvilTier::Prismatic,
                _ => return None,
            };
            Some(ShardInfo {
                id: s.id.clone(),
                tier,
                kind: s.kind.clone(),
                name: s.name.clone(),
                values: s.values.clone(),
            })
        })
        .collect()
}

async fn run(engine: Arc<Engine>) {
    let (base, cache_dir) = {
        let st = engine.lock();
        (st.tuning.service_base.clone(), engine.cache_dir.join("aramkit"))
    };
    let client = AramkitClient::new(base, cache_dir);
    // Once per start: earlier builds stored an answer per enemy team and per offer, and never
    // removed one.
    let removed = client.remove_obsolete_copies().await;
    if removed > 0 {
        log::info!("removed {removed} stored responses that are no longer used");
    }
    let mut catalogue_at: Option<Instant> = None;
    // The enemy team a damage fetch last failed for, and when: retried, but only reported once.
    let mut damage_failed: Option<(Vec<i64>, Instant)> = None;

    loop {
        tokio::time::sleep(TICK).await;

        let (wanted_locale, have_locale, matcher_cfg, enemies, have_enemies) = {
            let st = engine.lock();
            (
                st.static_data.as_ref().map(|d| service_locale(&d.locale)),
                st.anvils.locale.clone(),
                st.tuning.matcher,
                enemy_champions(&st),
                st.anvils.enemy_damage.as_ref().map(|d| d.champions.clone()),
            )
        };

        // The catalogue, once the game's language is known.
        if let Some(locale) = wanted_locale {
            let due = have_locale.as_deref() != Some(locale.as_str())
                || catalogue_at.is_none_or(|t| t.elapsed() >= CATALOGUE_REFRESH);
            let backing_off =
                have_locale.as_deref() != Some(locale.as_str()) && catalogue_at.is_some_and(|t| t.elapsed() < RETRY);
            if due && !backing_off {
                catalogue_at = Some(Instant::now());
                match client.anvils(&locale).await {
                    Ok(fetched) => {
                        let shards = shards_from(&fetched.value);
                        let matcher = ShardMatcher::new(matcher_cfg, &shards);
                        let mut st = engine.lock();
                        st.log_event(format!(
                            "anvil shards ready: {} in {locale} for {}",
                            shards.len(),
                            fetched.value.patch
                        ));
                        st.anvils.locale = Some(locale);
                        st.anvils.patch = fetched.value.patch;
                        st.anvils.catalogue = Arc::new(shards);
                        st.anvils.matcher = Some(Arc::new(matcher));
                        st.anvils.freshness = Some(fetched.freshness);
                        st.anvils.catalogue_error = None;
                    }
                    Err(e) => {
                        let mut st = engine.lock();
                        st.anvils.catalogue_error = Some(format!("anvil shards: {e}"));
                        st.log_event(format!("could not fetch anvil shards: {e}"));
                    }
                }
            }
        }

        // The enemy team's damage split, once per game. The one request made in game (everything
        // else stays off the network there): who the enemies are is not known any earlier.
        if enemies.is_empty() {
            if have_enemies.is_some() {
                engine.lock().anvils.enemy_damage = None;
            }
        } else if have_enemies.as_ref() != Some(&enemies)
            && damage_failed.as_ref().is_none_or(|(_, at)| at.elapsed() >= RETRY)
        {
            let result = match client.damage(&enemies).await {
                Ok(response) => response.team.ok_or_else(|| "the service has no data for any of them".to_owned()),
                Err(e) => Err(e.to_string()),
            };
            let mut st = engine.lock();
            match result {
                Ok(split) => {
                    damage_failed = None;
                    st.log_event(format!(
                        "enemy damage: {:.0}% physical, {:.0}% magic, {:.0}% true over {} champions",
                        split.physical * 100.0,
                        split.magic * 100.0,
                        split.true_damage * 100.0,
                        enemies.len()
                    ));
                    st.anvils.enemy_damage = Some(EnemyDamage { champions: enemies, split });
                }
                Err(e) => {
                    // A split left over from another game must not be applied to this one.
                    st.anvils.enemy_damage = None;
                    if damage_failed.as_ref().map(|(ids, _)| ids) != Some(&enemies) {
                        st.log_event(format!("could not fetch the enemy team's damage: {e}"));
                    }
                    damage_failed = Some((enemies, Instant::now()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aramkit_client::AnvilShard;

    #[test]
    fn one_fetch_succeeding_does_not_clear_the_others_error() {
        let mut data = AnvilData::default();
        assert_eq!(data.error(), None);
        data.catalogue_error = Some("anvil shards: unreachable".into());
        data.rankings_error = Some("anvil rankings: unreachable".into());
        // The rankings come back; the catalogue is still missing and must still say so.
        data.rankings_error = None;
        assert_eq!(data.error().as_deref(), Some("anvil shards: unreachable"));
        data.rankings_error = Some("anvil rankings: 503".into());
        assert_eq!(data.error().as_deref(), Some("anvil shards: unreachable · anvil rankings: 503"));
    }

    #[test]
    fn english_is_the_default_locale_folder() {
        assert_eq!(service_locale("default"), "en_us");
        assert_eq!(service_locale("fr_fr"), "fr_fr");
    }

    #[test]
    fn unknown_tiers_are_skipped_not_fatal() {
        let shard = |id: &str, tier: &str| AnvilShard {
            id: id.into(),
            tier: tier.into(),
            values: vec![1.0],
            ..Default::default()
        };
        let catalogue = AnvilCatalogue { shards: vec![shard("a", "gold"), shard("b", "mythic")], ..Default::default() };
        let shards = shards_from(&catalogue);
        assert_eq!(shards.len(), 1);
        assert_eq!(shards[0].tier, AnvilTier::Gold);
    }
}
