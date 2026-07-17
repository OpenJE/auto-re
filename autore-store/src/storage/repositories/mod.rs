//! Repository trait definitions for the auto-re storage layer.
//!
//! Each trait defines the async interface for persisting and querying
//! a specific domain entity. For M1, only `TaskRepository` receives a
//! SQLite implementation (in Todo 11). Other traits are defined here
//! for architectural completeness and will be implemented in later
//! milestones.

pub mod claim;
pub mod task;

pub use claim::SqliteClaimRepository;
pub use task::SqliteTaskRepository;

use async_trait::async_trait;

use crate::domain::{ArtifactId, Campaign, CampaignState, Claim, Evidence, Function, Task};
use crate::ids::{BinaryRevisionId, CampaignId, ClaimId, EvidenceId, FunctionId, ModuleId, TaskId};

// ---------------------------------------------------------------------------
// CampaignRepository
// ---------------------------------------------------------------------------

/// Persistence interface for `Campaign` entities.
#[async_trait]
pub trait CampaignRepository: Send + Sync {
    /// Persists a new campaign and returns its ID.
    async fn create(&self, campaign: &Campaign) -> crate::Result<CampaignId>;

    /// Finds a campaign by ID, returning `None` if not found.
    async fn find_by_id(&self, id: CampaignId) -> crate::Result<Option<Campaign>>;

    /// Updates the state of an existing campaign.
    async fn update_state(&self, id: CampaignId, state: CampaignState) -> crate::Result<()>;
}

// ---------------------------------------------------------------------------
// BinaryRevisionRepository
// ---------------------------------------------------------------------------

/// Persistence interface for binary revision records.
#[async_trait]
pub trait BinaryRevisionRepository: Send + Sync {
    /// Finds a binary revision by ID.
    async fn find_by_id(
        &self,
        id: BinaryRevisionId,
    ) -> crate::Result<Option<crate::ids::BinaryRevisionId>>;
}

// ---------------------------------------------------------------------------
// ModuleRepository
// ---------------------------------------------------------------------------

/// Persistence interface for `Module` entities within a binary revision.
#[async_trait]
pub trait ModuleRepository: Send + Sync {
    /// Finds a module by ID.
    async fn find_by_id(&self, id: ModuleId) -> crate::Result<Option<ModuleId>>;
}

// ---------------------------------------------------------------------------
// FunctionRepository
// ---------------------------------------------------------------------------

/// Persistence interface for `Function` entities.
#[async_trait]
pub trait FunctionRepository: Send + Sync {
    /// Persists a new function and returns its ID.
    async fn create(&self, function: &Function) -> crate::Result<FunctionId>;

    /// Finds a function by ID.
    async fn find_by_id(&self, id: FunctionId) -> crate::Result<Option<Function>>;
}

// ---------------------------------------------------------------------------
// TaskRepository
// ---------------------------------------------------------------------------

/// Persistence interface for `Task` entities.
///
/// This is the primary repository with a SQLite implementation in M1.
/// It supports the full task lifecycle: creation, leasing, renewal,
/// completion, and failure.
#[async_trait]
pub trait TaskRepository: Send + Sync {
    /// Persists a new task and returns its ID.
    async fn create(&self, task: &Task) -> crate::Result<TaskId>;

    /// Atomically selects the next available task for the given campaign,
    /// creates a lease, and returns the task. Returns `None` if no tasks
    /// are available.
    async fn lease_next(
        &self,
        campaign_id: CampaignId,
        now: time::OffsetDateTime,
    ) -> crate::Result<Option<Task>>;

    /// Extends the lease on a task until the given deadline.
    async fn renew_lease(&self, task_id: TaskId, until: time::OffsetDateTime) -> crate::Result<()>;

    /// Marks a task as completed and releases its lease.
    async fn complete(&self, task_id: TaskId) -> crate::Result<()>;

    /// Marks a task as failed with an error message and releases its lease.
    async fn fail(&self, task_id: TaskId, error: String) -> crate::Result<()>;
}

// ---------------------------------------------------------------------------
// ClaimRepository
// ---------------------------------------------------------------------------

/// Persistence interface for `Claim` entities.
#[async_trait]
pub trait ClaimRepository: Send + Sync {
    /// Persists a new claim and returns its ID.
    async fn create(&self, claim: &Claim) -> crate::Result<ClaimId>;

    /// Finds a claim by ID.
    async fn find_by_id(&self, id: ClaimId) -> crate::Result<Option<Claim>>;
}

// ---------------------------------------------------------------------------
// EvidenceRepository
// ---------------------------------------------------------------------------

/// Persistence interface for `Evidence` entities.
#[async_trait]
pub trait EvidenceRepository: Send + Sync {
    /// Persists a new evidence record and returns its ID.
    async fn create(&self, evidence: &Evidence) -> crate::Result<EvidenceId>;

    /// Finds evidence by ID.
    async fn find_by_id(&self, id: EvidenceId) -> crate::Result<Option<Evidence>>;
}

// ---------------------------------------------------------------------------
// ArtifactRepository
// ---------------------------------------------------------------------------

/// Persistence interface for content-addressed artifact blobs.
#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    /// Stores an artifact and returns its ID.
    async fn store(&self, id: ArtifactId, content_hash: &str, data: &[u8]) -> crate::Result<()>;

    /// Retrieves artifact data by ID.
    async fn retrieve(&self, id: ArtifactId) -> crate::Result<Option<Vec<u8>>>;
}
