//! Scheduler — deterministic priority scoring and model-routed dispatch.

mod lease;
mod repos;
mod scheduler;

pub use lease::TaskLease;
pub use repos::{RepositorySet, SchedulerQueries};
pub use scheduler::{CampaignEvaluation, PriorityFactors, Scheduler};
