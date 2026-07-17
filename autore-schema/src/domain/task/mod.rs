//! Task entity — a single unit of work within a campaign.
//!
//! Tasks are the atomic unit of the analysis scheduler. Each task has a
//! `TaskKind` (what to do), a `TaskState` (where in its lifecycle), a
//! `TaskSubject` (what entity it operates on), and a set of dependencies
//! that must complete before the task is eligible.

use std::collections::HashSet;

use crate::ids::{CampaignId, TaskId, WorkerRunId};
use autore_core::{Error, Result};

// Re-export sub-module types at module level.
pub use kind::TaskKind;
pub use types::{RequiredCapabilities, TaskPriority, TaskSubject};

mod kind;
mod types;

// ---------------------------------------------------------------------------
// TaskState
// ---------------------------------------------------------------------------

/// The lifecycle state of an analysis task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TaskState {
    /// Task has been created but dependencies not yet checked.
    Pending,
    /// Dependencies are satisfied and the task is eligible for leasing.
    Ready,
    /// A worker has acquired an exclusive lease on this task.
    Leased,
    /// The worker is actively executing the task.
    Running,
    /// Task cannot proceed because a dependency is not satisfied.
    Blocked,
    /// Task completed successfully.
    Completed,
    /// Task execution failed (may be retried).
    Failed,
    /// Task was cancelled before completion.
    Cancelled,
    /// Lease expired without completion; task may be re-queued.
    Stale,
}

impl TaskState {
    /// Returns `true` if the task is in a terminal state (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Completed | TaskState::Cancelled)
    }

    /// Returns `true` if the task is still active (can make progress).
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            TaskState::Pending
                | TaskState::Ready
                | TaskState::Leased
                | TaskState::Running
                | TaskState::Blocked
        )
    }

    /// Returns `true` if the task is available for leasing.
    pub fn is_available(&self) -> bool {
        matches!(self, TaskState::Ready)
    }

    /// Returns `true` if the task is in a recoverable failed/stale state.
    pub fn can_retry(&self) -> bool {
        matches!(self, TaskState::Failed | TaskState::Stale)
    }
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// A single unit of work within a campaign.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Task {
    /// Unique identifier for this task.
    pub id: TaskId,
    /// The campaign this task belongs to.
    pub campaign_id: CampaignId,
    /// What kind of analysis this task performs.
    pub kind: TaskKind,
    /// What entity this task operates on.
    pub subject: TaskSubject,
    /// Stable priority score for scheduling.
    pub priority: TaskPriority,
    /// Current lifecycle state.
    pub state: TaskState,
    /// IDs of tasks that must complete before this one can run.
    pub dependencies: Vec<TaskId>,
    /// Capabilities a worker must have to execute this task.
    pub required_capabilities: RequiredCapabilities,
    /// Preferred worker (if any) for this task.
    pub preferred_worker: Option<WorkerRunId>,
    /// The class of model required for model-inference tasks.
    pub preferred_model_class: Option<String>,
    /// Maximum number of attempts before the task is permanently failed.
    pub maximum_attempts: u32,
    /// Number of attempts so far.
    pub attempt_count: u32,
    /// Input revision for cache invalidation (bumped when dependencies'
    /// outputs change).
    pub input_revision: u64,
}

impl Task {
    /// Creates a new task in the `Pending` state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TaskId,
        campaign_id: CampaignId,
        kind: TaskKind,
        subject: TaskSubject,
        priority: TaskPriority,
        required_capabilities: RequiredCapabilities,
        preferred_worker: Option<WorkerRunId>,
        preferred_model_class: Option<String>,
        maximum_attempts: u32,
    ) -> Self {
        Task {
            id,
            campaign_id,
            kind,
            subject,
            priority,
            state: TaskState::Pending,
            dependencies: Vec::new(),
            required_capabilities,
            preferred_worker,
            preferred_model_class,
            maximum_attempts,
            attempt_count: 0,
            input_revision: 0,
        }
    }

    // -----------------------------------------------------------------------
    // State transitions
    // -----------------------------------------------------------------------

    /// Transitions from `Pending` → `Ready`.
    pub fn mark_ready(&mut self) -> Result<()> {
        match self.state {
            TaskState::Pending => {
                self.state = TaskState::Ready;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot mark task {:?} as ready from state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from `Ready` → `Leased`.
    pub fn lease(&mut self) -> Result<()> {
        match self.state {
            TaskState::Ready => {
                self.state = TaskState::Leased;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot lease task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from `Leased` → `Running`.
    pub fn start(&mut self) -> Result<()> {
        match self.state {
            TaskState::Leased => {
                self.state = TaskState::Running;
                self.attempt_count = self.attempt_count.saturating_add(1);
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot start task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from `Running` → `Completed`.
    pub fn complete(&mut self) -> Result<()> {
        match self.state {
            TaskState::Running => {
                self.state = TaskState::Completed;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot complete task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from `Running` → `Failed`.
    pub fn fail(&mut self) -> Result<()> {
        match self.state {
            TaskState::Running => {
                self.state = TaskState::Failed;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot fail task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from any non-terminal state → `Cancelled`.
    pub fn cancel(&mut self) -> Result<()> {
        if self.state.is_terminal() {
            return Err(Error::Validation(format!(
                "cannot cancel task {:?} in terminal state {:?}",
                self.id, self.state
            )));
        }
        self.state = TaskState::Cancelled;
        Ok(())
    }

    /// Transitions from `Ready` or `Pending` → `Blocked`.
    pub fn block(&mut self) -> Result<()> {
        match self.state {
            TaskState::Pending | TaskState::Ready => {
                self.state = TaskState::Blocked;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot block task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Transitions from `Blocked` → `Ready`.
    pub fn unblock(&mut self) -> Result<()> {
        match self.state {
            TaskState::Blocked => {
                self.state = TaskState::Ready;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot unblock task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Marks a leased task as stale (lease expired).
    /// Transitions from `Leased` → `Stale`.
    pub fn mark_stale(&mut self) -> Result<()> {
        match self.state {
            TaskState::Leased => {
                self.state = TaskState::Stale;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot mark task {:?} stale from state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Re-queues a stale or failed task (transitions to `Ready`).
    pub fn requeue(&mut self) -> Result<()> {
        match self.state {
            TaskState::Failed | TaskState::Stale => {
                if self.attempt_count >= self.maximum_attempts {
                    return Err(Error::Validation(format!(
                        "task {:?} has exhausted its {} maximum attempts",
                        self.id, self.maximum_attempts
                    )));
                }
                self.state = TaskState::Ready;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot requeue task {:?} in state {:?}",
                self.id, self.state
            ))),
        }
    }

    /// Returns `true` if all dependencies are satisfied.
    pub fn dependencies_satisfied(&self, completed_deps: &[TaskId]) -> bool {
        if self.dependencies.is_empty() {
            return true;
        }
        let completed: HashSet<&TaskId> = completed_deps.iter().collect();
        self.dependencies.iter().all(|dep| completed.contains(dep))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task() -> Task {
        Task::new(
            TaskId::new(),
            CampaignId::new(),
            TaskKind::AnalyzeFunction,
            TaskSubject::Binary,
            TaskPriority::new(100),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        )
    }

    fn add_dep(task: &mut Task, dep_id: TaskId) {
        task.dependencies.push(dep_id);
    }

    #[test]
    fn task_starts_pending() {
        let t = sample_task();
        assert_eq!(t.state, TaskState::Pending);
        assert!(t.state.is_active());
        assert!(!t.state.is_terminal());
        assert!(!t.state.is_available());
    }

    #[test]
    fn task_full_lifecycle() {
        let mut t = sample_task();
        t.mark_ready().unwrap();
        assert_eq!(t.state, TaskState::Ready);
        assert!(t.state.is_available());

        t.lease().unwrap();
        assert_eq!(t.state, TaskState::Leased);

        t.start().unwrap();
        assert_eq!(t.state, TaskState::Running);
        assert_eq!(t.attempt_count, 1);

        t.complete().unwrap();
        assert_eq!(t.state, TaskState::Completed);
        assert!(t.state.is_terminal());
    }

    #[test]
    fn task_state_transitions() {
        let mut t = sample_task();
        t.mark_ready().unwrap();
        assert_eq!(t.state, TaskState::Ready);

        t.lease().unwrap();
        assert_eq!(t.state, TaskState::Leased);

        t.start().unwrap();
        assert_eq!(t.state, TaskState::Running);

        let mut t2 = sample_task();
        t2.mark_ready().unwrap();
        t2.lease().unwrap();
        t2.start().unwrap();
        t2.fail().unwrap();
        assert_eq!(t2.state, TaskState::Failed);
        assert!(t2.state.can_retry());

        t2.requeue().unwrap();
        assert_eq!(t2.state, TaskState::Ready);

        let mut t3 = sample_task();
        t3.block().unwrap();
        assert_eq!(t3.state, TaskState::Blocked);
        assert!(!t3.state.is_terminal());

        t3.unblock().unwrap();
        assert_eq!(t3.state, TaskState::Ready);

        let mut t4 = sample_task();
        t4.mark_ready().unwrap();
        t4.lease().unwrap();
        t4.mark_stale().unwrap();
        assert_eq!(t4.state, TaskState::Stale);
        assert!(t4.state.can_retry());
    }

    #[test]
    fn task_rejects_invalid_transitions() {
        let mut t = sample_task();
        assert!(t.complete().is_err());

        let mut t2 = sample_task();
        t2.mark_ready().unwrap();
        t2.lease().unwrap();
        t2.start().unwrap();
        t2.complete().unwrap();
        assert!(t2.cancel().is_err());

        let mut t3 = sample_task();
        assert!(t3.lease().is_err());

        let mut t4 = sample_task();
        t4.mark_ready().unwrap();
        t4.lease().unwrap();
        t4.start().unwrap();
        assert!(t4.mark_stale().is_err());
        assert!(t4.requeue().is_err());

        let mut t5 = sample_task();
        assert!(t5.fail().is_err());

        let mut t6 = sample_task();
        t6.mark_ready().unwrap();
        assert!(t6.start().is_err());
    }

    #[test]
    fn task_requeue_exhausts_attempts() {
        let mut t = sample_task();
        t.state = TaskState::Stale;
        t.attempt_count = 3;
        assert!(t.requeue().is_err());
    }

    #[test]
    fn task_cancel_from_non_terminal() {
        let mut t = sample_task();
        t.cancel().unwrap();
        assert_eq!(t.state, TaskState::Cancelled);
        assert!(t.state.is_terminal());
    }

    #[test]
    fn task_no_dependencies() {
        let t = sample_task();
        assert!(t.dependencies.is_empty());
        assert!(t.dependencies_satisfied(&[]));
    }

    #[test]
    fn task_pending_dependencies() {
        let mut t = sample_task();
        let dep_id = TaskId::new();
        add_dep(&mut t, dep_id);

        assert!(!t.dependencies_satisfied(&[]));
        assert!(!t.dependencies_satisfied(&[TaskId::new()]));
        assert!(t.dependencies_satisfied(&[dep_id]));
    }

    #[test]
    fn task_dependencies() {
        let mut t = sample_task();
        let dep_a = TaskId::new();
        let dep_b = TaskId::new();
        let dep_c = TaskId::new();
        add_dep(&mut t, dep_a);
        add_dep(&mut t, dep_b);
        add_dep(&mut t, dep_c);

        assert!(!t.dependencies_satisfied(&[]));
        assert!(!t.dependencies_satisfied(&[dep_a]));
        assert!(t.dependencies_satisfied(&[dep_a, dep_b, dep_c]));
        assert!(t.dependencies_satisfied(&[dep_a, dep_b, dep_c, TaskId::new()]));
    }

    #[test]
    fn task_serialize_roundtrip() {
        let mut t = sample_task();
        t.mark_ready().unwrap();
        let json = serde_json::to_string(&t).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(t.id, deserialized.id);
        assert_eq!(t.state, deserialized.state);
        assert_eq!(t.priority.score(), 100);
        assert_eq!(deserialized.attempt_count, 0);
    }

    #[test]
    fn task_priority_display() {
        let p = TaskPriority::new(42);
        assert_eq!(p.to_string(), "42");
        assert_eq!(p.score(), 42);
    }

    #[test]
    fn task_required_capabilities_extras() {
        let caps =
            RequiredCapabilities::new(true, true, false, true).with_extra("special_analysis");
        assert!(caps.decompilation);
        assert!(caps.extras.contains("special_analysis"));
        assert!(!caps.extras.contains("nonexistent"));
    }
}
