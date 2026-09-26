use std::error::Error;

use fantasy_football_manager::EmailNotifier;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    EmailNotifier::from_env()?.send_test_email().await?;
    println!("Test email sent.");
    Ok(())
}
