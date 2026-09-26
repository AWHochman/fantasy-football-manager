use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Deserializer};

use crate::{
    AvailablePlayer, FantasySource, FantasyTeam, LeagueSnapshot, LineupSlot, LineupStatus,
    PlayerAvailability, PlayerEnrichment, Provider, RosteredPlayer, SourceError,
};

const FREE_AGENT_SLOT_IDS: [u8; 6] = [0, 2, 4, 6, 16, 17];
const FREE_AGENTS_PER_POSITION: u8 = 12;

// This is the read endpoint used by the `espn-api` Python library. ESPN does
// not document it as a public integration API, so failures are surfaced with
// a reconnect-friendly message below.
const ESPN_FANTASY_BASE: &str = "https://lm-api-reads.fantasy.espn.com/apis/v3/games/ffl";

/// Read-only client for one private ESPN fantasy football league.
///
/// `swid` and `espn_s2` are supplied by the caller and are never logged or
/// written to disk by this crate.
pub struct EspnSource {
    http: Client,
    league_id: u64,
    season: u16,
    swid: String,
    espn_s2: String,
}

impl EspnSource {
    pub fn new(
        league_id: u64,
        season: u16,
        swid: impl Into<String>,
        espn_s2: impl Into<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            league_id,
            season,
            swid: swid.into(),
            espn_s2: espn_s2.into(),
        }
    }

    pub fn with_http_client(
        league_id: u64,
        season: u16,
        swid: impl Into<String>,
        espn_s2: impl Into<String>,
        http: Client,
    ) -> Self {
        Self {
            http,
            league_id,
            season,
            swid: swid.into(),
            espn_s2: espn_s2.into(),
        }
    }

    pub async fn fetch_player_enrichment(
        &self,
        player_ids: &[i64],
        scoring_period: u8,
    ) -> Result<Vec<PlayerEnrichment>, SourceError> {
        if player_ids.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{ESPN_FANTASY_BASE}/seasons/{}/segments/0/leagues/{}?view=kona_playercard&scoringPeriodId={scoring_period}", self.season, self.league_id);
        let filter = serde_json::json!({"players": {"filterIds": {"value": player_ids}}});
        let response = self
            .http
            .get(url)
            .header(header::COOKIE, self.cookie_header())
            .header("x-fantasy-filter", filter.to_string())
            .send()
            .await?;
        if let Some(error) = source_error_for_status(response.status()) {
            return Err(error);
        }
        let payload: serde_json::Value = response.error_for_status()?.json().await?;
        Ok(payload["players"]
            .as_array()
            .ok_or_else(|| {
                SourceError::InvalidResponse("ESPN player response had no players list.".to_owned())
            })?
            .iter()
            .filter_map(|card| card.get("player").or(Some(card)))
            .filter_map(|player| normalize_player_enrichment(player, self.season, scoring_period))
            .collect())
    }

    /// Retrieves a bounded, read-only pool of the league's available players.
    /// ESPN includes waiver players in this view because they are actionable
    /// additions, even when they cannot be added immediately.
    pub async fn fetch_available_players(
        &self,
        scoring_period: u8,
    ) -> Result<Vec<AvailablePlayer>, SourceError> {
        let mut players = HashMap::new();
        for slot_id in FREE_AGENT_SLOT_IDS {
            for player in self
                .fetch_available_players_for_slot(scoring_period, slot_id)
                .await?
            {
                players
                    .entry(player.provider_player_id.clone())
                    .or_insert(player);
            }
        }
        Ok(players.into_values().collect())
    }

    async fn fetch_available_players_for_slot(
        &self,
        scoring_period: u8,
        slot_id: u8,
    ) -> Result<Vec<AvailablePlayer>, SourceError> {
        let url = format!("{ESPN_FANTASY_BASE}/seasons/{}/segments/0/leagues/{}?view=kona_player_info&scoringPeriodId={scoring_period}", self.season, self.league_id);
        let filter = serde_json::json!({"players": {
            "filterStatus": {"value": ["FREEAGENT", "WAIVERS"]},
            "filterSlotIds": {"value": [slot_id]},
            "limit": FREE_AGENTS_PER_POSITION,
            "sortPercOwned": {"sortPriority": 1, "sortAsc": false},
            "sortDraftRanks": {"sortPriority": 100, "sortAsc": true, "value": "STANDARD"}
        }});
        let response = self
            .http
            .get(url)
            .header(header::COOKIE, self.cookie_header())
            .header("x-fantasy-filter", filter.to_string())
            .send()
            .await?;
        if let Some(error) = source_error_for_status(response.status()) {
            return Err(error);
        }
        let payload: serde_json::Value = response.error_for_status()?.json().await?;
        Ok(payload["players"]
            .as_array()
            .ok_or_else(|| {
                SourceError::InvalidResponse(
                    "ESPN available-player response had no players list.".to_owned(),
                )
            })?
            .iter()
            .filter_map(|card| card.get("player").or(Some(card)))
            .filter_map(|player| normalize_available_player(player, self.season, scoring_period))
            .collect())
    }

    fn cookie_header(&self) -> String {
        format!("SWID={}; espn_s2={}", self.swid, self.espn_s2)
    }
}

#[async_trait]
impl FantasySource for EspnSource {
    fn provider(&self) -> Provider {
        Provider::Espn
    }

    async fn fetch_league(&self) -> Result<LeagueSnapshot, SourceError> {
        let url = format!(
            "{ESPN_FANTASY_BASE}/seasons/{}/segments/0/leagues/{}?view=mTeam&view=mRoster&view=mSettings",
            self.season, self.league_id
        );
        let response = self
            .http
            .get(url)
            .header(header::COOKIE, self.cookie_header())
            .header(
                header::USER_AGENT,
                "fantasy-football-manager/0.1 (personal read-only client)",
            )
            .send()
            .await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("unknown")
            .to_owned();
        let body = response.text().await?;

        if let Some(error) = source_error_for_status(status) {
            return Err(error);
        }
        if !status.is_success() {
            return Err(SourceError::InvalidResponse(format!(
                "ESPN returned HTTP {status}. Verify the managed ESPN league_id, ESPN_SWID, and ESPN_S2."
            )));
        }

        let document: serde_json::Value = serde_json::from_str(&body).map_err(|error| {
            SourceError::InvalidResponse(format!(
                "ESPN returned an unsupported response (HTTP {status}, content type {content_type}, {} bytes): {error}",
                body.len(),
            ))
        })?;
        let league_document = match document {
            serde_json::Value::Object(_) => document,
            serde_json::Value::Array(mut leagues) => leagues.pop().ok_or_else(|| {
                SourceError::InvalidResponse("ESPN returned an empty league response.".to_owned())
            })?,
            _ => {
                return Err(SourceError::InvalidResponse(
                    "ESPN returned JSON that was not a league object or array.".to_owned(),
                ));
            }
        };
        let response: EspnLeague = serde_json::from_value(league_document).map_err(|error| {
            SourceError::InvalidResponse(format!(
                "ESPN returned a league response with an unsupported field: {error}"
            ))
        })?;

        Ok(normalize(self.league_id, response))
    }
}

fn source_error_for_status(status: StatusCode) -> Option<SourceError> {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        .then_some(SourceError::EspnAuthenticationRequired)
}

fn normalize(league_id: u64, league: EspnLeague) -> LeagueSnapshot {
    let lineup_slots = normalize_lineup_slots(&league.settings.roster_settings.lineup_slot_counts);
    let league_name = league
        .settings
        .name
        .or(league.name)
        .unwrap_or_else(|| format!("ESPN league {league_id}"));
    let teams = league
        .teams
        .into_iter()
        .map(|team| FantasyTeam {
            team_id: team.id.to_string(),
            team_name: team.name.unwrap_or_else(|| {
                [team.location, team.nickname]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
            owner_id: team.owners.first().cloned(),
            owner_name: None,
            players: team
                .roster
                .entries
                .into_iter()
                .map(|entry| {
                    let status = lineup_status(entry.lineup_slot_id);
                    RosteredPlayer {
                        provider_player_id: entry.player_id.to_string(),
                        espn_player_id: Some(entry.player_id.to_string()),
                        eligible_positions: eligible_positions(
                            entry.player_pool_entry.player.default_position_id,
                        ),
                        is_locked: false,
                        game_start_time: None,
                        full_name: entry
                            .player_pool_entry
                            .player
                            .full_name
                            .unwrap_or_else(|| "Unknown player".to_owned()),
                        position: position_name(entry.player_pool_entry.player.default_position_id)
                            .map(str::to_owned),
                        nfl_team: pro_team_name(entry.player_pool_entry.player.pro_team_id)
                            .map(str::to_owned),
                        lineup_slot: lineup_slot_name(entry.lineup_slot_id).to_owned(),
                        lineup_status: status,
                        availability: PlayerAvailability {
                            is_on_bye: false,
                            is_confirmed_inactive: false,
                            injury_status: entry.player_pool_entry.player.injury_status,
                            projected_points: None,
                        },
                    }
                })
                .collect(),
        })
        .collect();

    LeagueSnapshot {
        provider: Provider::Espn,
        league_id: league_id.to_string(),
        league_name,
        scoring_period: league.scoring_period_id,
        lineup_slots,
        teams,
    }
}

fn normalize_lineup_slots(counts: &HashMap<String, i16>) -> Vec<LineupSlot> {
    let mut slots = Vec::new();
    for (slot_id, count) in counts {
        let Ok(slot_id) = slot_id.parse::<u8>() else {
            continue;
        };
        let Some(slot) = lineup_slot_definition(slot_id) else {
            continue;
        };
        for _ in 0..(*count).max(0) {
            slots.push(slot.clone());
        }
    }
    slots.sort_by(|left, right| left.name.cmp(&right.name));
    slots
}

fn lineup_slot_definition(slot_id: u8) -> Option<LineupSlot> {
    let (name, eligible_positions) = match slot_id {
        0 => ("QB", &["QB", "TQB"][..]),
        2 => ("RB", &["RB"][..]),
        4 => ("WR", &["WR"][..]),
        6 => ("TE", &["TE"][..]),
        7 => ("SUPERFLEX", &["QB", "TQB", "RB", "WR", "TE"][..]),
        16 => ("D/ST", &["D/ST"][..]),
        17 => ("K", &["K"][..]),
        23 => ("FLEX", &["RB", "WR", "TE"][..]),
        _ => return None,
    };
    Some(LineupSlot {
        name: name.to_owned(),
        eligible_positions: eligible_positions
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
    })
}

fn eligible_positions(position_id: Option<u8>) -> Vec<String> {
    position_name(position_id)
        .map(|position| position.split('/').map(str::to_owned).collect())
        .unwrap_or_default()
}

fn lineup_status(slot_id: u8) -> LineupStatus {
    match slot_id {
        20 => LineupStatus::Bench,
        21 => LineupStatus::Reserve,
        _ => LineupStatus::Starter,
    }
}

fn lineup_slot_name(slot_id: u8) -> &'static str {
    match slot_id {
        0 => "QB",
        2 => "RB",
        4 => "WR",
        6 => "TE",
        16 => "D/ST",
        17 => "K",
        20 => "BENCH",
        21 => "IR",
        23 => "FLEX",
        _ => "CUSTOM",
    }
}

fn position_name(position_id: Option<u8>) -> Option<&'static str> {
    match position_id? {
        0 => Some("QB"),
        1 => Some("TQB"),
        2 => Some("RB"),
        3 => Some("RB/WR"),
        4 => Some("WR"),
        5 => Some("WR/TE"),
        6 => Some("TE"),
        7 => Some("OP"),
        8 => Some("DT"),
        9 => Some("DE"),
        10 => Some("LB"),
        11 => Some("DL"),
        12 => Some("CB"),
        13 => Some("S"),
        14 => Some("DB"),
        15 => Some("DP"),
        16 => Some("D/ST"),
        17 => Some("K"),
        18 => Some("P"),
        19 => Some("HC"),
        _ => None,
    }
}

fn pro_team_name(pro_team_id: Option<u8>) -> Option<&'static str> {
    match pro_team_id? {
        1 => Some("ATL"),
        2 => Some("BUF"),
        3 => Some("CHI"),
        4 => Some("CIN"),
        5 => Some("CLE"),
        6 => Some("DAL"),
        7 => Some("DEN"),
        8 => Some("DET"),
        9 => Some("GB"),
        10 => Some("TEN"),
        11 => Some("IND"),
        12 => Some("KC"),
        13 => Some("LV"),
        14 => Some("LAR"),
        15 => Some("MIA"),
        16 => Some("MIN"),
        17 => Some("NE"),
        18 => Some("NO"),
        19 => Some("NYG"),
        20 => Some("NYJ"),
        21 => Some("PHI"),
        22 => Some("ARI"),
        23 => Some("PIT"),
        24 => Some("LAC"),
        25 => Some("SF"),
        26 => Some("SEA"),
        27 => Some("TB"),
        28 => Some("WSH"),
        29 => Some("CAR"),
        30 => Some("JAX"),
        33 => Some("BAL"),
        34 => Some("HOU"),
        _ => None,
    }
}

fn normalize_player_enrichment(
    player: &serde_json::Value,
    season: u16,
    scoring_period: u8,
) -> Option<PlayerEnrichment> {
    let projected_points = player
        .get("stats")
        .and_then(serde_json::Value::as_array)
        .and_then(|stats| {
            stats.iter().find_map(|stat| {
                (stat.get("seasonId")?.as_u64()? == u64::from(season)
                    && stat.get("scoringPeriodId")?.as_u64()? == u64::from(scoring_period)
                    && stat.get("statSourceId")?.as_u64()? == 1)
                    .then(|| stat.get("appliedTotal")?.as_f64())
                    .flatten()
            })
        });
    Some(PlayerEnrichment {
        provider_player_id: player.get("id")?.as_i64()?.to_string(),
        full_name: player.get("fullName")?.as_str()?.to_owned(),
        position: position_name(
            player
                .get("defaultPositionId")
                .and_then(serde_json::Value::as_u64)
                .map(|id| id as u8),
        )
        .map(str::to_owned),
        nfl_team: pro_team_name(
            player
                .get("proTeamId")
                .and_then(serde_json::Value::as_u64)
                .map(|id| id as u8),
        )
        .map(str::to_owned),
        injury_status: player
            .get("injuryStatus")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        is_on_bye: None,
        projected_points,
    })
}

fn normalize_available_player(
    player: &serde_json::Value,
    season: u16,
    scoring_period: u8,
) -> Option<AvailablePlayer> {
    let enrichment = normalize_player_enrichment(player, season, scoring_period)?;
    let position_id = player
        .get("defaultPositionId")
        .and_then(serde_json::Value::as_u64)
        .map(|id| id as u8);
    Some(AvailablePlayer {
        provider_player_id: enrichment.provider_player_id.clone(),
        espn_player_id: Some(enrichment.provider_player_id),
        eligible_positions: eligible_positions(position_id),
        is_locked: false,
        game_start_time: None,
        full_name: enrichment.full_name,
        position: enrichment.position,
        nfl_team: enrichment.nfl_team,
        availability: PlayerAvailability {
            is_on_bye: enrichment.is_on_bye.unwrap_or(false),
            is_confirmed_inactive: false,
            injury_status: enrichment.injury_status,
            projected_points: enrichment.projected_points,
        },
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EspnLeague {
    name: Option<String>,
    scoring_period_id: Option<u8>,
    #[serde(default, deserialize_with = "null_to_default")]
    settings: EspnSettings,
    #[serde(default, deserialize_with = "null_to_default")]
    teams: Vec<EspnTeam>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EspnSettings {
    name: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    roster_settings: EspnRosterSettings,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EspnRosterSettings {
    #[serde(default, deserialize_with = "null_to_default")]
    lineup_slot_counts: HashMap<String, i16>,
}

#[derive(Deserialize)]
struct EspnTeam {
    id: u64,
    name: Option<String>,
    location: Option<String>,
    nickname: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    owners: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    roster: EspnRoster,
}

#[derive(Default, Deserialize)]
struct EspnRoster {
    #[serde(default, deserialize_with = "null_to_default")]
    entries: Vec<EspnRosterEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EspnRosterEntry {
    // ESPN represents D/ST entries with negative player IDs.
    player_id: i64,
    lineup_slot_id: u8,
    player_pool_entry: EspnPlayerPoolEntry,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EspnPlayerPoolEntry {
    player: EspnPlayer,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EspnPlayer {
    full_name: Option<String>,
    default_position_id: Option<u8>,
    pro_team_id: Option<u8>,
    injury_status: Option<String>,
}

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_espn_bench_and_ir_slots() {
        assert_eq!(lineup_status(0), LineupStatus::Starter);
        assert_eq!(lineup_status(20), LineupStatus::Bench);
        assert_eq!(lineup_status(21), LineupStatus::Reserve);
        assert_eq!(lineup_slot_name(23), "FLEX");
    }

    #[test]
    fn deserializes_a_minimal_league_response() {
        let league: EspnLeague = serde_json::from_str(
            r#"{
                "name": "Monday league",
                "settings": {"name": "Settings league"},
                "teams": [{
                    "id": 1,
                    "name": "Custom name",
                    "location": "Team",
                    "nickname": "One",
                    "owners": ["owner-1"],
                    "roster": {"entries": [{
                        "playerId": -16030,
                        "lineupSlotId": 20,
                        "playerPoolEntry": {"player": {
                            "fullName": "Bench Player",
                            "defaultPositionId": 4,
                            "proTeamId": 2,
                            "injuryStatus": "ACTIVE"
                        }}
                    }]}
                }]
            }"#,
        )
        .expect("fixture should match the ESPN response shape");

        let snapshot = normalize(42, league);
        assert_eq!(snapshot.league_name, "Settings league");
        assert_eq!(snapshot.teams[0].team_name, "Custom name");
        assert_eq!(snapshot.teams[0].players[0].provider_player_id, "-16030");
        assert_eq!(snapshot.teams[0].players[0].position.as_deref(), Some("WR"));
        assert_eq!(
            snapshot.teams[0].players[0].nfl_team.as_deref(),
            Some("BUF")
        );
        assert_eq!(
            snapshot.teams[0].players[0].lineup_status,
            LineupStatus::Bench
        );
    }

    #[test]
    fn accepts_null_optional_collections() {
        let league: EspnLeague = serde_json::from_str(r#"{"name": "Empty", "teams": null}"#)
            .expect("ESPN may return null instead of an empty team collection");

        assert!(league.teams.is_empty());
    }

    #[test]
    fn accepts_the_array_response_variant() {
        let response: Vec<EspnLeague> =
            serde_json::from_str(r#"[{"name": "League", "teams": []}]"#)
                .expect("ESPN may wrap a league response in an array");

        assert_eq!(response.len(), 1);
    }

    #[test]
    fn maps_known_espn_player_and_team_ids() {
        assert_eq!(position_name(Some(16)), Some("D/ST"));
        assert_eq!(pro_team_name(Some(34)), Some("HOU"));
        assert_eq!(position_name(Some(99)), None);
    }

    #[test]
    fn recognizes_expired_espn_sessions() {
        assert!(matches!(
            source_error_for_status(StatusCode::UNAUTHORIZED),
            Some(SourceError::EspnAuthenticationRequired)
        ));
        assert!(matches!(
            source_error_for_status(StatusCode::FORBIDDEN),
            Some(SourceError::EspnAuthenticationRequired)
        ));
        assert!(source_error_for_status(StatusCode::INTERNAL_SERVER_ERROR).is_none());
    }
}
