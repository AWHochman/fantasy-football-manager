use std::error::Error;

use fantasy_football_manager::{fetch_managed_snapshots, AppConfig};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env()?;
    for managed in fetch_managed_snapshots(&config).await? {
        println!("{}", serde_json::to_string_pretty(&managed.snapshot)?);
    }

    Ok(())
}
