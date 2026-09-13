use async_trait::async_trait;

use crate::{LeagueSnapshot, Provider, SourceError};

/// A read-only provider integration.
///
/// Implementations expose only league retrieval so the monitoring service has
/// no capability to modify a fantasy account.
#[async_trait]
pub trait FantasySource: Send + Sync {
    fn provider(&self) -> Provider;

    async fn fetch_league(&self) -> Result<LeagueSnapshot, SourceError>;
}
