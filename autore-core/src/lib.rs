pub mod logging;
pub mod validation;

use thiserror::Error;

/// Stage 0 core error categories shared across all pipeline stages.
///
/// Stage-specific errors (e.g. model-provider, analysis-backend, IDA) live
/// in their respective stage crates and forward core errors via `From`.
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("hash mismatch")]
    HashMismatch,

    #[error("schema mismatch: expected {expected}, actual {actual}")]
    SchemaMismatch { expected: String, actual: String },

    #[error("migration error: {0}")]
    Migration(String),

    #[error("invalid state transition: {0}")]
    InvalidStateTransition(String),

    #[error("subscription error: {0}")]
    Subscription(String),

    #[error("operation error: {0}")]
    Operation(String),

    #[error("unsupported: {0}")]
    Unsupported(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_enum_database_display() {
        let err = Error::Database("connection failed".into());
        assert_eq!(err.to_string(), "database error: connection failed");
    }

    #[test]
    fn error_enum_validation_display() {
        let err = Error::Validation("bad input".into());
        assert_eq!(err.to_string(), "validation error: bad input");
    }

    #[test]
    fn error_enum_io_from_std() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(err.to_string().contains("io error:"));
    }

    #[test]
    fn error_enum_not_found_display() {
        let err = Error::NotFound("task abc".into());
        assert_eq!(err.to_string(), "not found: task abc");
    }

    #[test]
    fn error_enum_conflict_display() {
        let err = Error::Conflict("duplicate ID".into());
        assert_eq!(err.to_string(), "conflict: duplicate ID");
    }

    #[test]
    fn error_enum_hash_mismatch_display() {
        let err = Error::HashMismatch;
        assert_eq!(err.to_string(), "hash mismatch");
    }

    #[test]
    fn error_enum_schema_mismatch_display() {
        let err = Error::SchemaMismatch {
            expected: "1.0".into(),
            actual: "2.0".into(),
        };
        assert_eq!(err.to_string(), "schema mismatch: expected 1.0, actual 2.0");
    }

    #[test]
    fn error_enum_serialization_display() {
        let err = Error::Serialization("invalid JSON".into());
        assert_eq!(err.to_string(), "serialization error: invalid JSON");
    }

    #[test]
    fn error_enum_migration_display() {
        let err = Error::Migration("v1 to v2 failed".into());
        assert_eq!(err.to_string(), "migration error: v1 to v2 failed");
    }

    #[test]
    fn error_enum_invalid_state_transition_display() {
        let err = Error::InvalidStateTransition("Running -> Completed".into());
        assert_eq!(
            err.to_string(),
            "invalid state transition: Running -> Completed"
        );
    }

    #[test]
    fn error_enum_subscription_display() {
        let err = Error::Subscription("channel closed".into());
        assert_eq!(err.to_string(), "subscription error: channel closed");
    }

    #[test]
    fn error_enum_operation_display() {
        let err = Error::Operation("timed out".into());
        assert_eq!(err.to_string(), "operation error: timed out");
    }

    #[test]
    fn error_enum_unsupported_display() {
        let err = Error::Unsupported("format xyz".into());
        assert_eq!(err.to_string(), "unsupported: format xyz");
    }

    #[test]
    fn result_alias_default_error() {
        let ok_val: Result<i32> = Ok(42);
        assert_eq!(ok_val.unwrap(), 42);
    }

    #[test]
    fn result_alias_explicit_type() {
        let err_val: Result<i32, Error> = Err(Error::Validation("bad input".into()));
        assert!(err_val.is_err());
    }
}
