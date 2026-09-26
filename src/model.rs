use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum Provider {
    Espn,
    Sleeper,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LeagueSnapshot {
    pub provider: Provider,
    pub league_id: String,
    pub league_name: String,
    #[serde(default)]
    pub scoring_period: Option<u8>,
    #[serde(default)]
    pub lineup_slots: Vec<LineupSlot>,
    pub teams: Vec<FantasyTeam>,
}

/// One starting slot and the player positions it accepts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LineupSlot {
    pub name: String,
    pub eligible_positions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FantasyTeam {
    pub team_id: String,
    pub team_name: String,
    pub owner_id: Option<String>,
    pub owner_name: Option<String>,
    pub players: Vec<RosteredPlayer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RosteredPlayer {
    pub provider_player_id: String,
    #[serde(default)]
    pub espn_player_id: Option<String>,
    #[serde(default)]
    pub eligible_positions: Vec<String>,
    #[serde(default)]
    pub is_locked: bool,
    #[serde(default)]
    pub game_start_time: Option<DateTime<Utc>>,
    pub full_name: String,
    pub position: Option<String>,
    pub nfl_team: Option<String>,
    pub lineup_slot: String,
    pub lineup_status: LineupStatus,
    pub availability: PlayerAvailability,
}

/// A player currently available to add in a league. Provider clients are
/// responsible for determining availability; the optimizer treats this as a
/// provider-neutral candidate.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AvailablePlayer {
    pub provider_player_id: String,
    #[serde(default)]
    pub espn_player_id: Option<String>,
    #[serde(default)]
    pub eligible_positions: Vec<String>,
    #[serde(default)]
    pub is_locked: bool,
    #[serde(default)]
    pub game_start_time: Option<DateTime<Utc>>,
    pub full_name: String,
    pub position: Option<String>,
    pub nfl_team: Option<String>,
    pub availability: PlayerAvailability,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PlayerEnrichment {
    pub provider_player_id: String,
    pub full_name: String,
    pub position: Option<String>,
    pub nfl_team: Option<String>,
    pub injury_status: Option<String>,
    pub is_on_bye: Option<bool>,
    pub projected_points: Option<f64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LineupStatus {
    Starter,
    Bench,
    Reserve,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct PlayerAvailability {
    #[serde(default)]
    pub is_on_bye: bool,
    #[serde(default)]
    pub is_confirmed_inactive: bool,
    pub injury_status: Option<String>,
    pub projected_points: Option<f64>,
}
