//! Read-only clients for retrieving fantasy football league rosters.
//!
//! The crate intentionally has no methods for lineup changes, transactions,
//! waivers, trades, or any other provider mutation.

mod error;
mod espn;
mod model;
mod sleeper;
mod source;

pub use error::SourceError;
pub use espn::EspnSource;
pub use model::{
    FantasyTeam, LeagueSnapshot, LineupStatus, PlayerAvailability, Provider, RosteredPlayer,
};
pub use sleeper::SleeperSource;
pub use source::FantasySource;
