use std::{collections::HashSet, env, fs, path::PathBuf};

use thiserror::Error;

use crate::{AlertReason, MonitorAlert, Provider};

#[derive(Debug, Error)]
pub enum AlertStateError {
    #[error("could not serialize local alert state: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("could not write local alert state: {0}")]
    Io(#[from] std::io::Error),
}

/// Local memory of active alerts, used to avoid repeating identical emails.
pub struct AlertState {
    path: PathBuf,
    active: HashSet<AlertFingerprint>,
}

/// Local memory of monitor health notifications, used to avoid repeating the
/// same configuration warning on every scheduled run.
pub struct MonitorHealthState {
    path: PathBuf,
    data: MonitorHealthData,
}

impl MonitorHealthState {
    pub fn load() -> Self {
        let path = cache_file("monitor_health.json");
        let data = fs::read(&path)
            .ok()
            .and_then(|contents| serde_json::from_slice(&contents).ok())
            .unwrap_or_default();
        Self { path, data }
    }

    /// Returns true only when this is a newly observed authentication failure.
    pub fn mark_espn_authentication_failed(&mut self) -> bool {
        if self.data.espn_authentication_failure_active {
            return false;
        }
        self.data.espn_authentication_failure_active = true;
        true
    }

    /// Returns true only when a previous authentication failure has recovered.
    pub fn mark_espn_authentication_recovered(&mut self) -> bool {
        if !self.data.espn_authentication_failure_active {
            return false;
        }
        self.data.espn_authentication_failure_active = false;
        true
    }

    pub fn save(&self) -> Result<(), AlertStateError> {
        save_json(&self.path, &self.data)
    }
}

impl AlertState {
    pub fn load() -> Self {
        let path = default_state_path();
        let active = fs::read(&path)
            .ok()
            .and_then(|contents| serde_json::from_slice(&contents).ok())
            .unwrap_or_default();
        Self { path, active }
    }

    pub fn new_alerts(&self, alerts: &[MonitorAlert]) -> Vec<MonitorAlert> {
        alerts
            .iter()
            .filter(|alert| !self.active.contains(&AlertFingerprint::from(*alert)))
            .cloned()
            .collect()
    }

    pub fn replace_active(&mut self, alerts: &[MonitorAlert]) {
        self.active = alerts.iter().map(AlertFingerprint::from).collect();
    }

    pub fn save(&self) -> Result<(), AlertStateError> {
        save_json(&self.path, &self.active)
    }
}

fn default_state_path() -> PathBuf {
    cache_file("active_alerts.json")
}

fn cache_file(file_name: &str) -> PathBuf {
    if let Some(path) = env::var_os("FANTASY_FOOTBALL_CACHE_DIR") {
        return PathBuf::from(path).join(file_name);
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(home_cache_directory)
        .unwrap_or_else(env::temp_dir)
        .join("fantasy-football-manager")
        .join(file_name)
}

fn save_json<T: serde::Serialize>(path: &PathBuf, value: &T) -> Result<(), AlertStateError> {
    let contents = serde_json::to_vec(value)?;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "state path has no parent")
    })?;
    fs::create_dir_all(parent)?;

    let temporary_path = path.with_extension("tmp");
    fs::write(&temporary_path, contents)?;
    fs::rename(temporary_path, path)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn home_cache_directory(home: PathBuf) -> PathBuf {
    home.join("Library").join("Caches")
}

#[cfg(not(target_os = "macos"))]
fn home_cache_directory(home: PathBuf) -> PathBuf {
    home.join(".cache")
}

impl From<&MonitorAlert> for AlertFingerprint {
    fn from(alert: &MonitorAlert) -> Self {
        match alert {
            MonitorAlert::Starter(alert) => Self::Starter {
                provider: alert.provider,
                league_id: alert.league_id.clone(),
                team_id: alert.team_id.clone(),
                player_id: alert.player_id.clone(),
                reasons: alert.reasons.clone(),
            },
            MonitorAlert::RecommendedLineup(recommendation) => Self::RecommendedLineup {
                provider: recommendation.provider,
                league_id: recommendation.league_id.clone(),
                team_id: recommendation.team_id.clone(),
                current_projected_points: recommendation.current_projected_points.to_bits(),
                optimized_projected_points: recommendation.optimized_projected_points.to_bits(),
                assignments: recommendation
                    .assignments
                    .iter()
                    .map(|assignment| (assignment.slot_name.clone(), assignment.player_id.clone()))
                    .collect(),
            },
            MonitorAlert::RecommendedFreeAgentLineup(recommendation) => {
                let lineup = &recommendation.lineup;
                Self::RecommendedFreeAgentLineup {
                    provider: lineup.provider,
                    league_id: lineup.league_id.clone(),
                    team_id: lineup.team_id.clone(),
                    current_projected_points: lineup.current_projected_points.to_bits(),
                    optimized_projected_points: lineup.optimized_projected_points.to_bits(),
                    add_player_id: recommendation.add_player_id.clone(),
                    drop_player_id: recommendation.drop_player_id.clone(),
                    assignments: lineup
                        .assignments
                        .iter()
                        .map(|assignment| {
                            (assignment.slot_name.clone(), assignment.player_id.clone())
                        })
                        .collect(),
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
enum AlertFingerprint {
    Starter {
        provider: Provider,
        league_id: String,
        team_id: String,
        player_id: String,
        reasons: Vec<AlertReason>,
    },
    RecommendedLineup {
        provider: Provider,
        league_id: String,
        team_id: String,
        current_projected_points: u64,
        optimized_projected_points: u64,
        assignments: Vec<(String, String)>,
    },
    RecommendedFreeAgentLineup {
        provider: Provider,
        league_id: String,
        team_id: String,
        current_projected_points: u64,
        optimized_projected_points: u64,
        add_player_id: String,
        drop_player_id: String,
        assignments: Vec<(String, String)>,
    },
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct MonitorHealthData {
    #[serde(default)]
    espn_authentication_failure_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LineupAlert, LineupAssignment, LineupRecommendation};

    fn alert(reasons: Vec<AlertReason>) -> MonitorAlert {
        MonitorAlert::Starter(LineupAlert {
            provider: Provider::Sleeper,
            league_id: "league".to_owned(),
            league_name: "League".to_owned(),
            team_id: "team".to_owned(),
            team_name: "Team".to_owned(),
            player_id: "player".to_owned(),
            player_name: "Player".to_owned(),
            position: Some("WR".to_owned()),
            reasons,
        })
    }

    fn recommendation(projected_gain: f64) -> MonitorAlert {
        MonitorAlert::RecommendedLineup(LineupRecommendation {
            provider: Provider::Sleeper,
            league_id: "league".to_owned(),
            league_name: "League".to_owned(),
            team_id: "team".to_owned(),
            team_name: "Team".to_owned(),
            current_projected_points: 100.0,
            optimized_projected_points: 100.0 + projected_gain,
            projected_gain,
            action_by: None,
            assignments: vec![LineupAssignment {
                slot_name: "WR".to_owned(),
                player_id: "player".to_owned(),
                player_name: "Player".to_owned(),
                projected_points: 15.0,
            }],
        })
    }

    #[test]
    fn returns_only_new_or_changed_alerts() {
        let current = alert(vec![AlertReason::ZeroProjection]);
        let mut state = AlertState {
            path: PathBuf::new(),
            active: HashSet::new(),
        };

        assert_eq!(state.new_alerts(std::slice::from_ref(&current)).len(), 1);
        state.replace_active(std::slice::from_ref(&current));
        assert!(state.new_alerts(std::slice::from_ref(&current)).is_empty());

        let changed = alert(vec![AlertReason::ConfirmedUnavailable]);
        assert_eq!(state.new_alerts(&[changed]).len(), 1);
    }

    #[test]
    fn resolved_alerts_can_alert_again_later() {
        let current = alert(vec![AlertReason::ZeroProjection]);
        let mut state = AlertState {
            path: PathBuf::new(),
            active: HashSet::from([AlertFingerprint::from(&current)]),
        };

        state.replace_active(&[]);
        assert_eq!(state.new_alerts(&[current]).len(), 1);
    }

    #[test]
    fn returns_a_recommendation_again_when_its_projections_change() {
        let current = recommendation(2.0);
        let state = AlertState {
            path: PathBuf::new(),
            active: HashSet::from([AlertFingerprint::from(&current)]),
        };

        assert!(state.new_alerts(std::slice::from_ref(&current)).is_empty());
        assert_eq!(state.new_alerts(&[recommendation(3.0)]).len(), 1);
    }

    #[test]
    fn reports_health_transitions_once() {
        let mut state = MonitorHealthState {
            path: PathBuf::new(),
            data: MonitorHealthData::default(),
        };

        assert!(state.mark_espn_authentication_failed());
        assert!(!state.mark_espn_authentication_failed());
        assert!(state.mark_espn_authentication_recovered());
        assert!(!state.mark_espn_authentication_recovered());
    }
}
