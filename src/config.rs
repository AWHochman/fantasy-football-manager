use std::env;

use thiserror::Error;

use crate::Provider;

#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq)]
pub struct ManagedTeam {
    pub provider: Provider,
    pub league_id: String,
    pub team_id: String,
}

#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq)]
pub struct AppConfig {
    pub teams: Vec<ManagedTeam>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("MANAGED_TEAMS_JSON is required")]
    MissingManagedTeams,
    #[error("MANAGED_TEAMS_JSON must be a JSON array of managed teams: {0}")]
    InvalidManagedTeams(#[from] serde_json::Error),
    #[error("at least one managed team must be configured")]
    NoManagedTeams,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let teams = env::var("MANAGED_TEAMS_JSON").map_err(|_| ConfigError::MissingManagedTeams)?;
        Self::from_json(&teams)
    }

    pub fn from_json(value: &str) -> Result<Self, ConfigError> {
        let teams: Vec<ManagedTeam> = serde_json::from_str(value)?;
        if teams.is_empty() {
            return Err(ConfigError::NoManagedTeams);
        }
        Ok(Self { teams })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_managed_teams() {
        let config = AppConfig::from_json(
            r#"[
                {"provider":"Sleeper","league_id":"sleeper-league","team_id":"3"},
                {"provider":"Espn","league_id":"espn-league","team_id":"8"}
            ]"#,
        )
        .expect("valid configuration");

        assert_eq!(config.teams.len(), 2);
        assert_eq!(config.teams[0].provider, Provider::Sleeper);
    }
}
