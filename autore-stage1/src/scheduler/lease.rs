//! Task lease representation — an active exclusive worker lock.
//!
//! A `TaskLease` is created by the scheduler during dispatch and tracked
//! until the task completes, fails, or the lease expires.

use time::OffsetDateTime;

use crate::ids::{CampaignId, TaskId};

/// An active lease on a task, representing exclusive worker access.
#[derive(Debug, Clone)]
pub struct TaskLease {
    /// The task this lease applies to.
    pub task_id: TaskId,
    /// The campaign the task belongs to.
    pub campaign_id: CampaignId,
    /// Identifier of the worker holding this lease.
    pub worker_id: String,
    /// When the lease was created.
    pub started_at: OffsetDateTime,
    /// When the lease expires and becomes recoverable.
    pub expires_at: OffsetDateTime,
}
