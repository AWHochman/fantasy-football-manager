use std::collections::HashSet;

use reqwest::Client;

use crate::SourceError;

const NFL_SCOREBOARD_URL: &str = "https://cdn.espn.com/core/nfl/scoreboard?xhr=1";

/// Read-only source of NFL game state, shared by every fantasy provider.
pub struct NflGameStatusSource {
    http: Client,
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

    /// Returns NFL team abbreviations whose games have begun or finished.
    pub async fn fetch_locked_teams(&self) -> Result<HashSet<String>, SourceError> {
        let payload = self
            .http
            .get(NFL_SCOREBOARD_URL)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(locked_teams_from_payload(&payload))
    }
}

fn locked_teams_from_payload(payload: &serde_json::Value) -> HashSet<String> {
    payload["content"]["sbData"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| {
            matches!(
                event["status"]["type"]["state"].as_str(),
                Some("in") | Some("post")
            )
        })
        .flat_map(|event| {
            event["competitions"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|competition| {
                    competition["competitors"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|competitor| {
                            competitor["team"]["abbreviation"]
                                .as_str()
                                .map(str::to_owned)
                        })
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::locked_teams_from_payload;

    #[test]
    fn locks_only_teams_with_started_games() {
        let payload = serde_json::json!({
            "content": {
                "sbData": {
                    "events": [
                        {
                            "status": {"type": {"state": "pre"}},
                            "competitions": [{"competitors": [
                                {"team": {"abbreviation": "BUF"}},
                                {"team": {"abbreviation": "MIA"}}
                            ]}]
                        },
                        {
                            "status": {"type": {"state": "in"}},
                            "competitions": [{"competitors": [
                                {"team": {"abbreviation": "KC"}},
                                {"team": {"abbreviation": "DEN"}}
                            ]}]
                        },
                        {
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

        assert_eq!(
            locked_teams_from_payload(&payload),
            HashSet::from([
                "KC".to_owned(),
                "DEN".to_owned(),
                "DAL".to_owned(),
                "PHI".to_owned(),
            ])
        );
    }
}
