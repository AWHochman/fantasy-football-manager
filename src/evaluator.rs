use thiserror::Error;

use crate::{LeagueSnapshot, LineupStatus, Provider};

/// A reason a currently-started player needs the manager's attention.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum AlertReason {
    ByeWeek,
    ConfirmedUnavailable,
    ZeroProjection,
}

/// One actionable starter alert, potentially containing more than one reason.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct LineupAlert {
    pub provider: Provider,
    pub league_id: String,
    pub league_name: String,
    pub team_id: String,
    pub team_name: String,
    pub player_id: String,
    pub player_name: String,
    pub position: Option<String>,
    pub reasons: Vec<AlertReason>,
}

#[derive(Debug, Error)]
pub enum EvaluationError {
    #[error("team {team_id} was not found in {provider:?} league {league_id}")]
    TeamNotFound {
        provider: Provider,
        league_id: String,
        team_id: String,
    },
}

/// Evaluates the active starters for one managed team.
///
/// A missing projection is intentionally not treated as zero: a source may not
/// publish projections yet, and that must not create a false alert.
pub fn evaluate_team(
    snapshot: &LeagueSnapshot,
    team_id: &str,
) -> Result<Vec<LineupAlert>, EvaluationError> {
    let team = snapshot
        .teams
        .iter()
        .find(|team| team.team_id == team_id)
        .ok_or_else(|| EvaluationError::TeamNotFound {
            provider: snapshot.provider,
            league_id: snapshot.league_id.clone(),
            team_id: team_id.to_owned(),
        })?;

    Ok(team
        .players
        .iter()
        .filter(|player| player.lineup_status == LineupStatus::Starter)
        .filter_map(|player| {
            let mut reasons = Vec::new();

            if player.availability.is_on_bye {
                reasons.push(AlertReason::ByeWeek);
            }
            if player.availability.is_confirmed_inactive
                || has_confirmed_unavailability(player.availability.injury_status.as_deref())
            {
                reasons.push(AlertReason::ConfirmedUnavailable);
            }
            if player
                .availability
                .projected_points
                .is_some_and(|points| points <= 0.0)
            {
                reasons.push(AlertReason::ZeroProjection);
            }

            (!reasons.is_empty()).then(|| LineupAlert {
                provider: snapshot.provider,
                league_id: snapshot.league_id.clone(),
                league_name: snapshot.league_name.clone(),
                team_id: team.team_id.clone(),
                team_name: team.team_name.clone(),
                player_id: player.provider_player_id.clone(),
                player_name: player.full_name.clone(),
                position: player.position.clone(),
                reasons,
            })
        })
        .collect())
}

fn has_confirmed_unavailability(injury_status: Option<&str>) -> bool {
    matches!(
        injury_status.map(|status| status.trim().to_ascii_uppercase()),
        Some(status)
            if matches!(
                status.as_str(),
                "OUT" | "DOUBTFUL" | "IR" | "PUP" | "NFI" | "SUSPENDED" | "INACTIVE"
            )
    )
}

#[cfg(test)]
mod tests {
    use crate::{FantasyTeam, PlayerAvailability, RosteredPlayer};

    use super::*;

    fn player(id: &str, status: LineupStatus, availability: PlayerAvailability) -> RosteredPlayer {
        RosteredPlayer {
            provider_player_id: id.to_owned(),
            espn_player_id: None,
            full_name: format!("Player {id}"),
            position: Some("WR".to_owned()),
            nfl_team: Some("BUF".to_owned()),
            lineup_slot: "WR".to_owned(),
            lineup_status: status,
            availability,
        }
    }

    fn availability(
        is_on_bye: bool,
        injury_status: Option<&str>,
        projected_points: Option<f64>,
    ) -> PlayerAvailability {
        PlayerAvailability {
            is_on_bye,
            is_confirmed_inactive: false,
            injury_status: injury_status.map(str::to_owned),
            projected_points,
        }
    }

    fn snapshot(players: Vec<RosteredPlayer>) -> LeagueSnapshot {
        LeagueSnapshot {
            provider: Provider::Sleeper,
            league_id: "league-1".to_owned(),
            league_name: "Test league".to_owned(),
            scoring_period: None,
            teams: vec![FantasyTeam {
                team_id: "team-1".to_owned(),
                team_name: "My team".to_owned(),
                owner_id: None,
                owner_name: None,
                players,
            }],
        }
    }

    #[test]
    fn alerts_only_for_actionable_starters() {
        let alerts = evaluate_team(
            &snapshot(vec![
                player(
                    "bye",
                    LineupStatus::Starter,
                    availability(true, None, Some(8.0)),
                ),
                player(
                    "out",
                    LineupStatus::Starter,
                    availability(false, Some("Out"), Some(0.0)),
                ),
                player(
                    "bench",
                    LineupStatus::Bench,
                    availability(false, Some("IR"), Some(0.0)),
                ),
                player(
                    "unknown",
                    LineupStatus::Starter,
                    availability(false, None, None),
                ),
            ]),
            "team-1",
        )
        .expect("configured team should be present");

        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].reasons, vec![AlertReason::ByeWeek]);
        assert_eq!(
            alerts[1].reasons,
            vec![
                AlertReason::ConfirmedUnavailable,
                AlertReason::ZeroProjection
            ]
        );
    }

    #[test]
    fn returns_an_error_for_an_unknown_team() {
        let result = evaluate_team(&snapshot(Vec::new()), "missing-team");

        assert!(matches!(result, Err(EvaluationError::TeamNotFound { .. })));
    }
}
