use std::error::Error;

use fantasy_football_manager::{
    evaluate_team, fetch_managed_snapshots, recommend_optimal_lineup, AlertNotifier, AlertState,
    AppConfig, EmailNotifier, MonitorAlert, MonitorHealthState, DEFAULT_MINIMUM_PROJECTED_GAIN,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let config = AppConfig::from_env()?;
    let mut health = MonitorHealthState::load();
    let snapshots = match fetch_managed_snapshots(&config).await {
        Ok(snapshots) => snapshots,
        Err(error) if error.requires_espn_session_refresh() => {
            if health.mark_espn_authentication_failed() {
                EmailNotifier::from_env()?
                    .send_espn_session_refresh_email()
                    .await?;
                health.save()?;
                println!("Sent an ESPN session refresh alert.");
            } else {
                println!("ESPN session still needs to be refreshed.");
            }
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    if health.mark_espn_authentication_recovered() {
        EmailNotifier::from_env()?
            .send_espn_session_recovered_email()
            .await?;
        health.save()?;
        println!("Sent an ESPN session recovery alert.");
    }
    let mut alerts = Vec::new();

    for managed in snapshots {
        alerts.extend(
            evaluate_team(&managed.snapshot, &managed.team.team_id)?
                .into_iter()
                .map(MonitorAlert::Starter),
        );
        if managed.locks_known {
            if let Some(recommendation) = recommend_optimal_lineup(
                &managed.snapshot,
                &managed.team.team_id,
                DEFAULT_MINIMUM_PROJECTED_GAIN,
            )? {
                alerts.push(MonitorAlert::RecommendedLineup(recommendation));
            }
        } else {
            println!(
                "Skipped projected-lineup recommendations because NFL game locks were unavailable."
            );
        }
    }

    let mut state = AlertState::load();
    let new_alerts = state.new_alerts(&alerts);
    if new_alerts.is_empty() {
        state.replace_active(&alerts);
        state.save()?;
        println!("No new actionable lineup alerts.");
        return Ok(());
    }

    EmailNotifier::from_env()?.notify(&new_alerts).await?;
    state.replace_active(&alerts);
    state.save()?;
    println!("Sent {} new lineup alert(s).", new_alerts.len());

    Ok(())
}
