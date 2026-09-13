use std::error::Error;

use fantasy_football_manager::{evaluate_team, fetch_managed_snapshots, AppConfig};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let config = AppConfig::from_env()?;
    let snapshots = fetch_managed_snapshots(&config).await?;

    for managed in snapshots {
        for alert in evaluate_team(&managed.snapshot, &managed.team.team_id)? {
            println!(
                "{}: {} ({:?})",
                alert.league_name, alert.player_name, alert.reasons
            );
        }
    }

    Ok(())
}
