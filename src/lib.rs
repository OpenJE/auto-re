// TUI modules — decoupled from IDA; compiled when `tui` feature is enabled.
#[cfg(feature = "tui")]
mod event;
#[cfg(feature = "tui")]
pub mod tui;

// IDA-dependent modules — gated behind `ida` feature (engine.rs imports `idax`).
#[cfg(feature = "ida")]
mod engine;
#[cfg(feature = "ida")]
mod store;

// ---------------------------------------------------------------------------
// Core error type (spec-aligned)
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error("configuration error: {0}")]
	Configuration(String),

	#[error("database error: {0}")]
	Database(String),

	#[error("model provider error: {0}")]
	ModelProvider(String),

	#[error("analysis backend error: {0}")]
	AnalysisBackend(String),

	#[error("worker error: {0}")]
	Worker(String),

	#[error("validation error: {0}")]
	Validation(String),

	#[error("io error: {0}")]
	Io(#[from] std::io::Error),

	#[cfg(feature = "ida")]
	#[error("ida error: {0}")]
	Ida(#[from] idax::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

// ---------------------------------------------------------------------------
// Typed IDs and domain primitives (always compiled, feature-independent)
// ---------------------------------------------------------------------------

pub mod ids;
pub mod domain;

// Re-export frequently used types at crate root so callers can write
// `use crate::TaskId` or `use crate::Confidence` without naming the module.
pub use domain::{
    Address, AddressSpace, ArtifactId, Campaign, CampaignState, Claim, ClaimPredicate,
    ClaimState, ClaimValue, Confidence, ContentHash, EntityId, Evidence, EvidenceKind,
    EvidenceLocation, Function, Provenance, RequiredCapabilities, SymbolName, Task,
    TaskKind, TaskPriority, TaskState, TaskSubject,
};
pub use ids::{
    BinaryId, BinaryRevisionId, CampaignId, ClaimId, EvidenceId, FunctionId,
    ImplementationTargetId, ModuleId, ProjectId, TaskId, TransactionId,
    ValidationRunId, WorkerRunId,
};

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn error_enum_database_display() {
		let err = Error::Database("connection failed".into());
		assert_eq!(err.to_string(), "database error: connection failed");
	}

	#[test]
	fn error_enum_configuration_display() {
		let err = Error::Configuration("missing key".into());
		assert_eq!(err.to_string(), "configuration error: missing key");
	}

	#[test]
	fn error_enum_io_from_std() {
		let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
		let err: Error = io_err.into();
		assert!(err.to_string().contains("io error:"));
	}

	#[test]
	fn result_alias_default_error() {
		// Verify that Result<T> defaults to Error
		let ok_val: Result<i32> = Ok(42);
		assert_eq!(ok_val.unwrap(), 42);
	}

	#[test]
	fn result_alias_explicit_type() {
		let err_val: Result<i32, Error> = Err(Error::Validation("bad input".into()));
		assert!(err_val.is_err());
	}

	#[cfg(feature = "tui")]
	#[test]
	fn tui_compiles() {
		// Verify the run_tui symbol exists.
		// This test passes at compile time; actual TUI is tested via smoke test.
		let _sig: fn() -> crate::tui::Tui = crate::tui::Tui::new;
	}
}
