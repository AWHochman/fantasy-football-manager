use std::error::Error;

use fantasy_football_manager::{
    evaluate_team, fetch_managed_snapshots, AlertNotifier, AppConfig, EmailNotifier,
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

    if alerts.is_empty() {
        println!("No actionable starter alerts.");
        return Ok(());
    }

    EmailNotifier::from_env()?.notify(&alerts).await?;
    println!("Sent {} lineup alert(s).", alerts.len());

    Ok(())
}
