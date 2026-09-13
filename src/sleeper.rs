use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Deserializer};

use crate::{
    FantasySource, FantasyTeam, LeagueSnapshot, LineupStatus, PlayerAvailability, Provider,
    RosteredPlayer, SourceError,
};

const SLEEPER_API_BASE: &str = "https://api.sleeper.app/v1";

/// Read-only client for one Sleeper NFL league.
pub struct SleeperSource {
    http: Client,
    league_id: String,
}

impl SleeperSource {
    pub fn new(league_id: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            league_id: league_id.into(),
        }
    }

    pub fn with_http_client(league_id: impl Into<String>, http: Client) -> Self {
        Self {
            http,
            league_id: league_id.into(),
        }
    }
}

#[async_trait]
impl FantasySource for SleeperSource {
    fn provider(&self) -> Provider {
        Provider::Sleeper
    }

    async fn fetch_league(&self) -> Result<LeagueSnapshot, SourceError> {
        let league_url = format!("{SLEEPER_API_BASE}/league/{}", self.league_id);
        let users_url = format!("{SLEEPER_API_BASE}/league/{}/users", self.league_id);
        let rosters_url = format!("{SLEEPER_API_BASE}/league/{}/rosters", self.league_id);
        let players_url = format!("{SLEEPER_API_BASE}/players/nfl");

        let (league, users, rosters, players) = tokio::try_join!(
            get_json::<SleeperLeague>(&self.http, &league_url),
            get_json::<Vec<SleeperUser>>(&self.http, &users_url),
            get_json::<Vec<SleeperRoster>>(&self.http, &rosters_url),
            get_json::<HashMap<String, SleeperPlayer>>(&self.http, &players_url),
        )?;

        Ok(normalize(&self.league_id, league, users, rosters, players))
    }
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    http: &Client,
    url: &str,
) -> Result<T, SourceError> {
    Ok(http
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

fn normalize(
    league_id: &str,
    league: SleeperLeague,
    users: Vec<SleeperUser>,
    rosters: Vec<SleeperRoster>,
    players: HashMap<String, SleeperPlayer>,
) -> LeagueSnapshot {
    let owners: HashMap<_, _> = users
        .into_iter()
        .map(|user| {
            let name = user
                .metadata
                .and_then(|metadata| metadata.team_name)
                .or(user.display_name)
                .or(user.username);
            (user.user_id, name)
        })
        .collect();

    let teams = rosters
        .into_iter()
        .map(|roster| {
            let starters: std::collections::HashSet<String> = roster.starters.into_iter().collect();
            let reserves: std::collections::HashSet<String> = roster.reserve.into_iter().collect();
            let owner_name = roster
                .owner_id
                .as_ref()
                .and_then(|owner_id| owners.get(owner_id))
                .cloned()
                .flatten();
            let team_name = owner_name
                .clone()
                .unwrap_or_else(|| format!("Sleeper team {}", roster.roster_id));

            let players = roster
                .players
                .into_iter()
                .map(|player_id| {
                    let player = players.get(&player_id);
                    let lineup_status = if starters.contains(&player_id) {
                        LineupStatus::Starter
                    } else if reserves.contains(&player_id) {
                        LineupStatus::Reserve
                    } else {
                        LineupStatus::Bench
                    };

                    RosteredPlayer {
                        provider_player_id: player_id,
                        full_name: player
                            .and_then(|value| value.full_name.clone())
                            .unwrap_or_else(|| "Unknown player".to_owned()),
                        position: player.and_then(|value| value.position.clone()),
                        nfl_team: player.and_then(|value| value.team.clone()),
                        lineup_slot: match lineup_status {
                            LineupStatus::Starter => "STARTER".to_owned(),
                            LineupStatus::Bench => "BENCH".to_owned(),
                            LineupStatus::Reserve => "RESERVE".to_owned(),
                        },
                        lineup_status,
                        availability: PlayerAvailability {
                            is_on_bye: false,
                            is_confirmed_inactive: false,
                            injury_status: player.and_then(|value| value.injury_status.clone()),
                            projected_points: None,
                        },
                    }
                })
                .collect();

            FantasyTeam {
                team_id: roster.roster_id.to_string(),
                team_name,
                owner_id: roster.owner_id,
                owner_name,
                players,
            }
        })
        .collect();

    LeagueSnapshot {
        provider: Provider::Sleeper,
        league_id: league_id.to_owned(),
        league_name: league
            .name
            .unwrap_or_else(|| format!("Sleeper league {league_id}")),
        teams,
    }
}

#[derive(Deserialize)]
struct SleeperLeague {
    name: Option<String>,
}

#[derive(Deserialize)]
struct SleeperUser {
    user_id: String,
    display_name: Option<String>,
    username: Option<String>,
    metadata: Option<SleeperUserMetadata>,
}

#[derive(Deserialize)]
struct SleeperUserMetadata {
    team_name: Option<String>,
}

#[derive(Deserialize)]
struct SleeperRoster {
    roster_id: u64,
    owner_id: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    players: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    starters: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    reserve: Vec<String>,
}

#[derive(Deserialize)]
struct SleeperPlayer {
    full_name: Option<String>,
    position: Option<String>,
    team: Option<String>,
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
    fn normalizes_sleeper_roster_and_lineup_status() {
        let snapshot = normalize(
            "league-1",
            SleeperLeague {
                name: Some("Sunday league".to_owned()),
            },
            vec![SleeperUser {
                user_id: "owner-1".to_owned(),
                display_name: Some("Austin".to_owned()),
                username: None,
                metadata: Some(SleeperUserMetadata {
                    team_name: Some("Team Austin".to_owned()),
                }),
            }],
            vec![SleeperRoster {
                roster_id: 7,
                owner_id: Some("owner-1".to_owned()),
                players: vec!["p1".to_owned(), "p2".to_owned()],
                starters: vec!["p1".to_owned()],
                reserve: vec!["p2".to_owned()],
            }],
            HashMap::from([
                (
                    "p1".to_owned(),
                    SleeperPlayer {
                        full_name: Some("Starter Player".to_owned()),
                        position: Some("WR".to_owned()),
                        team: Some("NYJ".to_owned()),
                        injury_status: None,
                    },
                ),
                (
                    "p2".to_owned(),
                    SleeperPlayer {
                        full_name: Some("Reserve Player".to_owned()),
                        position: Some("RB".to_owned()),
                        team: Some("NE".to_owned()),
                        injury_status: Some("Questionable".to_owned()),
                    },
                ),
            ]),
        );

        assert_eq!(snapshot.league_name, "Sunday league");
        assert_eq!(snapshot.teams[0].owner_name.as_deref(), Some("Team Austin"));
        assert_eq!(
            snapshot.teams[0].players[0].lineup_status,
            LineupStatus::Starter
        );
        assert_eq!(
            snapshot.teams[0].players[1].lineup_status,
            LineupStatus::Reserve
        );
    }

    #[test]
    fn accepts_null_roster_collections() {
        let roster: SleeperRoster = serde_json::from_str(
            r#"{"roster_id": 1, "owner_id": null, "players": null, "starters": null, "reserve": null}"#,
        )
        .expect("Sleeper may return null instead of an empty roster collection");

        assert!(roster.players.is_empty());
        assert!(roster.starters.is_empty());
        assert!(roster.reserve.is_empty());
    }
}
