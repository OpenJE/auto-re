//! Repository set and scheduler-specific query trait.
//!
//! The `RepositorySet` bundles all repository references the scheduler
//! needs. `SchedulerQueries` provides additional query methods beyond
//! the base `TaskRepository` trait for lease recovery, dependency
//! promotion, and state evaluation.

use std::sync::Arc;

use async_trait::async_trait;
use time::OffsetDateTime;

use crate::domain::{Task, TaskState};
use crate::ids::{CampaignId, TaskId};
use crate::storage::repositories::{
    CampaignRepository, ClaimRepository, EvidenceRepository, TaskRepository,
};

use super::lease::TaskLease;

// ---------------------------------------------------------------------------
// SchedulerQueries
// ---------------------------------------------------------------------------

/// Additional query methods the scheduler needs beyond `TaskRepository`.
///
/// These support lease recovery, dependency promotion, and state
/// evaluation — operations requiring bulk queries and direct state
/// manipulation not covered by the base `TaskRepository` trait.
#[async_trait]
pub trait SchedulerQueries: Send + Sync {
    /// Returns all tasks for a campaign.
    async fn find_tasks_by_campaign(&self, campaign_id: CampaignId) -> crate::Result<Vec<Task>>;

    /// Returns leases that have expired as of `now`.
    async fn find_expired_leases(
        &self,
        campaign_id: CampaignId,
        now: OffsetDateTime,
    ) -> crate::Result<Vec<TaskLease>>;

    /// Updates a task's state directly (for recovery/promotion).
    async fn update_task_state(&self, task_id: TaskId, state: TaskState) -> crate::Result<()>;

    /// Removes the lease record for a task (for lease recovery).
    async fn delete_lease(&self, task_id: TaskId) -> crate::Result<()>;
}

// ---------------------------------------------------------------------------
// RepositorySet
// ---------------------------------------------------------------------------

/// Collection of repository references the scheduler needs.
///
/// Bundles all persistence interfaces required for campaign evaluation,
/// lease recovery, dependency promotion, and task dispatch.
pub struct RepositorySet {
    /// Base task repository for leasing, completion, and failure.
    pub tasks: Arc<dyn TaskRepository>,
    /// Extended query interface for scheduler-specific operations.
    pub queries: Arc<dyn SchedulerQueries>,
    /// Campaign repository for state management.
    pub campaigns: Arc<dyn CampaignRepository>,
    /// Claim repository for claim tracking.
    pub claims: Arc<dyn ClaimRepository>,
    /// Evidence repository for evidence tracking.
    pub evidence: Arc<dyn EvidenceRepository>,
}
