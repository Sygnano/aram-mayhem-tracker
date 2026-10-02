//! LCU item sets, one per archetype.
//!
//! Two halves: [`build`] turns the service's archetypes into the JSON the LCU stores, which is pure
//! and tested here; and [`run`] watches champ select and writes them on lock-in, which is not,
//! because it needs a running client.
//!
//! **This replaces every item set on the account.** `PUT /lol-item-sets/v1/item-sets/{id}/sets`
//! takes the whole collection, so there is no way to add ours without rewriting the rest. That is
//! the behaviour you asked for; the *Manage item sets* setting is what guards it. For the same
//! reason the client's own recommended pages (`Config/Global/Recommended` in the install folder) are
//! deleted at the same time, so only ours show in the shop.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aramkit_client::ArchetypeBuild;
use league_api::lcu::LcuClient;
use mayhem_core::queues;
use serde_json::{json, Value};

use super::Engine;

/// Champ select is short; this is the lag between locking in and the sets appearing.
const TICK: Duration = Duration::from_millis(600);

/// The ARAM map. Mayhem runs on the Howling Abyss, so the sets are tagged for it and stay out of
/// the way on Summoner's Rift.
const ARAM_MAP: i64 = 12;

#[derive(Debug, Clone, Default)]
pub struct ItemSetState {
    /// The champion whose sets are currently written, so locking the same champion twice is a
    /// no-op rather than a second write.
    pub written_for: Option<i64>,
    pub error: Option<String>,
}

/// The mode's name as the client shows it, in the client's language.
fn mode_name(locale: Option<&str>) -> &'static str {
    match locale {
        Some(l) if l.to_ascii_lowercase().starts_with("fr") => "ARAM du chaos",
        _ => "ARAM Mayhem",
    }
}

/// The archetype's name, in the client's language. These are every key aramkit used across all 173
/// champions on 16.19; one it adds later falls back to its key, `snake_case` turned into words.
fn build_type(key: &str, locale: Option<&str>) -> String {
    let fr = locale.is_some_and(|l| l.to_ascii_lowercase().starts_with("fr"));
    let known = match (key, fr) {
        ("ap", _) => "AP",
        ("ad", _) => "AD",
        ("tank", _) => "Tank",
        ("bruiser", false) => "Bruiser",
        ("bruiser", true) => "Combattant",
        ("crit", false) => "Crit",
        ("crit", true) => "Critique",
        ("lethality", false) => "Lethality",
        ("lethality", true) => "Létalité",
        ("on_hit", false) => "On-Hit",
        ("on_hit", true) => "À l'impact",
        ("utility", false) => "Utility",
        ("utility", true) => "Support",
        ("hybrid", false) => "Hybrid",
        ("hybrid", true) => "Hybride",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_owned();
    }
    key.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `Crit Ezreal - 60.1% - ARAM Mayhem`.
fn title(key: &str, champion: &str, locale: Option<&str>, win_rate: f64) -> String {
    format!("{} {champion} - {:.1}% - {}", build_type(key, locale), win_rate * 100.0, mode_name(locale))
}

/// Turns the archetypes into the `itemSets` array the LCU stores.
///
/// Block order is the order they appear in game: what you open with, boots, the core in the order
/// it is actually bought, then situational.
pub fn build(
    champion_id: i64,
    champion_name: &str,
    locale: Option<&str>,
    archetypes: &[ArchetypeBuild],
    patch: &str,
) -> Value {
    let sets: Vec<Value> = archetypes
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mut blocks = Vec::new();

            // Each starter option is a set of items bought together, so they share one block.
            let starters: Vec<i64> = a.starters.iter().flat_map(|s| s.items.iter().copied()).collect();
            if !starters.is_empty() {
                blocks.push(block("Starters", &dedupe(&starters)));
            }
            let boots: Vec<i64> = a.boots.iter().map(|b| b.id).collect();
            if !boots.is_empty() {
                blocks.push(block("Boots", &boots));
            }
            if !a.core.is_empty() {
                blocks.push(block("Core (in order)", &a.core));
            }
            let situational: Vec<i64> = a.situational.iter().map(|s| s.id).collect();
            if !situational.is_empty() {
                blocks.push(block("Situational", &situational));
            }

            json!({
                "title": title(&a.key, champion_name, locale, a.win_rate),
                "type": "custom",
                "map": "any",
                "mode": "any",
                "priority": false,
                "sortrank": i as i64 + 1,
                "startedFrom": "blank",
                "associatedChampions": [champion_id],
                "associatedMaps": [ARAM_MAP],
                "uid": format!("mayhem-{champion_id}-{}-{patch}", a.key),
                "blocks": blocks,
            })
        })
        .collect();

    Value::Array(sets)
}

fn block(name: &str, items: &[i64]) -> Value {
    json!({
        "type": name,
        "showIfSummonerSpell": "",
        "hideIfSummonerSpell": "",
        // The LCU wants item ids as strings here, not numbers.
        "items": items.iter().map(|id| json!({ "id": id.to_string(), "count": 1 })).collect::<Vec<_>>(),
    })
}

/// Keeps the first occurrence of each id. Starter options overlap (boots turn up in several), and a
/// repeated item in one block just wastes a slot.
fn dedupe(ids: &[i64]) -> Vec<i64> {
    let mut seen = std::collections::HashSet::new();
    ids.iter().copied().filter(|id| seen.insert(*id)).collect()
}

pub fn spawn(engine: Arc<Engine>) {
    super::spawn_task(&engine, "the item-set writer", run(engine.clone()));
}

async fn run(engine: Arc<Engine>) {
    loop {
        tokio::time::sleep(TICK).await;

        // Everything that reads state happens here, before any await: the state is behind a plain
        // `std::sync::Mutex`. The build itself was fetched at lock-in by `champion.rs`, so there is
        // nothing to download at this point.
        let Some(job) = ({
            let st = engine.lock();
            pending_job(&st)
        }) else {
            continue;
        };

        if job.archetypes.is_empty() {
            let mut st = engine.lock();
            st.item_sets.written_for = Some(job.champion_id);
            st.log_event(format!("item sets skipped: no archetypes for champion {}", job.champion_id));
            continue;
        }

        if let Some(dir) = &job.recommended_dir {
            match remove_recommended(dir).await {
                Ok(true) => engine.lock().log_event(format!("deleted {}", dir.display())),
                Ok(false) => {}
                Err(e) => engine.lock().log_event(format!("could not delete {}: {e}", dir.display())),
            }
        }

        let sets = build(job.champion_id, &job.champion_name, job.locale.as_deref(), &job.archetypes, &job.patch);
        let count = job.archetypes.len();
        let lcu = LcuClient::new(&job.credentials);

        let result = match lcu.current_summoner_id().await {
            Ok(id) => lcu.replace_item_sets(id, sets).await.map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };

        let mut st = engine.lock();
        // Set on failure too, on purpose: **a failed write is not retried** for this champion. The
        // write replaces every item set on the account, and a client that refused it once will
        // refuse it again 600 ms later; retrying would hammer the endpoint and fill the log for the
        // rest of champ select. The error is shown instead, and the next lock-in (a swap, or the
        // next game) tries again, because leaving champ select clears `written_for`.
        st.item_sets.written_for = Some(job.champion_id);
        match result {
            Ok(()) => {
                st.item_sets.error = None;
                st.log_event(format!(
                    "wrote {count} item sets for champion {} (replacing all others)",
                    job.champion_id
                ));
            }
            Err(e) => {
                st.item_sets.error = Some(e.clone());
                st.log_event(format!("could not write item sets: {e}"));
            }
        }
    }
}

/// Deletes the client's recommended item pages. `Ok(false)` when there were none.
async fn remove_recommended(dir: &Path) -> std::io::Result<bool> {
    match tokio::fs::remove_dir_all(dir).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

struct Job {
    champion_id: i64,
    champion_name: String,
    locale: Option<String>,
    recommended_dir: Option<PathBuf>,
    patch: String,
    archetypes: Vec<ArchetypeBuild>,
    credentials: league_api::lockfile::Credentials,
}

/// The write that is due, if one is.
///
/// Guarded five ways: the setting is on, the LCU is reachable, the queue is Mayhem, we have
/// not already written for this champion, and the build for it has arrived.
fn pending_job(st: &super::EngineState) -> Option<Job> {
    if !st.settings.manage_item_sets {
        return None;
    }
    let credentials = st.client.credentials.clone()?;
    let mayhem = st.client.queue.as_ref().is_some_and(|q| queues::is_mayhem_queue(q.id, Some(&q.description)));
    if !mayhem {
        return None;
    }
    let champion_id = st.client.locked_champion?;
    if st.item_sets.written_for == Some(champion_id) {
        return None;
    }
    // Wait for the champ select prefetch rather than starting a second download of the same thing.
    let archetypes = st.champion.build_for(champion_id)?.archetypes.clone();
    let patch = st.static_data.as_ref().map(|d| d.patch.clone()).unwrap_or_default();
    let champion_name = st
        .static_data
        .as_ref()
        .and_then(|d| d.champion_names.get(&champion_id).cloned())
        .unwrap_or_else(|| champion_id.to_string());
    let recommended_dir = st.client.install_dir.as_ref().map(|d| d.join("Config").join("Global").join("Recommended"));
    Some(Job {
        champion_id,
        champion_name,
        locale: st.client.locale.clone(),
        recommended_dir,
        patch,
        archetypes,
        credentials,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aramkit_client::{ItemStat, StarterSet};

    fn item(id: i64) -> ItemStat {
        ItemStat { id, win_rate: 0.6, pick_rate: 0.3, sample_count: 1000 }
    }

    fn archetype(key: &str) -> ArchetypeBuild {
        ArchetypeBuild {
            key: key.into(),
            rank: 1,
            win_rate: 0.6012,
            pick_rate: 0.6,
            sample_count: 1000,
            starters: vec![
                StarterSet { items: vec![1038], win_rate: 0.66, pick_rate: 0.5, sample_count: 100 },
                // 3006 also appears under boots; within the starters block it must not repeat.
                StarterSet { items: vec![1042, 3006], win_rate: 0.66, pick_rate: 0.18, sample_count: 90 },
            ],
            boots: vec![item(3006), item(3111)],
            core: vec![3032, 123_430, 6333],
            situational: vec![item(3065), item(3072)],
        }
    }

    fn blocks(set: &Value) -> Vec<(String, Vec<String>)> {
        set["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                (
                    b["type"].as_str().unwrap().to_owned(),
                    b["items"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap().to_owned()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn one_set_per_archetype_in_order() {
        let sets = build(157, "Twitch", None, &[archetype("crit"), archetype("on_hit")], "16.19");
        let sets = sets.as_array().unwrap();

        assert_eq!(sets.len(), 2);
        assert_eq!(sets[0]["title"], "Crit Twitch - 60.1% - ARAM Mayhem");
        assert_eq!(sets[1]["title"], "On-Hit Twitch - 60.1% - ARAM Mayhem");
        assert_eq!(sets[0]["sortrank"], 1);
        assert_eq!(sets[1]["sortrank"], 2);
        assert_eq!(sets[0]["associatedChampions"], json!([157]));
        assert_eq!(sets[0]["associatedMaps"], json!([ARAM_MAP]));
    }

    #[test]
    fn the_title_follows_the_client_language() {
        assert_eq!(title("on_hit", "Twitch", Some("fr_FR"), 0.6012), "À l'impact Twitch - 60.1% - ARAM du chaos");
        assert_eq!(title("on_hit", "Twitch", Some("en_US"), 0.6012), "On-Hit Twitch - 60.1% - ARAM Mayhem");
        assert_eq!(title("crit", "Twitch", None, 0.5), "Crit Twitch - 50.0% - ARAM Mayhem");
    }

    #[test]
    fn an_unknown_build_type_falls_back_to_its_key_in_words() {
        assert_eq!(build_type("attack_speed", Some("fr_FR")), "Attack Speed");
        assert_eq!(build_type("ap", Some("fr_FR")), "AP");
    }

    #[test]
    fn blocks_are_in_buy_order_and_ids_are_strings() {
        let sets = build(157, "Twitch", None, &[archetype("crit")], "16.19");
        let b = blocks(&sets[0]);

        assert_eq!(
            b.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(),
            ["Starters", "Boots", "Core (in order)", "Situational"]
        );
        // The LCU rejects numeric ids here.
        assert_eq!(b[2].1, ["3032", "123430", "6333"], "core keeps the purchase order");
        assert_eq!(b[1].1, ["3006", "3111"]);
        assert_eq!(b[3].1, ["3065", "3072"]);
    }

    #[test]
    fn a_repeated_starter_appears_once() {
        let sets = build(157, "Twitch", None, &[archetype("crit")], "16.19");
        let starters = &blocks(&sets[0])[0].1;
        assert_eq!(starters, &["1038", "1042", "3006"], "3006 is listed once despite two options");
    }

    #[test]
    fn an_archetype_with_nothing_in_a_block_omits_it() {
        let mut a = archetype("crit");
        a.boots.clear();
        a.situational.clear();
        let sets = build(157, "Twitch", None, &[a], "16.19");

        assert_eq!(
            blocks(&sets[0]).iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["Starters", "Core (in order)"],
            "an empty block would render as a gap in game"
        );
    }
}
