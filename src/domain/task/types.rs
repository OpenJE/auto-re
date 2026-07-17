use std::collections::HashSet;

use crate::domain::EntityId;

// ---------------------------------------------------------------------------
// TaskSubject
// ---------------------------------------------------------------------------

/// The entity that a task operates on.
///
/// This allows tasks to target specific functions, modules, campaigns,
/// claims, or an entire binary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TaskSubject {
    /// The task operates on a specific entity (function, campaign, claim, etc.).
    Entity(EntityId),
    /// The task operates on the entire binary (inventory, global analysis).
    Binary,
    /// The task is a global/campaign-level operation with no specific target.
    Global,
}

// ---------------------------------------------------------------------------
// TaskPriority
// ---------------------------------------------------------------------------

/// A stable priority score for task scheduling.
///
/// Higher values indicate higher priority. Scores are intentionally opaque
/// — computed from `PriorityFactors` by the scheduler, not set directly.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct TaskPriority(u64);

impl TaskPriority {
    /// Creates a new priority with a given score.
    pub fn new(score: u64) -> Self {
        TaskPriority(score)
    }

    /// Returns the raw priority score.
    pub fn score(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for TaskPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// RequiredCapabilities
// ---------------------------------------------------------------------------

/// The set of capabilities a worker must possess to execute this task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RequiredCapabilities {
    /// Whether the worker must be able to decompile functions.
    pub decompilation: bool,
    /// Whether the worker must be able to disassemble.
    pub disassembly: bool,
    /// Whether the worker must be able to run dynamic analysis.
    pub dynamic_analysis: bool,
    /// Whether the worker must have a model provider for LLM tasks.
    pub model_inference: bool,
    /// Additional capability requirements as string keys.
    pub extras: HashSet<String>,
}

impl RequiredCapabilities {
    /// Creates a set of required capabilities with no extras.
    pub fn new(
        decompilation: bool,
        disassembly: bool,
        dynamic_analysis: bool,
        model_inference: bool,
    ) -> Self {
        RequiredCapabilities {
            decompilation,
            disassembly,
            dynamic_analysis,
            model_inference,
            extras: HashSet::new(),
        }
    }

    /// Adds an extra capability requirement.
    pub fn with_extra(mut self, cap: impl Into<String>) -> Self {
        self.extras.insert(cap.into());
        self
    }
}
