use crate::{FreeAgentRecommendation, LineupAlert, LineupRecommendation};

/// Any actionable notification produced by the provider-neutral monitor.
#[derive(Clone, Debug)]
pub enum MonitorAlert {
    Starter(LineupAlert),
    RecommendedLineup(LineupRecommendation),
    RecommendedFreeAgentLineup(FreeAgentRecommendation),
}
