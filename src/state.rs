use std::{collections::HashSet, env, fs, path::PathBuf};

use thiserror::Error;

use crate::{AlertReason, LineupAlert, Provider};

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

impl AlertState {
    pub fn load() -> Self {
        let path = default_state_path();
        let active = fs::read(&path)
            .ok()
            .and_then(|contents| serde_json::from_slice(&contents).ok())
            .unwrap_or_default();
        Self { path, active }
    }

    pub fn new_alerts(&self, alerts: &[LineupAlert]) -> Vec<LineupAlert> {
        alerts
            .iter()
            .filter(|alert| !self.active.contains(&AlertFingerprint::from(*alert)))
            .cloned()
            .collect()
    }

    pub fn replace_active(&mut self, alerts: &[LineupAlert]) {
        self.active = alerts.iter().map(AlertFingerprint::from).collect();
    }

    pub fn save(&self) -> Result<(), AlertStateError> {
        let contents = serde_json::to_vec(&self.active)?;
        let parent = self.path.parent().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "state path has no parent")
        })?;
        fs::create_dir_all(parent)?;

        let temporary_path = self.path.with_extension("tmp");
        fs::write(&temporary_path, contents)?;
        fs::rename(temporary_path, &self.path)?;
        Ok(())
    }
}

fn default_state_path() -> PathBuf {
    if let Some(path) = env::var_os("FANTASY_FOOTBALL_CACHE_DIR") {
        return PathBuf::from(path).join("active_alerts.json");
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(home_cache_directory)
        .unwrap_or_else(env::temp_dir)
        .join("fantasy-football-manager")
        .join("active_alerts.json")
}

#[cfg(target_os = "macos")]
fn home_cache_directory(home: PathBuf) -> PathBuf {
    home.join("Library").join("Caches")
}

#[cfg(not(target_os = "macos"))]
fn home_cache_directory(home: PathBuf) -> PathBuf {
    home.join(".cache")
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
struct AlertFingerprint {
    provider: Provider,
    league_id: String,
    team_id: String,
    player_id: String,
    reasons: Vec<AlertReason>,
}

impl From<&LineupAlert> for AlertFingerprint {
    fn from(alert: &LineupAlert) -> Self {
        Self {
            provider: alert.provider,
            league_id: alert.league_id.clone(),
            team_id: alert.team_id.clone(),
            player_id: alert.player_id.clone(),
            reasons: alert.reasons.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(reasons: Vec<AlertReason>) -> LineupAlert {
        LineupAlert {
            provider: Provider::Sleeper,
            league_id: "league".to_owned(),
            league_name: "League".to_owned(),
            team_id: "team".to_owned(),
            team_name: "Team".to_owned(),
            player_id: "player".to_owned(),
            player_name: "Player".to_owned(),
            position: Some("WR".to_owned()),
            reasons,
        }
    }

    #[test]
    fn returns_only_new_or_changed_alerts() {
        let current = alert(vec![AlertReason::ZeroProjection]);
        let mut state = AlertState {
            path: PathBuf::new(),
            active: HashSet::new(),
        };

        assert_eq!(state.new_alerts(&[current.clone()]).len(), 1);
        state.replace_active(&[current.clone()]);
        assert!(state.new_alerts(&[current.clone()]).is_empty());

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
}
