//! In-memory state tracked by the coordinator across ticks.
//!
//! This state is intentionally separate from durable project state. It caches
//! the current work-item snapshot, provider health, and per-entity response-hash
//! history used for no-progress detection.

use std::collections::{HashMap, VecDeque};

use autore_schema::domain::records::WorkItemState;
use autore_schema::ids::{EntityId, ProviderRunId};

/// A lightweight read-only view of a work item used by the coordinator.
#[derive(Debug, Clone)]
pub struct CoordinatorWorkItem {
    pub work_item_id: String,
    pub kind: autore_schema::domain::records::WorkItemKind,
    /// Description carries intent when the coarse `WorkItemKind` is ambiguous
    /// (e.g. an `Investigation` may be static, dynamic, or semantic).
    pub description: String,
    pub state: WorkItemState,
    pub subject_entity: Option<EntityId>,
    pub dependencies: Vec<String>,
    /// Whether this item counts toward campaign completion.
    pub required: bool,
}

impl CoordinatorWorkItem {
    /// Entity key used for no-progress tracking.
    pub fn entity_key(&self) -> String {
        self.subject_entity
            .map(|e| e.to_string())
            .unwrap_or_else(|| self.work_item_id.clone())
    }
}

/// Health status of a provider instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderHealth {
    #[default]
    Unknown,
    Healthy,
    Degraded,
    Unhealthy,
}

/// Mutable coordinator state. This is rebuilt or updated each tick from the
/// durable store; the coordinator never writes directly to project storage.
#[derive(Debug, Clone, Default)]
pub struct CoordinatorState {
    /// Current work-item snapshot.
    pub work_items: Vec<CoordinatorWorkItem>,
    /// Last-known health per provider instance id.
    pub provider_health: HashMap<String, ProviderHealth>,
    /// Last raw-response hashes per entity, newest at the back.
    pub response_hashes: HashMap<String, VecDeque<u64>>,
    /// Monotonically increasing tick counter.
    pub tick_count: u64,
    /// If set, the coordinator should refresh program structure this tick.
    pub program_structure_refresh_run_id: Option<ProviderRunId>,
    /// Work-item dependencies as (successor, predecessor) pairs.
    pub dependencies: Vec<(String, String)>,
}

impl CoordinatorState {
    /// Returns the number of required work items.
    pub fn required_count(&self) -> usize {
        self.work_items.iter().filter(|w| w.required).count()
    }

    /// Returns required items that are in a terminal state.
    pub fn terminal_required(&self) -> Vec<&CoordinatorWorkItem> {
        self.work_items
            .iter()
            .filter(|w| w.required && is_terminal(w.state))
            .collect()
    }

    /// Returns true if any required item is blocked.
    pub fn has_blocked_required(&self) -> bool {
        self.work_items
            .iter()
            .any(|w| w.required && w.state == WorkItemState::Blocked)
    }

    /// Records a raw-response hash for an entity.
    pub fn record_hash(&mut self, entity_key: &str, hash: u64) {
        let history = self
            .response_hashes
            .entry(entity_key.to_string())
            .or_default();
        history.push_back(hash);
        while history.len() > 3 {
            history.pop_front();
        }
    }

    /// Returns the last N hashes for an entity.
    pub fn last_hashes(&self, entity_key: &str, n: usize) -> Vec<u64> {
        self.response_hashes
            .get(entity_key)
            .map(|h| h.iter().copied().rev().take(n).collect())
            .unwrap_or_default()
    }
}

pub(crate) fn is_terminal(state: WorkItemState) -> bool {
    matches!(
        state,
        WorkItemState::Completed | WorkItemState::Blocked | WorkItemState::Cancelled
    )
}
