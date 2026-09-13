use std::{env, error::Error, io};

use fantasy_football_manager::{EspnSource, FantasySource, SleeperSource};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    if let Some(league_id) = optional_env("SLEEPER_LEAGUE_ID") {
        let snapshot = SleeperSource::new(league_id).fetch_league().await?;
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    }

    match (
        optional_env("ESPN_LEAGUE_ID"),
        optional_env("ESPN_SWID"),
        optional_env("ESPN_S2"),
    ) {
        (Some(league_id), Some(swid), Some(espn_s2)) => {
            let season = optional_env("ESPN_SEASON")
                .unwrap_or_else(|| "2026".to_owned())
                .parse::<u16>()?;
            let snapshot = EspnSource::new(league_id.parse::<u64>()?, season, swid, espn_s2)
                .fetch_league()
                .await?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        (None, None, None) => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ESPN_LEAGUE_ID, ESPN_SWID, and ESPN_S2 must be set together",
            )
            .into())
        }
    }

    Ok(())
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}
