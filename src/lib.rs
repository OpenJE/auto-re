// Remote TUI modules — feature-gated behind `tui`.
// These are experimental/incomplete and require crossterm + ratatui + smol.
#[cfg(feature = "tui")]
mod event;
#[cfg(feature = "tui")]
mod engine;
#[cfg(feature = "tui")]
mod store;
#[cfg(feature = "tui")]
mod tui;

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

pub type Result<T> = std::result::Result<T, Error>;
