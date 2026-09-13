use std::{env, error::Error, io};

use fantasy_football_manager::EspnSource;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    let ids = required("ESPN_PLAYER_IDS")?
        .split(',')
        .map(|value| value.trim().parse::<i64>())
        .collect::<Result<Vec<_>, _>>()?;
    let source = EspnSource::new(
        required("ESPN_LEAGUE_ID")?.parse::<u64>()?,
        required("ESPN_SEASON")?.parse::<u16>()?,
        required("ESPN_SWID")?,
        required("ESPN_S2")?,
    );
    let players = source
        .fetch_player_enrichment(&ids, required("ESPN_SCORING_PERIOD")?.parse::<u8>()?)
        .await?;
    println!("{}", serde_json::to_string_pretty(&players)?);
    Ok(())
}

fn required(name: &str) -> Result<String, io::Error> {
    env::var(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}
