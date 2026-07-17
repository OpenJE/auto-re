//! Worker subsystem — analysis workers, output types, and scheduling.
//!
//! This module is feature-independent: it compiles with `--no-default-features`
//! and does not import IDA, LLM, TUI, or debugger crates.

pub mod output;
pub mod runner;

pub use output::{FunctionAnalysisOutput, ProposedClaim, ProposedEvidence, validate_output};
pub use runner::{WorkerInput, WorkerOutput, WorkerRunner};
