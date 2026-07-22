//! Schema loading and request-payload construction.
//!
//! Each of the 7 LLM analysis capabilities has a response JSON Schema
//! committed under `schemas/analysis/`. These are embedded at compile time
//! and loaded on demand. `request_payload_for` produces the final JSON
//! payload for an `ExecutionRequest` — artifact handles + cooked textual
//! summaries, no raw bytes.

use serde_json::Value;

use crate::analysis::bundle::InvestigationBundle;

/// The 7 analysis capabilities.
pub const CAPABILITIES: &[&str] = &[
    "llm.analysis.function",
    "llm.analysis.type",
    "llm.analysis.class",
    "llm.analysis.subsystem",
    "llm.analysis.conflict",
    "llm.analysis.failure",
    "llm.experiment.design",
];

/// Returns the response schema for a capability ID.
///
/// Schemas are embedded via `include_str!` from the committed JSON files.
pub fn response_schema_for(capability_id: &str) -> Option<Value> {
    let raw = match capability_id {
        "llm.analysis.function" => {
            include_str!("../../schemas/analysis/function-analysis.schema.json")
        }
        "llm.analysis.type" => include_str!("../../schemas/analysis/type-analysis.schema.json"),
        "llm.analysis.class" => include_str!("../../schemas/analysis/class-analysis.schema.json"),
        "llm.analysis.subsystem" => {
            include_str!("../../schemas/analysis/subsystem-analysis.schema.json")
        }
        "llm.analysis.conflict" => {
            include_str!("../../schemas/analysis/conflict-analysis.schema.json")
        }
        "llm.analysis.failure" => {
            include_str!("../../schemas/analysis/failure-analysis.schema.json")
        }
        "llm.experiment.design" => {
            include_str!("../../schemas/analysis/experiment-design.schema.json")
        }
        _ => return None,
    };
    serde_json::from_str(raw).ok()
}

/// Returns the shared request (bundle) schema.
pub fn request_schema() -> Value {
    let raw = include_str!("../../schemas/analysis/analysis-bundle-request.schema.json");
    serde_json::from_str(raw).expect("embedded request schema is valid JSON")
}

/// Produces the final JSON payload sent in an `ExecutionRequest`.
///
/// Serializes the bundle to a JSON value. Artifact fields are serialized
/// as `ArtifactId` strings — no raw bytes are ever included.
pub fn request_payload_for(bundle: &InvestigationBundle) -> Vec<u8> {
    serde_json::to_vec(bundle).expect("InvestigationBundle serializes to JSON")
}

/// Validates that a bundle payload conforms to the request schema.
///
/// Returns `Ok(())` if the payload validates, or `Err` with the first
/// validation error message.
pub fn validate_request_payload(payload: &Value) -> Result<(), String> {
    let schema = request_schema();
    let validator =
        jsonschema::validator_for(&schema).map_err(|e| format!("schema compile: {e}"))?;
    validator.validate(payload).map_err(|e| e.to_string())
}

/// Validates a response value against a capability's response schema.
pub fn validate_response_payload(capability_id: &str, response: &Value) -> Result<(), String> {
    let schema = response_schema_for(capability_id)
        .ok_or_else(|| format!("unknown capability: {capability_id}"))?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|e| format!("schema compile: {e}"))?;
    validator.validate(response).map_err(|e| e.to_string())
}
