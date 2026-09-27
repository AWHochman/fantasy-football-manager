use std::{collections::HashMap, collections::HashSet, env};

use thiserror::Error;

use crate::{
    merge_available_player_enrichment, merge_player_enrichment, AppConfig, AvailablePlayer,
    ConfigError, EspnSource, FantasySource, LeagueSnapshot, ManagedTeam, NflGameStatusSource,
    Provider, SleeperSource, SourceError,
};

const FREE_AGENTS_PER_POSITION: usize = 8;

#[derive(Clone, Debug)]
pub struct ManagedSnapshot {
    pub team: ManagedTeam,
    pub snapshot: LeagueSnapshot,
    pub available_players: Vec<AvailablePlayer>,
    /// False means game state could not be retrieved, so optimizer alerts must
    /// be skipped rather than treating every player as movable.
    pub locks_known: bool,
    /// False means the available-player pool or its projection enrichment was
    /// unavailable, so add/drop recommendations must be skipped.
    pub free_agents_known: bool,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("{0} is required for an ESPN team")]
    MissingEspnSetting(&'static str),
    #[error("an ESPN league_id in MANAGED_TEAMS_JSON must be numeric: {0}")]
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
        let (snapshot, available_players, free_agents_known) = match team.provider {
            Provider::Sleeper => {
                let source = SleeperSource::new(&team.league_id);
                let snapshot = source.fetch_league().await?;
                let (available_players, free_agents_known) =
                    match source.fetch_available_players().await {
                        Ok(players) => (players, true),
                        Err(_) => (Vec::new(), false),
                    };
                let enrichment_by_player_id = source
                    .fetch_player_enrichment(&snapshot.scoring_settings)
                    .await?;
                let snapshot = merge_player_enrichment(&snapshot, &enrichment_by_player_id);
                let available_players = limit_available_players(merge_available_player_enrichment(
                    &available_players,
                    &enrichment_by_player_id,
                ));
                (snapshot, available_players, free_agents_known)
            }
            Provider::Espn => {
                let session = espn.as_ref().expect("ESPN configuration was validated");
                let source = EspnSource::new(
                    team.league_id.parse::<u64>()?,
                    session.season,
                    &session.swid,
                    &session.espn_s2,
                );
                let snapshot = source.fetch_league().await?;
                let (available_players, free_agents_known) = match snapshot.scoring_period {
                    Some(scoring_period) => {
                        match source.fetch_available_players(scoring_period).await {
                            Ok(players) => (players, true),
                            Err(SourceError::EspnAuthenticationRequired) => {
                                return Err(RuntimeError::Source(
                                    SourceError::EspnAuthenticationRequired,
                                ));
                            }
                            Err(_) => (Vec::new(), false),
                        }
                    }
                    None => (Vec::new(), false),
                };
                let enrichment_by_player_id =
                    fetch_espn_enrichment(&source, &snapshot, &available_players).await?;
                let snapshot = merge_player_enrichment(&snapshot, &enrichment_by_player_id);
                let available_players = limit_available_players(merge_available_player_enrichment(
                    &available_players,
                    &enrichment_by_player_id,
                ));
                (snapshot, available_players, free_agents_known)
            }
        };
        snapshots.push(ManagedSnapshot {
            team: team.clone(),
            snapshot,
            available_players,
            locks_known: false,
            free_agents_known,
        });
    }

    Ok(apply_game_locks(snapshots).await)
}

async fn fetch_espn_enrichment(
    source: &EspnSource,
    snapshot: &LeagueSnapshot,
    available_players: &[AvailablePlayer],
) -> Result<HashMap<String, crate::PlayerEnrichment>, RuntimeError> {
    let scoring_period = snapshot
        .scoring_period
        .ok_or(RuntimeError::MissingEspnScoringPeriod)?;
    let mut ids = HashSet::new();
    for team in &snapshot.teams {
        for player in &team.players {
            if let Ok(id) = player.provider_player_id.parse::<i64>() {
                if id > 0 {
                    ids.insert(id);
                }
            }
        }
    }
    for player in available_players {
        if let Ok(id) = player.provider_player_id.parse::<i64>() {
            if id > 0 {
                ids.insert(id);
            }
        }
    }
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    Ok(source
        .fetch_player_enrichment(&ids.into_iter().collect::<Vec<_>>(), scoring_period)
        .await?
        .into_iter()
        .map(|enrichment| (enrichment.provider_player_id.clone(), enrichment))
        .collect())
}

async fn apply_game_locks(snapshots: Vec<ManagedSnapshot>) -> Vec<ManagedSnapshot> {
    let Ok(team_statuses) = NflGameStatusSource::new().fetch_team_statuses().await else {
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
                        .and_then(|team| team_statuses.get(team))
                        .is_some_and(|status| status.is_locked);
                    player.game_start_time = player
                        .nfl_team
                        .as_deref()
                        .and_then(|team| team_statuses.get(team))
                        .map(|status| status.game_start_time);
                }
            }
            for player in &mut managed.available_players {
                player.is_locked = player
                    .nfl_team
                    .as_deref()
                    .and_then(|team| team_statuses.get(team))
                    .is_some_and(|status| status.is_locked);
                player.game_start_time = player
                    .nfl_team
                    .as_deref()
                    .and_then(|team| team_statuses.get(team))
                    .map(|status| status.game_start_time);
            }
            managed.locks_known = true;
            managed
        })
        .collect()
}

fn limit_available_players(players: Vec<AvailablePlayer>) -> Vec<AvailablePlayer> {
    let mut by_position: HashMap<String, Vec<AvailablePlayer>> = HashMap::new();
    for player in players
        .into_iter()
        .filter(|player| player.availability.projected_points.is_some())
    {
        let position = player
            .position
            .clone()
            .unwrap_or_else(|| "OTHER".to_owned());
        by_position.entry(position).or_default().push(player);
    }
    by_position
        .into_values()
        .flat_map(|mut players| {
            players.sort_by(|left, right| {
                right
                    .availability
                    .projected_points
                    .unwrap_or_default()
                    .total_cmp(&left.availability.projected_points.unwrap_or_default())
            });
            players.into_iter().take(FREE_AGENTS_PER_POSITION)
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
