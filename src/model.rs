use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Provider {
    Espn,
    Sleeper,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LeagueSnapshot {
    pub provider: Provider,
    pub league_id: String,
    pub league_name: String,
    pub teams: Vec<FantasyTeam>,
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
    pub full_name: String,
    pub position: Option<String>,
    pub nfl_team: Option<String>,
    pub lineup_slot: String,
    pub lineup_status: LineupStatus,
    pub availability: PlayerAvailability,
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
