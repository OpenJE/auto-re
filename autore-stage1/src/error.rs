//! Stage 1 error types.
//!
//! Stage-specific variants (Configuration, ModelProvider, AnalysisBackend,
//! Worker) live here. Core pipeline errors are forwarded via `Core(#[from])`.

use thiserror::Error;

/// Stage 1 errors — wraps core errors and adds stage-local categories.
#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Configuration(String),

    #[error("model provider error: {0}")]
    ModelProvider(String),

    #[error("analysis backend error: {0}")]
    AnalysisBackend(String),

    #[error("worker error: {0}")]
    Worker(String),

    #[error(transparent)]
    Core(#[from] autore_core::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
