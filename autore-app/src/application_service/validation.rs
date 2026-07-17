use autore_core::validation::{validate_confidence_range, validate_namespaced_id};
use autore_core::{Error, Result};
use autore_schema::domain::NamespacedId;

/// Validates a raw namespaced-ID string and parses it into a typed [`NamespacedId`].
pub fn parse_namespaced_id(id: &str) -> Result<NamespacedId> {
    validate_namespaced_id(id)?;
    NamespacedId::parse(id).map_err(|e| Error::Validation(e.to_string()))
}

/// Validates a hypothesis confidence score is finite and within [0.0, 1.0].
pub fn validate_confidence(score: f64) -> Result<()> {
    validate_confidence_range(score, "hypothesis confidence")
}

/// Validates that a sub-record's project matches the command's project.
pub fn ensure_same_project(label: &str, command_project: autore_schema::ids::ProjectId, record_project: autore_schema::ids::ProjectId) -> Result<()> {
    if command_project != record_project {
        return Err(Error::Validation(format!(
            "{label} references a different project than the command"
        )));
    }
    Ok(())
}

/// Validates that a value is not empty after trimming.
pub fn validate_not_empty(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::Validation(format!("{label} must not be empty")));
    }
    Ok(())
}
