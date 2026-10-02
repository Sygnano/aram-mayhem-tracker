//! League client (LCU) REST adapter. Riot does not support the LCU for third parties:
//! every call can fail, and callers must degrade, not crash.
//!
//! Polled rather than subscribed to over the WAMP websocket. The engine's client loop reads
//! the gameflow phase once a second, and the champ-select session with it while one is open.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::lockfile::Credentials;
use crate::tls::riot_client;
use crate::ApiError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum GameflowPhase {
    None,
    Lobby,
    Matchmaking,
    ReadyCheck,
    ChampSelect,
    GameStart,
    InProgress,
    Reconnect,
    WaitingForStats,
    PreEndOfGame,
    EndOfGame,
    Other(String),
}

impl From<String> for GameflowPhase {
    fn from(s: String) -> Self {
        match s.as_str() {
            "None" => Self::None,
            "Lobby" => Self::Lobby,
            "Matchmaking" => Self::Matchmaking,
            "ReadyCheck" => Self::ReadyCheck,
            "ChampSelect" => Self::ChampSelect,
            "GameStart" => Self::GameStart,
            "InProgress" => Self::InProgress,
            "Reconnect" => Self::Reconnect,
            "WaitingForStats" => Self::WaitingForStats,
            "PreEndOfGame" => Self::PreEndOfGame,
            "EndOfGame" => Self::EndOfGame,
            _ => Self::Other(s),
        }
    }
}

impl From<GameflowPhase> for String {
    fn from(p: GameflowPhase) -> Self {
        match p {
            GameflowPhase::Other(s) => s,
            other => format!("{other:?}"),
        }
    }
}

/// The parts of `/lol-gameflow/v1/session` we use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueInfo {
    pub id: i64,
    pub description: String,
    pub game_mode: String,
    pub map_id: i64,
}

impl QueueInfo {
    pub fn from_session(session: &Value) -> Option<Self> {
        let q = session.pointer("/gameData/queue")?;
        Some(Self {
            id: q.get("id")?.as_i64()?,
            description: q.get("description").and_then(Value::as_str).unwrap_or_default().to_owned(),
            game_mode: q.get("gameMode").and_then(Value::as_str).unwrap_or_default().to_owned(),
            map_id: q.get("mapId").and_then(Value::as_i64).unwrap_or_default(),
        })
    }
}

#[derive(Clone)]
pub struct LcuClient {
    http: reqwest::Client,
    base: String,
    auth: String,
}

impl LcuClient {
    pub fn new(credentials: &Credentials) -> Self {
        Self {
            http: riot_client(Duration::from_secs(3)),
            base: credentials.base_url(),
            auth: credentials.authorization(),
        }
    }

    pub async fn get_json(&self, path: &str) -> Result<Value, ApiError> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .send()
            .await
            .map_err(ApiError::from_reqwest)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ApiError::Status { status: status.as_u16(), path: path.into() });
        }
        resp.json().await.map_err(ApiError::from_reqwest)
    }

    pub async fn gameflow_phase(&self) -> Result<GameflowPhase, ApiError> {
        let v = self.get_json("/lol-gameflow/v1/gameflow-phase").await?;
        serde_json::from_value(v).map_err(|e| ApiError::Decode(e.to_string()))
    }

    pub async fn gameflow_session(&self) -> Result<Value, ApiError> {
        self.get_json("/lol-gameflow/v1/session").await
    }

    /// A POST with no body, for the LCU's action endpoints.
    ///
    /// The response body is discarded: these endpoints answer with the new session, which we would
    /// only re-read on the next poll anyway.
    pub async fn post_empty(&self, path: &str) -> Result<(), ApiError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .send()
            .await
            .map_err(ApiError::from_reqwest)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ApiError::Status { status: status.as_u16(), path: path.into() });
        }
        Ok(())
    }

    /// Swaps the local player's champion for one on the bench.
    ///
    /// This is the same action as clicking the champion in the client, and it is the only *write* the
    /// champ-select overlay performs. It is deliberately only ever called from a click: a user asking
    /// for a swap is not automation, and nothing here should ever decide to swap on its own.
    ///
    /// Confirmed against live champ selects. Failures surface in the overlay rather than
    /// being swallowed.
    pub async fn bench_swap(&self, champion_id: i64) -> Result<(), ApiError> {
        self.post_empty(&format!("/lol-champ-select/v1/session/bench/swap/{champion_id}")).await
    }

    /// Has the local player already answered the ready check on screen?
    ///
    /// Asked before accepting for them, so a decline made by hand is never overturned.
    ///
    /// `[unverified]`: `playerResponse` is `None`, `Accepted` or `Declined` in the LCU's published
    /// schema; this has not been read off a live ready check.
    pub async fn ready_check_answered(&self) -> Result<bool, ApiError> {
        Ok(ready_check_answered(&self.get_json("/lol-matchmaking/v1/ready-check").await?))
    }

    /// Accepts the ready check: the same action as clicking Accept in the client.
    ///
    /// `[unverified]`: the endpoint is the one other tools use for this; it has not
    /// yet been fired against a live ready check.
    pub async fn accept_ready_check(&self) -> Result<(), ApiError> {
        self.post_empty("/lol-matchmaking/v1/ready-check/accept").await
    }

    /// The local player's chat status, or `None` when it is one we do not offer: the client sets
    /// `dnd` by itself in queue and in game.
    pub async fn chat_status(&self) -> Result<Option<ChatStatus>, ApiError> {
        let v = self.get_json("/lol-chat/v1/me").await?;
        Ok(v.get("availability").and_then(Value::as_str).and_then(ChatStatus::from_availability))
    }

    /// Sets the local player's chat status: the same field the client's own status menu
    /// writes. Only ever called because the user chose one.
    pub async fn set_chat_status(&self, status: ChatStatus) -> Result<(), ApiError> {
        self.put_json("/lol-chat/v1/me", &serde_json::json!({ "availability": status.availability() })).await
    }

    /// A PUT with a JSON body, for item sets and the chat status.
    pub async fn put_json(&self, path: &str, body: &Value) -> Result<(), ApiError> {
        let resp = self
            .http
            .put(format!("{}{path}", self.base))
            .header(reqwest::header::AUTHORIZATION, &self.auth)
            .json(body)
            .send()
            .await
            .map_err(ApiError::from_reqwest)?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ApiError::Status { status: status.as_u16(), path: path.into() });
        }
        Ok(())
    }

    /// The signed-in summoner. Item sets are stored per summoner id.
    pub async fn current_summoner_id(&self) -> Result<i64, ApiError> {
        let v = self.get_json("/lol-summoner/v1/current-summoner").await?;
        v.get("summonerId")
            .and_then(Value::as_i64)
            .ok_or_else(|| ApiError::Decode("current-summoner has no summonerId".into()))
    }

    /// The champ-select session, reduced to what we use.
    ///
    /// One request serves both callers that need it — the item-set writer wants the local player's
    /// champion, the champ-select overlay wants every champion on screen — so they do not poll the
    /// same endpoint twice a second each.
    ///
    /// Outside champ select the endpoint 404s, which surfaces as `Err(ApiError::Status)`.
    pub async fn champ_select(&self) -> Result<ChampSelect, ApiError> {
        let v = self.get_json("/lol-champ-select/v1/session").await?;
        let mut cs = ChampSelect::from_session(&v);
        // Only while the cards are actually up. Outside that window the endpoint has nothing to say
        // and answers 404, and asking anyway would be one guaranteed failure per poll for the rest of
        // champ select. Best effort even then: losing it must not cost us the session.
        if cs.phase == "BAN_PICK" {
            cs.subset = self.subset_champion_list().await.unwrap_or_default();
        }
        Ok(cs)
    }

    /// The champions offered on the big pick cards.
    ///
    /// Lives under `lol-lobby-team-builder`, not `lol-champ-select`, which is part of why it took a
    /// dump to find: nothing in the champ-select session points at it.
    pub async fn subset_champion_list(&self) -> Result<Vec<i64>, ApiError> {
        let v = self.get_json("/lol-lobby-team-builder/champ-select/v1/subset-champion-list").await?;
        Ok(v.as_array().map(|a| a.iter().filter_map(Value::as_i64).filter(|id| *id > 0).collect()).unwrap_or_default())
    }

    /// Replaces **every** item set on the account. The endpoint takes the whole collection,
    /// so there is no way to add one without rewriting the rest.
    pub async fn replace_item_sets(&self, summoner_id: i64, item_sets: Value) -> Result<(), ApiError> {
        let body = serde_json::json!({ "accountId": summoner_id, "itemSets": item_sets, "timestamp": 0 });
        self.put_json(&format!("/lol-item-sets/v1/item-sets/{summoner_id}/sets"), &body).await
    }

    /// `{"locale": "fr_FR", "region": "EUW", ...}`: the *game's* language, which is what OCR has
    /// to read and which CommunityDragon locale to load.
    pub async fn region_locale(&self) -> Result<Option<String>, ApiError> {
        let v = self.get_json("/riotclient/region-locale").await?;
        Ok(v.get("locale").and_then(Value::as_str).map(str::to_owned))
    }
}

/// The chat statuses the companion window offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatStatus {
    Online,
    Away,
    Offline,
}

impl ChatStatus {
    /// The LCU's name for it, in `availability` of `/lol-chat/v1/me`. Online is `chat`.
    pub fn availability(self) -> &'static str {
        match self {
            Self::Online => "chat",
            Self::Away => "away",
            Self::Offline => "offline",
        }
    }

    pub fn from_availability(availability: &str) -> Option<Self> {
        [Self::Online, Self::Away, Self::Offline].into_iter().find(|s| s.availability() == availability)
    }
}

/// Only an explicit answer counts. A payload without the field reads as unanswered, so a change in
/// the client's schema costs the check, not the feature.
fn ready_check_answered(ready_check: &Value) -> bool {
    matches!(ready_check.get("playerResponse").and_then(Value::as_str), Some("Accepted" | "Declined"))
}

/// The champions an ARAM champ select is showing, and which one is ours.
///
/// This is the *identity* half of the champ-select overlay: the LCU says which champions are on
/// screen and in what order, and a layout fitted to real captures says where the client draws them
/// (`mayhem_core::champselect`). Nothing is read off the screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChampSelect {
    /// The local player's champion, if they have one. In ARAM one is assigned immediately rather
    /// than picked, so this is normally set from the moment champ select opens.
    pub local_champion: Option<i64>,
    /// The five allies **by seat**, in `cellId` order, which is the order the client draws the rows
    /// in. `None` is a seat whose player has no champion yet.
    ///
    /// Positions are kept rather than compacted. A real dump had `myTeam` champions
    /// `[126, 0, 113, 17, 0]`: dropping the empty seats would move row 3's champion onto row 2 and
    /// label two teammates wrongly.
    pub team: Vec<Option<i64>>,
    /// The bench, in `benchChampions` order. This is what the available-champions strip shows, and
    /// the only thing a swap can target.
    pub bench: Vec<i64>,
    /// The champions offered on the big pick cards, left to right, from
    /// `/lol-lobby-team-builder/champ-select/v1/subset-champion-list`.
    ///
    /// A **separate endpoint**, and separate from the bench: during the card phase the two
    /// differ entirely. From three dumps taken across a real champ select: the local player had *no*
    /// champion (`0`), the bench held `[103, 235, 360]`, the subset held `[112, 40, 421]`, and the
    /// champion actually obtained was `421`, from the subset. It is fetched alongside the session
    /// and merged in, so callers see one object.
    pub subset: Vec<i64>,
    /// `timer.phase`: `PLANNING`, `BAN_PICK`, `FINALIZATION`, `GAME_STARTING`.
    ///
    /// This is what says whether the big pick cards are on screen, and it is why none of this needs
    /// to look at the screen at all.
    pub phase: String,
    /// Milliseconds left in the current phase.
    pub time_left_ms: i64,
    /// `timer.totalTimeInPhase`: how long the current phase runs for in total, or `0` when the
    /// client does not report it — the field is undocumented, so nothing is timed against it.
    /// Carried for the settings screen only.
    pub phase_total_ms: i64,
}

impl ChampSelect {
    fn from_session(v: &Value) -> Self {
        // The bench is a plain list: every entry is a real champion, and order is the order drawn.
        let ids = |key: &str| -> Vec<i64> {
            v.get(key)
                .and_then(Value::as_array)
                .map(|cells| {
                    cells
                        .iter()
                        .filter_map(|c| c.get("championId").and_then(Value::as_i64))
                        .filter(|id| *id > 0)
                        .collect()
                })
                .unwrap_or_default()
        };
        // The team is seats, not a list: an empty one has to stay empty so the rest keep their rows.
        let team = {
            let mut cells: Vec<(i64, Option<i64>)> = v
                .get("myTeam")
                .and_then(Value::as_array)
                .map(|cells| {
                    cells
                        .iter()
                        .map(|c| {
                            let cell = c.get("cellId").and_then(Value::as_i64).unwrap_or(i64::MAX);
                            let champ = c.get("championId").and_then(Value::as_i64).filter(|id| *id > 0);
                            (cell, champ)
                        })
                        .collect()
                })
                .unwrap_or_default();
            cells.sort_by_key(|(cell, _)| *cell);
            cells.into_iter().map(|(_, champ)| champ).collect()
        };
        let me = v.get("localPlayerCellId").and_then(Value::as_i64);
        let local_champion = me
            .and_then(|me| {
                v.get("myTeam")
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|p| p.get("cellId").and_then(Value::as_i64) == Some(me))
                    .and_then(|p| p.get("championId").and_then(Value::as_i64))
            })
            .filter(|id| *id > 0);
        let timer = v.get("timer");
        // `subset` comes from its own endpoint and is merged in by `champ_select`; the session
        // says nothing about it.
        Self {
            local_champion,
            team,
            bench: ids("benchChampions"),
            subset: Vec::new(),
            phase: timer.and_then(|t| t.get("phase")).and_then(Value::as_str).unwrap_or_default().to_owned(),
            time_left_ms: timer
                .and_then(|t| t.get("adjustedTimeLeftInPhase"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            phase_total_ms: timer.and_then(|t| t.get("totalTimeInPhase")).and_then(Value::as_i64).unwrap_or_default(),
        }
    }

    /// Are the big pick cards on screen **and settled**?
    ///
    /// **From the phase, not from the screen.** `BAN_PICK` is the ~15-second window where the offer
    /// is shown; by `FINALIZATION` the cards are gone and only the strip remains. Verified against
    /// three dumps taken through one champ select: cards up at `BAN_PICK` with no champion assigned,
    /// gone at `FINALIZATION` with one assigned.
    ///
    /// The entrance animation is **not** waited out here. Doing that from the timer needs the phase's
    /// length, and `totalTimeInPhase` is not in the LCU specification at all — a wrong or absent
    /// total made the arithmetic say the phase had barely started for its whole duration, and no card
    /// block was ever drawn. The wait is now held by `ChampSelectState`, against its own clock.
    pub fn cards_up(&self) -> bool {
        self.phase == "BAN_PICK" && !self.subset.is_empty()
    }

    /// Can this champion be swapped to? Only the bench can; your own champion is already yours.
    pub fn is_on_bench(&self, champion_id: i64) -> bool {
        self.bench.contains(&champion_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_round_trip_including_unknown() {
        let p: GameflowPhase = serde_json::from_str("\"ChampSelect\"").unwrap();
        assert_eq!(p, GameflowPhase::ChampSelect);
        let q: GameflowPhase = serde_json::from_str("\"SomethingNew\"").unwrap();
        assert_eq!(q, GameflowPhase::Other("SomethingNew".into()));
        assert_eq!(serde_json::to_string(&GameflowPhase::InProgress).unwrap(), "\"InProgress\"");
    }

    #[test]
    fn chat_statuses_map_to_the_clients_availability() {
        assert_eq!(ChatStatus::Online.availability(), "chat");
        assert_eq!(ChatStatus::from_availability("away"), Some(ChatStatus::Away));
        assert_eq!(ChatStatus::from_availability("offline"), Some(ChatStatus::Offline));
        assert_eq!(ChatStatus::from_availability("dnd"), None, "the client's own in-queue status");
        assert_eq!(serde_json::to_string(&ChatStatus::Online).unwrap(), "\"online\"");
    }

    #[test]
    fn a_ready_check_is_answered_only_by_an_explicit_response() {
        let with = |response: &str| serde_json::json!({ "state": "InProgress", "playerResponse": response });
        assert!(!ready_check_answered(&with("None")));
        assert!(ready_check_answered(&with("Accepted")));
        assert!(ready_check_answered(&with("Declined")), "a decline made by hand is an answer");
        assert!(!ready_check_answered(&serde_json::json!({})));
    }

    #[test]
    fn reads_every_champion_on_an_aram_champ_select_screen() {
        let session = serde_json::json!({
            "localPlayerCellId": 2,
            "myTeam": [
                { "cellId": 0, "championId": 22 },
                { "cellId": 1, "championId": 0 },
                { "cellId": 2, "championId": 64 },
                { "cellId": 3, "championId": 103 }
            ],
            "benchChampions": [{ "championId": 84 }, { "championId": 22 }, { "championId": 0 }]
        });
        let cs = ChampSelect::from_session(&session);
        assert_eq!(cs.local_champion, Some(64), "ours is the cell matching localPlayerCellId");
        // A cell with championId 0 is a player who has none yet: there is nothing on screen to
        // label, so it is not a champion to look for.
        // Seats are kept: cell 1 has nobody, and that hole must survive or everyone after it moves
        // up a row on screen.
        assert_eq!(cs.team, vec![Some(22), None, Some(64), Some(103)]);
        assert_eq!(cs.bench, vec![84, 22]);
    }

    #[test]
    fn a_session_with_nothing_in_it_yields_nothing() {
        let cs = ChampSelect::from_session(&serde_json::json!({}));
        assert_eq!(cs, ChampSelect::default());
        // Champ select open, but we have no champion and no cell is filled in yet.
        let early = ChampSelect::from_session(&serde_json::json!({ "myTeam": [{ "cellId": 0, "championId": 0 }] }));
        assert_eq!(early.local_champion, None);
        assert_eq!(early.team, vec![None], "the seat is kept, empty");
    }

    /// Seats keep their rows, and the phase says whether the cards are up — both straight from the
    /// LCU, with nothing read off the screen.
    ///
    /// The champion ids and the hole in the middle are from a real `BAN_PICK` dump.
    #[test]
    fn seats_keep_their_positions_and_the_phase_says_whether_cards_are_up() {
        let session = serde_json::json!({
            "localPlayerCellId": 4,
            "myTeam": [
                { "cellId": 2, "championId": 113 },
                { "cellId": 0, "championId": 126 },
                { "cellId": 4, "championId": 0 },
                { "cellId": 1, "championId": 0 },
                { "cellId": 3, "championId": 17 }
            ],
            "benchChampions": [{ "championId": 103 }, { "championId": 235 }, { "championId": 360 }],
            "timer": { "phase": "BAN_PICK", "adjustedTimeLeftInPhase": 8706, "totalTimeInPhase": 15000 }
        });
        let mut cs = ChampSelect::from_session(&session);

        // Sorted by cell, holes preserved: rows 2 and 5 are genuinely nobody.
        assert_eq!(cs.team, vec![Some(126), None, Some(113), Some(17), None]);
        assert_eq!(cs.phase, "BAN_PICK");
        assert_eq!(cs.time_left_ms, 8706);

        // The offer has not been merged in yet, so there is nothing to draw cards for.
        assert!(!cs.cards_up());
        cs.subset = vec![112, 40, 421];
        assert!(cs.cards_up(), "the offer is on the cards");

        // The timer is reported but not judged on: the entrance wait lives in `ChampSelectState`.
        cs.time_left_ms = 14_000;
        assert!(cs.cards_up());
        cs.time_left_ms = 0;
        assert!(cs.cards_up());

        // By finalisation the cards are gone, even though the subset endpoint still answers.
        cs.phase = "FINALIZATION".into();
        assert!(!cs.cards_up());
    }

    /// The cards and the bench are different lists, and during the card phase they share nothing.
    ///
    /// From a real `BAN_PICK` dump: the local player had no champion, the bench held
    /// `[103, 235, 360]`, the subset held `[112, 40, 421]` — and the next dump showed the champion
    /// obtained was `421`, from the subset. Anything that derived the cards from the bench would have
    /// put three wrong champions on screen.
    #[test]
    fn pick_cards_come_from_the_subset_and_the_bench_is_something_else() {
        let session = serde_json::json!({
            "localPlayerCellId": 4,
            "myTeam": [{ "cellId": 4, "championId": 0 }],
            "benchChampions": [{ "championId": 103 }, { "championId": 235 }, { "championId": 360 }]
        });
        let mut cs = ChampSelect::from_session(&session);
        cs.subset = vec![112, 40, 421];

        assert_eq!(cs.local_champion, None, "no champion yet while the cards are up");
        assert!(cs.subset.iter().all(|id| !cs.bench.contains(id)), "they share nothing");
        assert_eq!(cs.bench, vec![103, 235, 360]);
        assert!(cs.is_on_bench(103) && !cs.is_on_bench(112), "only the bench can be swapped to");
    }

    /// Outside the card phase the subset is empty, and that is not an error: there are simply no
    /// cards on screen to label.
    #[test]
    fn no_subset_means_no_cards() {
        let session = serde_json::json!({
            "localPlayerCellId": 4,
            "myTeam": [{ "cellId": 4, "championId": 421 }],
            "benchChampions": [{ "championId": 103 }]
        });
        let cs = ChampSelect::from_session(&session);
        assert_eq!(cs.local_champion, Some(421));
        assert!(cs.subset.is_empty());
        assert!(!cs.cards_up());
    }

    #[test]
    fn reads_queue_from_session() {
        let session = serde_json::json!({
            "phase": "InProgress",
            "gameData": { "queue": { "id": 2400, "description": "ARAM: Mayhem", "gameMode": "KIWI", "mapId": 12 } }
        });
        let q = QueueInfo::from_session(&session).unwrap();
        assert_eq!((q.id, q.map_id, q.game_mode.as_str()), (2400, 12, "KIWI"));
        assert!(QueueInfo::from_session(&serde_json::json!({})).is_none());
    }
}
