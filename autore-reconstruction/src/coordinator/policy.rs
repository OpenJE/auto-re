//! Completion policy and no-progress detection for the coordinator.
//!
//! Spec §14.3: the campaign is terminal only when every required work item is
//! Verified (`Completed`), Blocked, or ExplicitlyExcluded (`Cancelled`).
//! Blocked items do NOT count as success.

use autore_schema::domain::records::WorkItemState;

use crate::coordinator::state::{CoordinatorState, CoordinatorWorkItem, is_terminal};

/// Decides when the campaign has reached a terminal state.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompletionPolicy;

impl CompletionPolicy {
    /// Returns true when every required work item is terminal.
    ///
    /// Terminal states: Completed (verified), Blocked, Cancelled (explicitly
    /// excluded). This is intentionally separate from [`is_successfully_complete`]
    /// so the coordinator can stop looping without claiming success when blocked
    /// items remain.
    pub fn is_complete(state: &CoordinatorState) -> bool {
        state.required_count() == state.terminal_required().len()
    }

    /// Returns true when every required work item is completed or cancelled and
    /// none are blocked.
    pub fn is_successfully_complete(state: &CoordinatorState) -> bool {
        if !Self::is_complete(state) {
            return false;
        }
        !state.has_blocked_required()
            && state
                .work_items
                .iter()
                .filter(|w| w.required)
                .all(|w| matches!(w.state, WorkItemState::Completed | WorkItemState::Cancelled))
    }
}

/// Detects stagnation by comparing the last N raw-response hashes for an entity.
#[derive(Debug, Clone, Copy)]
pub struct NoProgressDetector {
    threshold: usize,
}

impl Default for NoProgressDetector {
    fn default() -> Self {
        Self { threshold: 3 }
    }
}

impl NoProgressDetector {
    /// Creates a detector with a custom threshold.
    pub fn with_threshold(threshold: usize) -> Self {
        Self { threshold }
    }

    /// Returns true if the last `threshold` hashes for `entity_key` are
    /// identical and non-zero, indicating repeated identical model output.
    pub fn is_stuck(&self, state: &CoordinatorState, entity_key: &str) -> bool {
        let history = state.last_hashes(entity_key, self.threshold);
        if history.len() < self.threshold {
            return false;
        }
        let first = history[0];
        first != 0 && history.iter().all(|h| *h == first)
    }
}

/// Tag attached to a `BlockWorkItem` command when no-progress is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoProgressKind {
    RepeatedIdenticalModelOutput,
}

impl NoProgressKind {
    pub fn as_reason(self) -> &'static str {
        match self {
            NoProgressKind::RepeatedIdenticalModelOutput => "RepeatedIdenticalModelOutput",
        }
    }
}

/// Returns true if `state` is available for dispatch.
pub fn is_dispatchable(state: WorkItemState) -> bool {
    state == WorkItemState::Ready
}

/// Returns true if a non-terminal item has all dependencies terminal.
pub fn dependencies_satisfied(item: &CoordinatorWorkItem, state: &CoordinatorState) -> bool {
    item.dependencies.iter().all(|dep_id| {
        state
            .work_items
            .iter()
            .find(|w| &w.work_item_id == dep_id)
            .map(|w| is_terminal(w.state))
            .unwrap_or(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::state::CoordinatorWorkItem;
    use autore_schema::domain::records::WorkItemKind;
    use autore_schema::ids::WorkItemId;

    fn work_item(state: WorkItemState, required: bool) -> CoordinatorWorkItem {
        CoordinatorWorkItem {
            work_item_id: WorkItemId::new().to_string(),
            kind: WorkItemKind::Function,
            description: String::new(),
            state,
            subject_entity: None,
            dependencies: Vec::new(),
            required,
        }
    }

    #[test]
    fn complete_when_all_required_terminal() {
        let mut state = CoordinatorState::default();
        state
            .work_items
            .push(work_item(WorkItemState::Completed, true));
        state
            .work_items
            .push(work_item(WorkItemState::Blocked, true));
        state
            .work_items
            .push(work_item(WorkItemState::Pending, false));
        assert!(CompletionPolicy::is_complete(&state));
        assert!(!CompletionPolicy::is_successfully_complete(&state));
    }

    #[test]
    fn not_complete_when_required_missing() {
        let mut state = CoordinatorState::default();
        state
            .work_items
            .push(work_item(WorkItemState::Completed, true));
        state.work_items.push(work_item(WorkItemState::Ready, true));
        assert!(!CompletionPolicy::is_complete(&state));
    }

    #[test]
    fn no_progress_detected_after_three_identical_hashes() {
        let mut state = CoordinatorState::default();
        let detector = NoProgressDetector::default();
        state.record_hash("ent", 42);
        assert!(!detector.is_stuck(&state, "ent"));
        state.record_hash("ent", 42);
        assert!(!detector.is_stuck(&state, "ent"));
        state.record_hash("ent", 42);
        assert!(detector.is_stuck(&state, "ent"));
    }

    #[test]
    fn no_progress_not_detected_with_varied_hashes() {
        let mut state = CoordinatorState::default();
        let detector = NoProgressDetector::default();
        state.record_hash("ent", 1);
        state.record_hash("ent", 2);
        state.record_hash("ent", 3);
        assert!(!detector.is_stuck(&state, "ent"));
    }
}
