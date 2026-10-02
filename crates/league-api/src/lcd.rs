//! Live Client Data API, `https://127.0.0.1:2999/liveclientdata/*`.
//!
//! One `/allgamedata` poll per tick, fanned out internally.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::tls::riot_client;
use crate::ApiError;

pub const LCD_BASE: &str = "https://127.0.0.1:2999";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChampionStats {
    pub current_health: f64,
    pub max_health: f64,
}

/// `activePlayer`. In spectator mode this is `{"error": ...}`, which lenient decoding turns into
/// an all-default value; check [`ActivePlayer::is_present`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivePlayer {
    pub level: u8,
    pub current_gold: f64,
    pub riot_id: String,
    pub summoner_name: String,
    pub champion_stats: ChampionStats,
}

impl ActivePlayer {
    pub fn is_present(&self) -> bool {
        !self.riot_id.is_empty() || !self.summoner_name.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Player {
    pub champion_name: String,
    pub raw_champion_name: String,
    pub riot_id: String,
    pub summoner_name: String,
    pub is_bot: bool,
    pub is_dead: bool,
    pub level: u8,
    pub respawn_timer: f64,
    pub team: String,
}

impl Player {
    /// Internal champion key, e.g. `Yasuo` from `game_character_displayname_Yasuo`. Locale
    /// independent, unlike `championName`.
    ///
    /// `rawChampionName` comes in two string-key formats. Most champions use
    /// `game_character_displayname_<Key>`, but Seraphine reports `Character_Seraphine_Name`
    /// (captured live on 16.19, French client). Only when neither format matches does this fall back
    /// to `championName`, which is localised ("Séraphine") and may not resolve.
    pub fn champion_key(&self) -> &str {
        let raw = self.raw_champion_name.as_str();
        raw.strip_prefix("game_character_displayname_")
            .or_else(|| raw.strip_prefix("Character_").and_then(|r| r.strip_suffix("_Name")))
            .filter(|key| !key.is_empty())
            .unwrap_or(&self.champion_name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    #[serde(rename = "EventID")]
    pub id: i64,
    #[serde(rename = "EventName")]
    pub name: String,
    #[serde(rename = "EventTime")]
    pub time: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Events {
    #[serde(rename = "Events")]
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameData {
    pub game_mode: String,
    pub game_time: f64,
    pub map_name: String,
    pub map_number: i64,
    pub map_terrain: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AllGameData {
    pub active_player: ActivePlayer,
    pub all_players: Vec<Player>,
    pub events: Events,
    pub game_data: GameData,
}

impl AllGameData {
    /// The active player's entry in `allPlayers` (for `isDead` and `respawnTimer`, which
    /// `activePlayer` lacks).
    pub fn me(&self) -> Option<&Player> {
        let ap = &self.active_player;
        if !ap.is_present() {
            return None;
        }
        self.all_players.iter().find(|p| {
            (!ap.riot_id.is_empty() && p.riot_id == ap.riot_id)
                || (!ap.summoner_name.is_empty() && p.summoner_name == ap.summoner_name)
        })
    }
}

#[derive(Clone)]
pub struct LiveClient {
    http: reqwest::Client,
    base: String,
}

impl Default for LiveClient {
    fn default() -> Self {
        Self::new(LCD_BASE)
    }
}

impl LiveClient {
    pub fn new(base: &str) -> Self {
        Self { http: riot_client(Duration::from_millis(1500)), base: base.trim_end_matches('/').to_owned() }
    }

    /// One poll, decoded straight into the typed view.
    ///
    /// Lenient: unknown fields are ignored and missing ones default. Only a payload that is not a
    /// JSON object at all is an error.
    pub async fn all_game_data(&self) -> Result<AllGameData, ApiError> {
        const PATH: &str = "/liveclientdata/allgamedata";
        let resp = self.http.get(format!("{}{PATH}", self.base)).send().await.map_err(ApiError::from_reqwest)?;
        let status = resp.status();
        if !status.is_success() {
            // 404 during the loading screen is normal: the API is up but has no game data yet.
            return Err(ApiError::Status { status: status.as_u16(), path: PATH.into() });
        }
        resp.json().await.map_err(ApiError::from_reqwest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_leniently() {
        let d: AllGameData =
            serde_json::from_str(include_str!("../tests/fixtures/allgamedata.synthetic.json")).unwrap();
        assert_eq!(d.game_data.game_mode, "KIWI");
        assert_eq!(d.game_data.map_number, 12);
        assert_eq!(d.active_player.level, 7);
        let me = d.me().unwrap();
        assert!(me.is_dead);
        assert_eq!(me.champion_key(), "Yasuo");
        assert!(d.events.events.iter().any(|e| e.name == "GameStart"));
        // The second player has no `items`; the unknown activePlayer field is ignored.
        assert_eq!(d.all_players[1].team, "CHAOS");
    }

    #[test]
    fn champion_key_reads_both_raw_name_formats() {
        let player = |raw: &str, display: &str| Player {
            raw_champion_name: raw.into(),
            champion_name: display.into(),
            ..Default::default()
        };
        assert_eq!(player("game_character_displayname_LeeSin", "Lee Sin").champion_key(), "LeeSin");
        // Seraphine on 16.19, captured from a French client: the other format, and an accented
        // display name that the English champion index cannot match.
        assert_eq!(player("Character_Seraphine_Name", "Séraphine").champion_key(), "Seraphine");
        // Neither format: the display name is all there is.
        assert_eq!(player("something_else", "Séraphine").champion_key(), "Séraphine");
        assert_eq!(player("Character__Name", "Séraphine").champion_key(), "Séraphine");
    }

    #[test]
    fn spectator_shape_has_no_me() {
        let v = serde_json::json!({ "activePlayer": { "error": "Spectator mode" }, "allPlayers": [] });
        let d: AllGameData = serde_json::from_value(v).unwrap();
        assert!(!d.active_player.is_present());
        assert!(d.me().is_none());
        assert!(serde_json::from_str::<AllGameData>("null").is_err(), "not an object at all");
    }

    #[tokio::test]
    async fn closed_port_is_not_running() {
        // Port 9 (discard) on loopback is closed in any sane test environment.
        let client = LiveClient::new("https://127.0.0.1:9");
        assert!(matches!(client.all_game_data().await, Err(ApiError::NotRunning)));
    }
}
