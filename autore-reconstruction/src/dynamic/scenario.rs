//! Typed debugger scenario AST.
//!
//! This module defines the [`Scenario`] language — a closed set of setup,
//! step, and stop operations that a debugger provider can execute. The AST
//! is fully typed: no free-text script injection is permitted. An LLM may
//! propose a scenario, but it must pass [`super::verifier::ScenarioVerifier`]
//! validation before execution.

use std::collections::HashMap;
use std::path::PathBuf;

use autore_schema::domain::NamespacedId;
use autore_schema::ids::{ArtifactId, EntityId};

// ---------------------------------------------------------------------------
// Address range
// ---------------------------------------------------------------------------

/// A contiguous range of addresses `[start, end)` used to describe mapped
/// memory segments (e.g., IDA segment snapshots).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AddressRange {
    /// Inclusive start address.
    pub start: u128,
    /// Exclusive end address.
    pub end: u128,
}

impl AddressRange {
    /// Creates a new address range.
    pub fn new(start: u128, end: u128) -> Self {
        AddressRange { start, end }
    }

    /// Returns `true` if `addr` falls within `[start, end)`.
    pub fn contains(&self, addr: u128) -> bool {
        addr >= self.start && addr < self.end
    }

    /// Returns the length of the range in bytes.
    pub fn len(&self) -> u128 {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` if the range is empty.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

// ---------------------------------------------------------------------------
// Setup operations
// ---------------------------------------------------------------------------

/// Operations that initialize the debugging session.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SetupOp {
    /// Launch a target process from a binary artifact.
    LaunchTarget {
        /// The binary artifact to execute.
        exe_artifact: ArtifactId,
        /// Environment variables for the target process.
        env: HashMap<String, String>,
        /// Working directory for the target process.
        working_dir: PathBuf,
    },
    /// Attach to an already-running process.
    AttachTarget {
        /// Process ID to attach to.
        pid: u32,
    },
}

// ---------------------------------------------------------------------------
// Step operations
// ---------------------------------------------------------------------------

/// Individual debugger steps that form the scenario body.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Step {
    /// Set a breakpoint at a known entity's entry point.
    SetBreakpoint {
        /// The entity (typically a function) to break at.
        entity: EntityId,
    },
    /// Remove a previously set breakpoint at an entity.
    RemoveBreakpoint {
        /// The entity whose breakpoint to remove.
        entity: EntityId,
    },
    /// Continue execution until the next breakpoint or stop condition.
    Continue,
    /// Single-step one instruction.
    Step,
    /// Step out of the current function (finish/return).
    Finish,
    /// Capture all CPU registers at the current instruction pointer.
    CaptureRegisters,
    /// Capture the argument values of a function call at its entry.
    CaptureArguments {
        /// The function entity whose arguments to capture.
        entity: EntityId,
    },
    /// Capture the return value after a function returns.
    CaptureReturnValue {
        /// The function entity whose return value to capture.
        entity: EntityId,
    },
    /// Capture a contiguous memory region at a known address.
    CaptureMemoryRegion {
        /// Start address of the region.
        addr: u128,
        /// Number of bytes to capture.
        size: usize,
    },
    /// Capture a before/after delta of a memory region.
    CaptureMemoryDelta {
        /// Start address of the region.
        addr: u128,
        /// Number of bytes in the delta region.
        size: usize,
    },
    /// Capture the value of a global variable entity.
    CaptureGlobalValue {
        /// The global entity to read.
        entity: EntityId,
    },
    /// Capture the resolved call target of an indirect call.
    CaptureCallTarget,
    /// Capture an external (imported) API call by its namespaced identifier.
    CaptureExternalCall {
        /// The API identifier (must be in the operator's allowlist).
        api: NamespacedId,
    },
    /// Capture exception/SEH information when a fault occurs.
    CaptureException,
}

// ---------------------------------------------------------------------------
// Stop conditions
// ---------------------------------------------------------------------------

/// Conditions that terminate the scenario execution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StopOp {
    /// Stop after a function has been invoked a given number of times.
    StopAfterInvocationCount {
        /// Maximum invocation count before stopping.
        count: u32,
    },
    /// Stop after a wall-clock timeout.
    StopAfterTimeout {
        /// Timeout in milliseconds.
        ms: u64,
    },
    /// Terminate the target process immediately.
    TerminateTarget,
}

// ---------------------------------------------------------------------------
// Scenario
// ---------------------------------------------------------------------------

/// A complete debugger scenario: setup, body steps, and stop conditions.
///
/// Scenarios are proposed by an LLM and validated by
/// [`super::verifier::ScenarioVerifier`] before execution. The typed AST
/// prevents arbitrary script injection — only the operations defined above
/// are representable.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scenario {
    /// Setup operations (launch or attach).
    pub setup: Vec<SetupOp>,
    /// The sequence of debugger steps to execute.
    pub body: Vec<Step>,
    /// Conditions that stop the scenario.
    pub stop_conditions: Vec<StopOp>,
}

impl Scenario {
    /// Creates a new scenario.
    pub fn new(setup: Vec<SetupOp>, body: Vec<Step>, stop_conditions: Vec<StopOp>) -> Self {
        Scenario {
            setup,
            body,
            stop_conditions,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_range_contains() {
        let r = AddressRange::new(0x1000, 0x2000);
        assert!(r.contains(0x1000));
        assert!(r.contains(0x1FFF));
        assert!(!r.contains(0x2000)); // exclusive end
        assert!(!r.contains(0x0FFF));
    }

    #[test]
    fn address_range_len_and_empty() {
        let r = AddressRange::new(0x1000, 0x2000);
        assert_eq!(r.len(), 0x1000);
        assert!(!r.is_empty());

        let empty = AddressRange::new(0x1000, 0x1000);
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn scenario_serde_roundtrip() {
        let scenario = Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: ArtifactId::new(),
                env: HashMap::new(),
                working_dir: PathBuf::from("/tmp"),
            }],
            vec![
                Step::SetBreakpoint {
                    entity: EntityId::new(),
                },
                Step::Continue,
                Step::CaptureRegisters,
            ],
            vec![StopOp::StopAfterInvocationCount { count: 1 }],
        );
        let json = serde_json::to_string(&scenario).unwrap();
        let deserialized: Scenario = serde_json::from_str(&json).unwrap();
        assert_eq!(scenario, deserialized);
    }

    #[test]
    fn scenario_all_step_variants_serialize() {
        let eid = EntityId::new();
        let api = NamespacedId::parse("win32.kernel32.create-file").unwrap();
        let steps = vec![
            Step::SetBreakpoint { entity: eid },
            Step::RemoveBreakpoint { entity: eid },
            Step::Continue,
            Step::Step,
            Step::Finish,
            Step::CaptureRegisters,
            Step::CaptureArguments { entity: eid },
            Step::CaptureReturnValue { entity: eid },
            Step::CaptureMemoryRegion {
                addr: 0x401000,
                size: 256,
            },
            Step::CaptureMemoryDelta {
                addr: 0x401000,
                size: 128,
            },
            Step::CaptureGlobalValue { entity: eid },
            Step::CaptureCallTarget,
            Step::CaptureExternalCall { api },
            Step::CaptureException,
        ];
        let json = serde_json::to_string(&steps).unwrap();
        let deserialized: Vec<Step> = serde_json::from_str(&json).unwrap();
        assert_eq!(steps, deserialized);
    }

    #[test]
    fn setup_attach_target_serializes() {
        let op = SetupOp::AttachTarget { pid: 12345 };
        let json = serde_json::to_string(&op).unwrap();
        let deserialized: SetupOp = serde_json::from_str(&json).unwrap();
        assert_eq!(op, deserialized);
    }

    #[test]
    fn stop_ops_serialize() {
        let ops = vec![
            StopOp::StopAfterInvocationCount { count: 42 },
            StopOp::StopAfterTimeout { ms: 5000 },
            StopOp::TerminateTarget,
        ];
        let json = serde_json::to_string(&ops).unwrap();
        let deserialized: Vec<StopOp> = serde_json::from_str(&json).unwrap();
        assert_eq!(ops, deserialized);
    }
}
