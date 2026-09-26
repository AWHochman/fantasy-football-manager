use std::collections::HashMap;

use chrono::{DateTime, NaiveDateTime, Utc};
use reqwest::Client;

use crate::SourceError;

const NFL_SCOREBOARD_URL: &str = "https://cdn.espn.com/core/nfl/scoreboard?xhr=1";

/// Read-only source of NFL game state, shared by every fantasy provider.
pub struct NflGameStatusSource {
    http: Client,
}

#[derive(Clone, Debug)]
pub struct NflTeamGameStatus {
    pub game_start_time: DateTime<Utc>,
    pub is_locked: bool,
}

impl Default for NflGameStatusSource {
    fn default() -> Self {
        Self::new()
    }
}

impl NflGameStatusSource {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
        }
    }

    pub fn with_http_client(http: Client) -> Self {
        Self { http }
    }

    /// Returns game start and lock state by NFL team abbreviation.
    pub async fn fetch_team_statuses(
        &self,
    ) -> Result<HashMap<String, NflTeamGameStatus>, SourceError> {
        let payload = self
            .http
            .get(NFL_SCOREBOARD_URL)
            // ESPN's core scoreboard returns the full game week for a date,
            // while an undated request can contain only games playing today.
            .query(&[("dates", Utc::now().format("%Y%m%d").to_string())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(team_statuses_from_payload(&payload))
    }
}

fn team_statuses_from_payload(payload: &serde_json::Value) -> HashMap<String, NflTeamGameStatus> {
    payload["content"]["sbData"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|event| {
            let game_start_time = event["date"].as_str().and_then(parse_game_start_time)?;
            let is_locked = matches!(
                event["status"]["type"]["state"].as_str(),
                Some("in") | Some("post")
            );
            Some((event, game_start_time, is_locked))
        })
        .flat_map(|(event, game_start_time, is_locked)| {
            event["competitions"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(move |competition| {
                    competition["competitors"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(move |competitor| {
                            competitor["team"]["abbreviation"].as_str().map(|team| {
                                (
                                    team.to_owned(),
                                    NflTeamGameStatus {
                                        game_start_time,
                                        is_locked,
                                    },
                                )
                            })
                        })
                })
        })
        .collect()
}

fn parse_game_start_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Utc))
        .or_else(|_| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%MZ").map(|date| date.and_utc())
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::team_statuses_from_payload;

    #[test]
    fn locks_only_teams_with_started_games() {
        let payload = serde_json::json!({
            "content": {
                "sbData": {
                    "events": [
                        {
                            "date": "2026-09-27T17:00Z",
                            "status": {"type": {"state": "pre"}},
                            "competitions": [{"competitors": [
                                {"team": {"abbreviation": "BUF"}},
                                {"team": {"abbreviation": "MIA"}}
                            ]}]
                        },
                        {
                            "date": "2026-09-27T20:25:00Z",
                            "status": {"type": {"state": "in"}},
                            "competitions": [{"competitors": [
                                {"team": {"abbreviation": "KC"}},
                                {"team": {"abbreviation": "DEN"}}
                            ]}]
                        },
                        {
                            "date": "2026-09-28T00:20:00Z",
                            "status": {"type": {"state": "post"}},
                            "competitions": [{"competitors": [
                                {"team": {"abbreviation": "DAL"}},
                                {"team": {"abbreviation": "PHI"}}
                            ]}]
                        }
                    ]
                }
            }
        });

        let statuses = team_statuses_from_payload(&payload);
        assert!(!statuses["BUF"].is_locked);
        assert!(statuses["KC"].is_locked);
        assert!(statuses["PHI"].is_locked);
        assert_eq!(
            statuses["BUF"].game_start_time,
            Utc.with_ymd_and_hms(2026, 9, 27, 17, 0, 0).unwrap()
        );
    }
}
