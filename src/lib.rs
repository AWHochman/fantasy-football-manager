//! Read-only clients for retrieving fantasy football league rosters.
//!
//! The crate intentionally has no methods for lineup changes, transactions,
//! waivers, trades, or any other provider mutation.

mod algorithm;
mod error;
mod espn;
mod evaluator;
mod model;
mod sleeper;
mod source;

pub use algorithm::merge_player_enrichment;
pub use error::SourceError;
pub use espn::EspnSource;
pub use evaluator::{evaluate_team, AlertReason, EvaluationError, LineupAlert};
pub use model::{
    FantasyTeam, LeagueSnapshot, LineupStatus, PlayerAvailability, PlayerEnrichment, Provider,
    RosteredPlayer,
};
pub use sleeper::SleeperSource;
pub use source::FantasySource;
