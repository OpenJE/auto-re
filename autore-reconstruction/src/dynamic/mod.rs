//! Dynamic investigation: typed debugger scenarios and their verification.
//!
//! The [`scenario`] module defines a closed AST of debugger operations that
//! an LLM can propose. The [`verifier`] module validates a proposed scenario
//! against known entities, mapped memory segments, and an API allowlist
//! before it is sent to a debugger provider for execution.

pub mod ida_provider;
pub mod runner;
pub mod scenario;
pub mod verifier;

pub use ida_provider::{
    ScenarioResult, ScenarioStatus, debug_capabilities, execute_scenario,
    permissive_validation_context,
};
pub use runner::{CaptureContext, DebugObservation, RunnerError, TargetRunner, WineGdbRunner};
pub use scenario::{AddressRange, Scenario, SetupOp, Step, StopOp};
pub use verifier::{ScenarioValidationError, ScenarioVerifier};
