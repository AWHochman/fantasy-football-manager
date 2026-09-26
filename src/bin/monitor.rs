use std::error::Error;

use fantasy_football_manager::{
    evaluate_team, fetch_managed_snapshots, AlertNotifier, AlertState, AppConfig, EmailNotifier,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let config = AppConfig::from_env()?;
    let snapshots = fetch_managed_snapshots(&config).await?;
    let mut alerts = Vec::new();

    for managed in snapshots {
        alerts.extend(evaluate_team(&managed.snapshot, &managed.team.team_id)?);
    }

    let mut state = AlertState::load();
    let new_alerts = state.new_alerts(&alerts);
    if new_alerts.is_empty() {
        state.replace_active(&alerts);
        state.save()?;
        println!("No new actionable starter alerts.");
        return Ok(());
    }

    EmailNotifier::from_env()?.notify(&new_alerts).await?;
    state.replace_active(&alerts);
    state.save()?;
    println!("Sent {} new lineup alert(s).", new_alerts.len());

    Ok(())
}
