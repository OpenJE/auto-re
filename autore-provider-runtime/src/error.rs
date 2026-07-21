//! Runtime errors for provider lifecycle management.

use thiserror::Error;

/// Errors that can occur during provider runtime operations.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("authentication failed: {0}")]
    Authentication(String),

    #[error("negotiation failed: {0}")]
    Negotiate(#[from] NegotiateError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    #[error("grpc status error: {0}")]
    Status(#[from] tonic::Status),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("spawn error: {0}")]
    Spawn(String),

    #[error("package identity mismatch: {0}")]
    PackageIdentity(String),

    #[error("concurrency limit exceeded for capability {capability_id}")]
    ConcurrencyLimitExceeded { capability_id: String },
}

/// Errors specific to protocol negotiation.
#[derive(Debug, Error)]
pub enum NegotiateError {
    #[error(
        "unsupported protocol version: provider range [{min}, {max}] does not include coordinator version 1"
    )]
    UnsupportedVersion { min: u32, max: u32 },

    #[error("invalid negotiation response: {0}")]
    InvalidResponse(String),
}
