use thiserror::Error;

#[derive(Debug, Error)]
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
        let ok_val: Result<i32> = Ok(42);
        assert_eq!(ok_val.unwrap(), 42);
    }

    #[test]
    fn result_alias_explicit_type() {
        let err_val: Result<i32, Error> = Err(Error::Validation("bad input".into()));
        assert!(err_val.is_err());
    }
}
