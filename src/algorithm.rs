use std::collections::HashMap;

use crate::{AvailablePlayer, LeagueSnapshot, PlayerEnrichment, Provider};

/// Combines provider roster data with ESPN's weekly player enrichment.
///
/// For ESPN rosters, the roster player ID is already an ESPN player ID. For
/// Sleeper rosters, supply the Sleeper-ID-to-ESPN-ID map from Sleeper's player
/// catalog. This function is pure: API access and scheduling remain outside
/// the decision pipeline.
pub fn merge_player_enrichment(
    snapshot: &LeagueSnapshot,
    enrichment_by_espn_id: &HashMap<String, PlayerEnrichment>,
    espn_id_by_provider_player_id: &HashMap<String, String>,
) -> LeagueSnapshot {
    let mut enriched = snapshot.clone();

    for team in &mut enriched.teams {
        for player in &mut team.players {
            let espn_id = match snapshot.provider {
                Provider::Espn => Some(&player.provider_player_id),
                Provider::Sleeper => espn_id_by_provider_player_id.get(&player.provider_player_id),
            };
            let Some(enrichment) = espn_id.and_then(|id| enrichment_by_espn_id.get(id)) else {
                continue;
            };

            if let Some(is_on_bye) = enrichment.is_on_bye {
                player.availability.is_on_bye = is_on_bye;
            }
            player.availability.injury_status = enrichment
                .injury_status
                .clone()
                .or(player.availability.injury_status.clone());
            player.availability.projected_points = enrichment
                .projected_points
                .or(player.availability.projected_points);
        }
    }

    enriched
}

/// Applies the shared weekly player enrichment to a provider's available
/// player candidates. Candidates expose their ESPN ID directly, whether they
/// originated from ESPN or Sleeper.
pub fn merge_available_player_enrichment(
    players: &[AvailablePlayer],
    enrichment_by_espn_id: &HashMap<String, PlayerEnrichment>,
) -> Vec<AvailablePlayer> {
    players
        .iter()
        .cloned()
        .map(|mut player| {
            let Some(enrichment) = player
                .espn_player_id
                .as_ref()
                .and_then(|id| enrichment_by_espn_id.get(id))
            else {
                return player;
            };
            player.availability.is_on_bye = enrichment
                .is_on_bye
                .unwrap_or(player.availability.is_on_bye);
            player.availability.injury_status = enrichment
                .injury_status
                .clone()
                .or(player.availability.injury_status);
            player.availability.projected_points = enrichment
                .projected_points
                .or(player.availability.projected_points);
            player
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FantasyTeam, LineupStatus, PlayerAvailability, RosteredPlayer};

    #[test]
    fn enriches_a_sleeper_player_through_its_espn_id() {
        let snapshot = LeagueSnapshot {
            provider: Provider::Sleeper,
            league_id: "sleeper-league".to_owned(),
            league_name: "Sleeper league".to_owned(),
            scoring_period: None,
            lineup_slots: Vec::new(),
            teams: vec![FantasyTeam {
                team_id: "team".to_owned(),
                team_name: "My team".to_owned(),
                owner_id: None,
                owner_name: None,
                players: vec![RosteredPlayer {
                    provider_player_id: "sleeper-1".to_owned(),
                    espn_player_id: Some("espn-1".to_owned()),
                    eligible_positions: vec!["WR".to_owned()],
                    is_locked: false,
                    game_start_time: None,
                    full_name: "Player One".to_owned(),
                    position: Some("WR".to_owned()),
                    nfl_team: Some("BUF".to_owned()),
                    lineup_slot: "WR".to_owned(),
                    lineup_status: LineupStatus::Starter,
                    availability: PlayerAvailability::default(),
                }],
            }],
        };
        let enrichments = HashMap::from([(
            "espn-1".to_owned(),
            PlayerEnrichment {
                provider_player_id: "espn-1".to_owned(),
                full_name: "Player One".to_owned(),
                position: Some("WR".to_owned()),
                nfl_team: Some("BUF".to_owned()),
                injury_status: Some("Out".to_owned()),
                is_on_bye: Some(true),
                projected_points: Some(0.0),
            },
        )]);
        let player_ids = HashMap::from([("sleeper-1".to_owned(), "espn-1".to_owned())]);

        let result = merge_player_enrichment(&snapshot, &enrichments, &player_ids);
        let availability = &result.teams[0].players[0].availability;

        assert!(availability.is_on_bye);
        assert_eq!(availability.injury_status.as_deref(), Some("Out"));
        assert_eq!(availability.projected_points, Some(0.0));
    }

    #[test]
    fn enriches_an_available_player_through_its_espn_id() {
        let player = AvailablePlayer {
            provider_player_id: "sleeper-1".to_owned(),
            espn_player_id: Some("espn-1".to_owned()),
            eligible_positions: vec!["WR".to_owned()],
            is_locked: false,
            game_start_time: None,
            full_name: "Player One".to_owned(),
            position: Some("WR".to_owned()),
            nfl_team: Some("BUF".to_owned()),
            availability: PlayerAvailability::default(),
        };
        let enrichments = HashMap::from([(
            "espn-1".to_owned(),
            PlayerEnrichment {
                provider_player_id: "espn-1".to_owned(),
                full_name: "Player One".to_owned(),
                position: Some("WR".to_owned()),
                nfl_team: Some("BUF".to_owned()),
                injury_status: Some("Out".to_owned()),
                is_on_bye: Some(true),
                projected_points: Some(0.0),
            },
        )]);

        let result = merge_available_player_enrichment(&[player], &enrichments);
        let availability = &result[0].availability;

        assert!(availability.is_on_bye);
        assert_eq!(availability.injury_status.as_deref(), Some("Out"));
        assert_eq!(availability.projected_points, Some(0.0));
    }
}
