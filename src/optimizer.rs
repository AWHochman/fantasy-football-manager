use std::collections::{HashMap, HashSet};

use thiserror::Error;

use crate::{AvailablePlayer, LeagueSnapshot, LineupSlot, LineupStatus, Provider, RosteredPlayer};

pub const DEFAULT_MINIMUM_PROJECTED_GAIN: f64 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub struct LineupAssignment {
    pub slot_name: String,
    pub player_id: String,
    pub player_name: String,
    pub projected_points: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineupRecommendation {
    pub provider: Provider,
    pub league_id: String,
    pub league_name: String,
    pub team_id: String,
    pub team_name: String,
    pub current_projected_points: f64,
    pub optimized_projected_points: f64,
    pub projected_gain: f64,
    pub assignments: Vec<LineupAssignment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FreeAgentRecommendation {
    pub lineup: LineupRecommendation,
    pub add_player_id: String,
    pub add_player_name: String,
    pub add_projected_points: f64,
    pub drop_player_id: String,
    pub drop_player_name: String,
}

#[derive(Debug, Error)]
pub enum OptimizationError {
    #[error("team {team_id} was not found in {provider:?} league {league_id}")]
    TeamNotFound {
        provider: Provider,
        league_id: String,
        team_id: String,
    },
    #[error("league {league_id} has no normalized starting lineup slots")]
    MissingLineupSlots { league_id: String },
    #[error("locked player {player_id} is assigned to unsupported slot {slot_name}")]
    UnsupportedLockedSlot {
        player_id: String,
        slot_name: String,
    },
    #[error("lineup optimization supports at most 64 movable players")]
    TooManyMovablePlayers,
}

/// Finds the best projected valid lineup while treating locked players as fixed.
/// This function is pure and operates only on the provider-neutral roster model.
pub fn recommend_optimal_lineup(
    snapshot: &LeagueSnapshot,
    team_id: &str,
    minimum_gain: f64,
) -> Result<Option<LineupRecommendation>, OptimizationError> {
    let team = snapshot
        .teams
        .iter()
        .find(|team| team.team_id == team_id)
        .ok_or_else(|| OptimizationError::TeamNotFound {
            provider: snapshot.provider,
            league_id: snapshot.league_id.clone(),
            team_id: team_id.to_owned(),
        })?;
    if snapshot.lineup_slots.is_empty() {
        return Err(OptimizationError::MissingLineupSlots {
            league_id: snapshot.league_id.clone(),
        });
    }

    let current_projected_points = team
        .players
        .iter()
        .filter(|player| player.lineup_status == LineupStatus::Starter)
        .map(|player| player.availability.projected_points)
        .collect::<Option<Vec<_>>>()
        .map(|points| points.into_iter().sum::<f64>());
    let Some(current_projected_points) = current_projected_points else {
        return Ok(None);
    };

    let mut remaining_slots = snapshot.lineup_slots.clone();
    let mut locked_assignments = Vec::new();
    for player in team.players.iter().filter(|player| player.is_locked) {
        if player.lineup_status != LineupStatus::Starter {
            continue;
        }
        let slot_index = remaining_slots
            .iter()
            .position(|slot| slot.name == player.lineup_slot)
            .ok_or_else(|| OptimizationError::UnsupportedLockedSlot {
                player_id: player.provider_player_id.clone(),
                slot_name: player.lineup_slot.clone(),
            })?;
        remaining_slots.remove(slot_index);
        let Some(projected_points) = player.availability.projected_points else {
            return Ok(None);
        };
        locked_assignments.push(LineupAssignment {
            slot_name: player.lineup_slot.clone(),
            player_id: player.provider_player_id.clone(),
            player_name: player.full_name.clone(),
            projected_points,
        });
    }

    let movable_players: Vec<_> = team
        .players
        .iter()
        .filter(|player| !player.is_locked && player.lineup_status != LineupStatus::Reserve)
        .filter(|player| player.availability.projected_points.is_some())
        .collect();
    if movable_players.len() > 64 {
        return Err(OptimizationError::TooManyMovablePlayers);
    }

    let Some(solution) = solve(
        &remaining_slots,
        &movable_players,
        0,
        0,
        &mut HashMap::new(),
    ) else {
        return Ok(None);
    };
    let mut assignments = locked_assignments;
    assignments.extend(solution.assignments.into_iter().enumerate().map(
        |(index, player_index)| {
            let player = movable_players[player_index];
            LineupAssignment {
                slot_name: remaining_slots[index].name.clone(),
                player_id: player.provider_player_id.clone(),
                player_name: player.full_name.clone(),
                projected_points: player.availability.projected_points.unwrap_or_default(),
            }
        },
    ));
    let optimized_projected_points = assignments
        .iter()
        .map(|assignment| assignment.projected_points)
        .sum();
    let projected_gain = optimized_projected_points - current_projected_points;
    let current_player_ids: HashSet<_> = team
        .players
        .iter()
        .filter(|player| player.lineup_status == LineupStatus::Starter)
        .map(|player| player.provider_player_id.as_str())
        .collect();
    let optimized_player_ids: HashSet<_> = assignments
        .iter()
        .map(|assignment| assignment.player_id.as_str())
        .collect();

    if projected_gain < minimum_gain || current_player_ids == optimized_player_ids {
        return Ok(None);
    }
    Ok(Some(LineupRecommendation {
        provider: snapshot.provider,
        league_id: snapshot.league_id.clone(),
        league_name: snapshot.league_name.clone(),
        team_id: team.team_id.clone(),
        team_name: team.team_name.clone(),
        current_projected_points,
        optimized_projected_points,
        projected_gain,
        assignments,
    }))
}

/// Finds the best valid lineup reachable with exactly one unlocked add/drop.
/// The function is pure and expects provider clients to supply only players
/// currently available in their respective league.
pub fn recommend_free_agent_lineup(
    snapshot: &LeagueSnapshot,
    team_id: &str,
    free_agents: &[AvailablePlayer],
    minimum_gain: f64,
) -> Result<Option<FreeAgentRecommendation>, OptimizationError> {
    let team = find_team(snapshot, team_id)?;
    let Some(current_projected_points) = starter_projection_total(team) else {
        return Ok(None);
    };
    if snapshot.lineup_slots.is_empty() {
        return Ok(None);
    }

    let mut best: Option<FreeAgentRecommendation> = None;
    for free_agent in free_agents
        .iter()
        .filter(|player| !player.is_locked)
        .filter(|player| player.availability.projected_points.is_some())
    {
        let mut players = team.players.clone();
        players.push(rostered_free_agent(free_agent));
        let Some(assignments) = best_assignments(snapshot, &players)? else {
            continue;
        };
        if !assignments
            .iter()
            .any(|assignment| assignment.player_id == free_agent.provider_player_id)
        {
            continue;
        }
        let selected_ids: HashSet<_> = assignments
            .iter()
            .map(|assignment| assignment.player_id.as_str())
            .collect();
        let Some(drop_player) = team
            .players
            .iter()
            .filter(|player| {
                !player.is_locked
                    && player.lineup_status != LineupStatus::Reserve
                    && !selected_ids.contains(player.provider_player_id.as_str())
            })
            .min_by(|left, right| {
                left.availability
                    .projected_points
                    .unwrap_or_default()
                    .total_cmp(&right.availability.projected_points.unwrap_or_default())
            })
        else {
            continue;
        };
        let optimized_projected_points = assignments
            .iter()
            .map(|assignment| assignment.projected_points)
            .sum();
        let projected_gain = optimized_projected_points - current_projected_points;
        if projected_gain < minimum_gain {
            continue;
        }
        let lineup = LineupRecommendation {
            provider: snapshot.provider,
            league_id: snapshot.league_id.clone(),
            league_name: snapshot.league_name.clone(),
            team_id: team.team_id.clone(),
            team_name: team.team_name.clone(),
            current_projected_points,
            optimized_projected_points,
            projected_gain,
            assignments,
        };
        let recommendation = FreeAgentRecommendation {
            lineup,
            add_player_id: free_agent.provider_player_id.clone(),
            add_player_name: free_agent.full_name.clone(),
            add_projected_points: free_agent.availability.projected_points.unwrap_or_default(),
            drop_player_id: drop_player.provider_player_id.clone(),
            drop_player_name: drop_player.full_name.clone(),
        };
        if best.as_ref().is_none_or(|current| {
            recommendation.lineup.projected_gain > current.lineup.projected_gain
        }) {
            best = Some(recommendation);
        }
    }
    Ok(best)
}

fn find_team<'a>(
    snapshot: &'a LeagueSnapshot,
    team_id: &str,
) -> Result<&'a crate::FantasyTeam, OptimizationError> {
    snapshot
        .teams
        .iter()
        .find(|team| team.team_id == team_id)
        .ok_or_else(|| OptimizationError::TeamNotFound {
            provider: snapshot.provider,
            league_id: snapshot.league_id.clone(),
            team_id: team_id.to_owned(),
        })
}

fn starter_projection_total(team: &crate::FantasyTeam) -> Option<f64> {
    team.players
        .iter()
        .filter(|player| player.lineup_status == LineupStatus::Starter)
        .map(|player| player.availability.projected_points)
        .collect::<Option<Vec<_>>>()
        .map(|points| points.into_iter().sum())
}

fn rostered_free_agent(player: &AvailablePlayer) -> RosteredPlayer {
    RosteredPlayer {
        provider_player_id: player.provider_player_id.clone(),
        espn_player_id: player.espn_player_id.clone(),
        eligible_positions: player.eligible_positions.clone(),
        is_locked: player.is_locked,
        full_name: player.full_name.clone(),
        position: player.position.clone(),
        nfl_team: player.nfl_team.clone(),
        lineup_slot: "BENCH".to_owned(),
        lineup_status: LineupStatus::Bench,
        availability: player.availability.clone(),
    }
}

fn best_assignments(
    snapshot: &LeagueSnapshot,
    players: &[RosteredPlayer],
) -> Result<Option<Vec<LineupAssignment>>, OptimizationError> {
    let mut remaining_slots = snapshot.lineup_slots.clone();
    let mut locked_assignments = Vec::new();
    for player in players.iter().filter(|player| player.is_locked) {
        if player.lineup_status != LineupStatus::Starter {
            continue;
        }
        let slot_index = remaining_slots
            .iter()
            .position(|slot| slot.name == player.lineup_slot)
            .ok_or_else(|| OptimizationError::UnsupportedLockedSlot {
                player_id: player.provider_player_id.clone(),
                slot_name: player.lineup_slot.clone(),
            })?;
        remaining_slots.remove(slot_index);
        let Some(projected_points) = player.availability.projected_points else {
            return Ok(None);
        };
        locked_assignments.push(LineupAssignment {
            slot_name: player.lineup_slot.clone(),
            player_id: player.provider_player_id.clone(),
            player_name: player.full_name.clone(),
            projected_points,
        });
    }

    let movable_players: Vec<_> = players
        .iter()
        .filter(|player| !player.is_locked && player.lineup_status != LineupStatus::Reserve)
        .filter(|player| player.availability.projected_points.is_some())
        .collect();
    if movable_players.len() > 64 {
        return Err(OptimizationError::TooManyMovablePlayers);
    }
    let Some(solution) = solve(
        &remaining_slots,
        &movable_players,
        0,
        0,
        &mut HashMap::new(),
    ) else {
        return Ok(None);
    };
    let mut assignments = locked_assignments;
    assignments.extend(solution.assignments.into_iter().enumerate().map(
        |(index, player_index)| {
            let player = movable_players[player_index];
            LineupAssignment {
                slot_name: remaining_slots[index].name.clone(),
                player_id: player.provider_player_id.clone(),
                player_name: player.full_name.clone(),
                projected_points: player.availability.projected_points.unwrap_or_default(),
            }
        },
    ));
    Ok(Some(assignments))
}

#[derive(Clone)]
struct Solution {
    projected_points: f64,
    assignments: Vec<usize>,
}

fn solve(
    slots: &[LineupSlot],
    players: &[&RosteredPlayer],
    slot_index: usize,
    selected: u64,
    memo: &mut HashMap<(usize, u64), Option<Solution>>,
) -> Option<Solution> {
    if slot_index == slots.len() {
        return Some(Solution {
            projected_points: 0.0,
            assignments: Vec::new(),
        });
    }
    if let Some(solution) = memo.get(&(slot_index, selected)) {
        return solution.clone();
    }
    let mut best: Option<Solution> = None;
    for (player_index, player) in players.iter().enumerate() {
        if selected & (1_u64 << player_index) != 0 || !is_eligible(player, &slots[slot_index]) {
            continue;
        }
        let Some(mut remainder) = solve(
            slots,
            players,
            slot_index + 1,
            selected | (1_u64 << player_index),
            memo,
        ) else {
            continue;
        };
        remainder.projected_points += player.availability.projected_points.unwrap_or_default();
        remainder.assignments.insert(0, player_index);
        if best
            .as_ref()
            .is_none_or(|best| remainder.projected_points > best.projected_points)
        {
            best = Some(remainder);
        }
    }
    memo.insert((slot_index, selected), best.clone());
    best
}

fn is_eligible(player: &RosteredPlayer, slot: &LineupSlot) -> bool {
    player
        .eligible_positions
        .iter()
        .any(|position| slot.eligible_positions.contains(position))
}

#[cfg(test)]
mod tests {
    use crate::{AvailablePlayer, FantasyTeam, LineupSlot, PlayerAvailability};

    use super::*;

    fn player(
        id: &str,
        position: &str,
        slot: &str,
        status: LineupStatus,
        projection: f64,
    ) -> RosteredPlayer {
        RosteredPlayer {
            provider_player_id: id.to_owned(),
            espn_player_id: None,
            eligible_positions: vec![position.to_owned()],
            is_locked: false,
            full_name: id.to_owned(),
            position: Some(position.to_owned()),
            nfl_team: None,
            lineup_slot: slot.to_owned(),
            lineup_status: status,
            availability: PlayerAvailability {
                projected_points: Some(projection),
                ..Default::default()
            },
        }
    }

    fn snapshot(players: Vec<RosteredPlayer>) -> LeagueSnapshot {
        LeagueSnapshot {
            provider: Provider::Sleeper,
            league_id: "league".to_owned(),
            league_name: "League".to_owned(),
            scoring_period: None,
            lineup_slots: vec![
                LineupSlot {
                    name: "RB".to_owned(),
                    eligible_positions: vec!["RB".to_owned()],
                },
                LineupSlot {
                    name: "FLEX".to_owned(),
                    eligible_positions: vec!["RB".to_owned(), "WR".to_owned()],
                },
            ],
            teams: vec![FantasyTeam {
                team_id: "team".to_owned(),
                team_name: "Team".to_owned(),
                owner_id: None,
                owner_name: None,
                players,
            }],
        }
    }

    fn free_agent(id: &str, position: &str, projection: f64) -> AvailablePlayer {
        AvailablePlayer {
            provider_player_id: id.to_owned(),
            espn_player_id: None,
            eligible_positions: vec![position.to_owned()],
            is_locked: false,
            full_name: id.to_owned(),
            position: Some(position.to_owned()),
            nfl_team: None,
            availability: PlayerAvailability {
                projected_points: Some(projection),
                ..Default::default()
            },
        }
    }

    #[test]
    fn finds_a_better_flex_configuration() {
        let recommendation = recommend_optimal_lineup(
            &snapshot(vec![
                player("rb-low", "RB", "RB", LineupStatus::Starter, 8.0),
                player("wr-low", "WR", "FLEX", LineupStatus::Starter, 7.0),
                player("rb-high", "RB", "BENCH", LineupStatus::Bench, 16.0),
            ]),
            "team",
            DEFAULT_MINIMUM_PROJECTED_GAIN,
        )
        .expect("valid roster")
        .expect("better lineup");
        assert_eq!(recommendation.projected_gain, 9.0);
        assert!(recommendation
            .assignments
            .iter()
            .any(|assignment| assignment.player_id == "rb-high"));
    }

    #[test]
    fn keeps_locked_starters_in_their_slots() {
        let mut locked = player("rb-locked", "RB", "RB", LineupStatus::Starter, 8.0);
        locked.is_locked = true;
        let recommendation = recommend_optimal_lineup(
            &snapshot(vec![
                locked,
                player("wr-low", "WR", "FLEX", LineupStatus::Starter, 7.0),
                player("rb-high", "RB", "BENCH", LineupStatus::Bench, 16.0),
                player("wr-high", "WR", "BENCH", LineupStatus::Bench, 12.0),
            ]),
            "team",
            DEFAULT_MINIMUM_PROJECTED_GAIN,
        )
        .expect("valid roster")
        .expect("better lineup");
        assert!(recommendation
            .assignments
            .iter()
            .any(|assignment| assignment.player_id == "rb-locked" && assignment.slot_name == "RB"));
        assert!(recommendation
            .assignments
            .iter()
            .any(|assignment| assignment.player_id == "rb-high" && assignment.slot_name == "FLEX"));
    }

    #[test]
    fn recommends_an_unlocked_add_drop_that_improves_the_lineup() {
        let recommendation = recommend_free_agent_lineup(
            &snapshot(vec![
                player("rb-low", "RB", "RB", LineupStatus::Starter, 8.0),
                player("wr-low", "WR", "FLEX", LineupStatus::Starter, 7.0),
                player("bench", "WR", "BENCH", LineupStatus::Bench, 4.0),
            ]),
            "team",
            &[free_agent("wr-free-agent", "WR", 16.0)],
            DEFAULT_MINIMUM_PROJECTED_GAIN,
        )
        .expect("valid roster")
        .expect("better add/drop lineup");

        assert_eq!(recommendation.add_player_id, "wr-free-agent");
        assert_eq!(recommendation.drop_player_id, "bench");
        assert_eq!(recommendation.lineup.projected_gain, 9.0);
    }

    #[test]
    fn does_not_recommend_dropping_a_locked_player() {
        let mut locked_bench = player("locked-bench", "WR", "BENCH", LineupStatus::Bench, 1.0);
        locked_bench.is_locked = true;
        let recommendation = recommend_free_agent_lineup(
            &snapshot(vec![
                player("rb-low", "RB", "RB", LineupStatus::Starter, 8.0),
                player("wr-low", "WR", "FLEX", LineupStatus::Starter, 7.0),
                locked_bench,
            ]),
            "team",
            &[free_agent("wr-free-agent", "WR", 16.0)],
            DEFAULT_MINIMUM_PROJECTED_GAIN,
        )
        .expect("valid roster")
        .expect("better add/drop lineup");

        assert_ne!(recommendation.drop_player_id, "locked-bench");
    }
}
