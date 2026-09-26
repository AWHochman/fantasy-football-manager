use std::{collections::HashMap, collections::HashSet, env};

use thiserror::Error;

use crate::{
    merge_player_enrichment, AppConfig, ConfigError, EspnSource, FantasySource, LeagueSnapshot,
    ManagedTeam, NflGameStatusSource, PlayerEnrichment, Provider, SleeperSource, SourceError,
};

#[derive(Clone, Debug)]
pub struct ManagedSnapshot {
    pub team: ManagedTeam,
    pub snapshot: LeagueSnapshot,
    /// False means game state could not be retrieved, so optimizer alerts must
    /// be skipped rather than treating every player as movable.
    pub locks_known: bool,
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
    #[error("ESPN did not provide a current scoring period")]
    MissingEspnScoringPeriod,
}

impl RuntimeError {
    pub fn requires_espn_session_refresh(&self) -> bool {
        matches!(self, Self::Source(SourceError::EspnAuthenticationRequired))
    }
}

/// Retrieves and enriches every configured league while keeping API work
/// outside the core enrichment and evaluation algorithms.
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
            locks_known: false,
        });
    }

    let snapshots = enrich_snapshots(snapshots, espn.as_ref()).await?;
    Ok(apply_game_locks(snapshots).await)
}

async fn enrich_snapshots(
    snapshots: Vec<ManagedSnapshot>,
    espn: Option<&EspnSession>,
) -> Result<Vec<ManagedSnapshot>, RuntimeError> {
    let Some(session) = espn else {
        return Ok(snapshots);
    };
    let Some(reference_league) = snapshots
        .iter()
        .find(|managed| managed.team.provider == Provider::Espn)
    else {
        return Ok(snapshots);
    };
    let scoring_period = reference_league
        .snapshot
        .scoring_period
        .ok_or(RuntimeError::MissingEspnScoringPeriod)?;

    let mut ids = HashSet::new();
    let mut sleeper_to_espn = HashMap::new();
    for managed in &snapshots {
        for team in &managed.snapshot.teams {
            for player in &team.players {
                let espn_id = player.espn_player_id.as_ref();
                if let Some(espn_id) = espn_id {
                    if let Ok(id) = espn_id.parse::<i64>() {
                        if id > 0 {
                            ids.insert(id);
                        }
                    }
                    if managed.team.provider == Provider::Sleeper {
                        sleeper_to_espn.insert(player.provider_player_id.clone(), espn_id.clone());
                    }
                }
            }
        }
    }
    if ids.is_empty() {
        return Ok(snapshots);
    }

    let source = EspnSource::new(
        reference_league.team.league_id.parse::<u64>()?,
        session.season,
        &session.swid,
        &session.espn_s2,
    );
    let enrichment_by_espn_id: HashMap<String, PlayerEnrichment> = source
        .fetch_player_enrichment(&ids.into_iter().collect::<Vec<_>>(), scoring_period)
        .await?
        .into_iter()
        .map(|enrichment| (enrichment.provider_player_id.clone(), enrichment))
        .collect();

    Ok(snapshots
        .into_iter()
        .map(|managed| ManagedSnapshot {
            snapshot: merge_player_enrichment(
                &managed.snapshot,
                &enrichment_by_espn_id,
                &sleeper_to_espn,
            ),
            team: managed.team,
            locks_known: managed.locks_known,
        })
        .collect())
}

async fn apply_game_locks(snapshots: Vec<ManagedSnapshot>) -> Vec<ManagedSnapshot> {
    let Ok(locked_teams) = NflGameStatusSource::new().fetch_locked_teams().await else {
        return snapshots;
    };

    snapshots
        .into_iter()
        .map(|mut managed| {
            for team in &mut managed.snapshot.teams {
                for player in &mut team.players {
                    player.is_locked = player
                        .nfl_team
                        .as_deref()
                        .is_some_and(|team| locked_teams.contains(team));
                }
            }
            managed.locks_known = true;
            managed
        })
        .collect()
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
