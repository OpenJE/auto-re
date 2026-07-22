//! Scheduler — deterministic priority scoring and model-routed dispatch.

mod lease;
mod repos;
#[allow(clippy::module_inception)]
mod scheduler;

pub use lease::TaskLease;
pub use repos::{RepositorySet, SchedulerQueries};
pub use scheduler::{CampaignEvaluation, PriorityContext, PriorityFactors, Scheduler};
