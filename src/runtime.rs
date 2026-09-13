use std::env;

use thiserror::Error;

use crate::{
    AppConfig, ConfigError, EspnSource, FantasySource, LeagueSnapshot, ManagedTeam, Provider,
    SleeperSource, SourceError,
};

#[derive(Clone, Debug)]
pub struct ManagedSnapshot {
    pub team: ManagedTeam,
    pub snapshot: LeagueSnapshot,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("{0} is required for an ESPN team")]
    MissingEspnSetting(&'static str),
    #[error("ESPN_LEAGUE_ID must be numeric: {0}")]
    InvalidEspnLeagueId(#[from] std::num::ParseIntError),
}

/// Retrieves every configured league while keeping API work outside the core
/// enrichment and evaluation algorithms.
pub async fn fetch_managed_snapshots(
    config: &AppConfig,
) -> Result<Vec<ManagedSnapshot>, RuntimeError> {
    let espn = EspnSession::from_env_if_needed(config)?;
    let mut snapshots = Vec::with_capacity(config.teams.len());

    for team in &config.teams {
        let snapshot = match team.provider {
            Provider::Sleeper => SleeperSource::new(&team.league_id).fetch_league().await?,
            Provider::Espn => {
                let session = espn.as_ref().expect("ESPN configuration was validated");
                EspnSource::new(
                    team.league_id.parse::<u64>()?,
                    session.season,
                    &session.swid,
                    &session.espn_s2,
                )
                .fetch_league()
                .await?
            }
        };
        snapshots.push(ManagedSnapshot {
            team: team.clone(),
            snapshot,
        });
    }

    Ok(snapshots)
}

struct EspnSession {
    season: u16,
    swid: String,
    espn_s2: String,
}

impl EspnSession {
    fn from_env_if_needed(config: &AppConfig) -> Result<Option<Self>, RuntimeError> {
        if !config
            .teams
            .iter()
            .any(|team| team.provider == Provider::Espn)
        {
            return Ok(None);
        }

        Ok(Some(Self {
            season: setting("ESPN_SEASON")?.parse::<u16>()?,
            swid: setting("ESPN_SWID")?,
            espn_s2: setting("ESPN_S2")?,
        }))
    }
}

fn setting(name: &'static str) -> Result<String, RuntimeError> {
    env::var(name).map_err(|_| RuntimeError::MissingEspnSetting(name))
}
