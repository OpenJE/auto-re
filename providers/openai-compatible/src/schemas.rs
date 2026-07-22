//! JSON Schema definitions for the 7 analysis capabilities and 6 generation
//! capabilities.
//!
//! Each analysis capability shares a common request schema (the bounded
//! investigation bundle) and has its own response schema. Generation
//! capabilities accept an `InvestigationBundle` plus a `GenerationContext`.
//! Schemas are embedded in the binary and exposed via
//! `CapabilityDescriptor.request_schema` and `response_schema` on Negotiate.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use autore_provider_protocol::v1::CapabilityDescriptor;

/// Names of the 7 analysis capabilities declared in spec §8.2.
pub const ANALYSIS_CAPABILITIES: &[&str] = &[
    "llm.analysis.function",
    "llm.analysis.type",
    "llm.analysis.class",
    "llm.analysis.subsystem",
    "llm.analysis.conflict",
    "llm.analysis.failure",
    "llm.experiment.design",
];

/// Names of the 6 generation capabilities declared in spec §11.4.
pub const GENERATION_CAPABILITIES: &[&str] = &[
    "llm.generation.declaration",
    "llm.generation.type",
    "llm.generation.function",
    "llm.generation.cluster",
    "llm.generation.test",
    "llm.generation.repair",
];

/// Context packet sent alongside an `InvestigationBundle` to a generation
/// capability (spec §11.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationContext {
    /// Accepted canonical type hypotheses available to the generator.
    pub accepted_types: Vec<Value>,
    /// Accepted canonical function/class specs available to the generator.
    pub accepted_specs: Vec<Value>,
    /// Map from entity/target identifier to currently generated stub bytes
    /// (base64-encoded strings).
    pub generated_stubs: HashMap<String, String>,
    /// ArtifactId string of the prior generated candidate, if this is a
    /// regeneration or repair pass.
    pub prior_generated_candidate: Option<String>,
    /// Summarized compiler diagnostics relevant to repair generation.
    pub compiler_diagnostics: Vec<Value>,
}

impl GenerationContext {
    /// Create an empty generation context for tests or minimal requests.
    pub fn empty() -> Self {
        Self {
            accepted_types: Vec::new(),
            accepted_specs: Vec::new(),
            generated_stubs: HashMap::new(),
            prior_generated_candidate: None,
            compiler_diagnostics: Vec::new(),
        }
    }
}

pub fn request_schema() -> Value {
    json!({
        "type": "object",
        "required": ["subject_entity_id"],
        "additionalProperties": true,
        "properties": {
            "subject_entity_id": {"type": "string", "minLength": 1},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "relevant_entity_ids": {"type": "array", "items": {"type": "string"}},
            "context_bytes_b64": {"type": "string"},
            "config_hints": {"type": "object", "additionalProperties": true}
        }
    })
}

/// Request schema shared by all generation capabilities.
///
/// The payload is an object with `bundle` (an `InvestigationBundle`) and
/// `generation_context` (`GenerationContext`).
pub fn generation_request_schema() -> Value {
    json!({
        "type": "object",
        "required": ["bundle", "generation_context"],
        "additionalProperties": false,
        "properties": {
            "bundle": {
                "type": "object",
                "description": "InvestigationBundle with artifact handles and summaries."
            },
            "generation_context": {
                "type": "object",
                "required": ["accepted_types", "accepted_specs", "generated_stubs"],
                "additionalProperties": false,
                "properties": {
                    "accepted_types": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "accepted_specs": {
                        "type": "array",
                        "items": { "type": "object" }
                    },
                    "generated_stubs": {
                        "type": "object",
                        "additionalProperties": { "type": "string" }
                    },
                    "prior_generated_candidate": {
                        "type": ["string", "null"]
                    },
                    "compiler_diagnostics": {
                        "type": "array",
                        "items": { "type": "object" }
                    }
                }
            }
        }
    })
}

pub fn response_schema_function() -> Value {
    json!({
        "type": "object",
        "required": ["proposed_name", "behavior_claims", "evidence_references", "confidence", "recommended_follow_up_work"],
        "additionalProperties": false,
        "properties": {
            "proposed_name": {"type": "string"},
            "behavior_claims": {"type": "array", "items": {"type": "string"}},
            "side_effects": {"type": "array", "items": {"type": "string"}},
            "signature": {"type": "string"},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
            "recommended_follow_up_work": {"type": "array", "items": {"type": "string"}}
        }
    })
}

pub fn response_schema_type() -> Value {
    json!({
        "type": "object",
        "required": ["proposed_name", "proposed_layout", "evidence_references", "confidence"],
        "additionalProperties": false,
        "properties": {
            "proposed_name": {"type": "string"},
            "proposed_layout": {"type": "array", "items": {"type": "object", "additionalProperties": true}},
            "size_bytes": {"type": "integer", "minimum": 0},
            "alignment": {"type": "integer", "minimum": 1},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        }
    })
}

pub fn response_schema_class() -> Value {
    json!({
        "type": "object",
        "required": ["proposed_name", "vtable_address", "method_ids", "evidence_references"],
        "additionalProperties": false,
        "properties": {
            "proposed_name": {"type": "string"},
            "base_class_ids": {"type": "array", "items": {"type": "string"}},
            "vtable_address": {"type": "integer", "minimum": 0},
            "method_ids": {"type": "array", "items": {"type": "string"}},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        }
    })
}

pub fn response_schema_subsystem() -> Value {
    json!({
        "type": "object",
        "required": ["subsystem_name", "member_entity_ids", "responsibility", "evidence_references"],
        "additionalProperties": false,
        "properties": {
            "subsystem_name": {"type": "string"},
            "responsibility": {"type": "string"},
            "member_entity_ids": {"type": "array", "items": {"type": "string"}},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        }
    })
}

pub fn response_schema_conflict() -> Value {
    json!({
        "type": "object",
        "required": ["resolution_kind", "accepted_hypothesis_id", "rejected_hypothesis_ids", "rationale", "evidence_references"],
        "additionalProperties": false,
        "properties": {
            "resolution_kind": {"type": "string"},
            "accepted_hypothesis_id": {"type": "string"},
            "rejected_hypothesis_ids": {"type": "array", "items": {"type": "string"}},
            "rationale": {"type": "string"},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        }
    })
}

pub fn response_schema_failure() -> Value {
    json!({
        "type": "object",
        "required": ["root_cause", "affected_entity_ids", "recommended_action", "evidence_references"],
        "additionalProperties": false,
        "properties": {
            "root_cause": {"type": "string"},
            "affected_entity_ids": {"type": "array", "items": {"type": "string"}},
            "recommended_action": {"type": "string"},
            "evidence_references": {"type": "array", "items": {"type": "string"}},
            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
        }
    })
}

pub fn response_schema_experiment_design() -> Value {
    json!({
        "type": "object",
        "required": ["hypothesis_statement", "test_plan", "expected_observations", "required_capabilities"],
        "additionalProperties": false,
        "properties": {
            "hypothesis_statement": {"type": "string"},
            "test_plan": {"type": "array", "items": {"type": "string"}},
            "expected_observations": {"type": "array", "items": {"type": "string"}},
            "required_capabilities": {"type": "array", "items": {"type": "string"}},
            "risk_factors": {"type": "array", "items": {"type": "string"}}
        }
    })
}

fn analysis_response_schema_for(capability_id: &str) -> Option<Value> {
    match capability_id {
        "llm.analysis.function" => Some(response_schema_function()),
        "llm.analysis.type" => Some(response_schema_type()),
        "llm.analysis.class" => Some(response_schema_class()),
        "llm.analysis.subsystem" => Some(response_schema_subsystem()),
        "llm.analysis.conflict" => Some(response_schema_conflict()),
        "llm.analysis.failure" => Some(response_schema_failure()),
        "llm.experiment.design" => Some(response_schema_experiment_design()),
        _ => None,
    }
}

/// Load a generation response schema from the committed JSON Schema files.
pub fn generation_response_schema_for(capability_id: &str) -> Option<Value> {
    let raw = match capability_id {
        "llm.generation.declaration" => {
            include_str!(
                "../../../autore-reconstruction/schemas/generation/generation.declaration.schema.json"
            )
        }
        "llm.generation.type" => {
            include_str!(
                "../../../autore-reconstruction/schemas/generation/generation.type.schema.json"
            )
        }
        "llm.generation.function" => {
            include_str!(
                "../../../autore-reconstruction/schemas/generation/generation.function.schema.json"
            )
        }
        "llm.generation.cluster" => {
            include_str!(
                "../../../autore-reconstruction/schemas/generation/generation.cluster.schema.json"
            )
        }
        "llm.generation.test" => {
            include_str!(
                "../../../autore-reconstruction/schemas/generation/generation.test.schema.json"
            )
        }
        "llm.generation.repair" => {
            include_str!(
                "../../../autore-reconstruction/schemas/generation/generation.repair.schema.json"
            )
        }
        _ => return None,
    };
    serde_json::from_str(raw).ok()
}

/// Build a `CapabilityDescriptor` with embedded JSON Schema bytes.
pub fn descriptor_for(capability_id: &str) -> CapabilityDescriptor {
    let (name, request_schema, response_schema) = match capability_id {
        "llm.analysis.function" => (
            "LLM Function Analysis",
            request_schema(),
            response_schema_function(),
        ),
        "llm.analysis.type" => (
            "LLM Type Analysis",
            request_schema(),
            response_schema_type(),
        ),
        "llm.analysis.class" => (
            "LLM Class Analysis",
            request_schema(),
            response_schema_class(),
        ),
        "llm.analysis.subsystem" => (
            "LLM Subsystem Analysis",
            request_schema(),
            response_schema_subsystem(),
        ),
        "llm.analysis.conflict" => (
            "LLM Conflict Analysis",
            request_schema(),
            response_schema_conflict(),
        ),
        "llm.analysis.failure" => (
            "LLM Failure Analysis",
            request_schema(),
            response_schema_failure(),
        ),
        "llm.experiment.design" => (
            "LLM Experiment Design",
            request_schema(),
            response_schema_experiment_design(),
        ),
        "llm.generation.declaration" => (
            "LLM Declaration Generation",
            generation_request_schema(),
            generation_response_schema_for(capability_id).expect("generation.declaration schema"),
        ),
        "llm.generation.type" => (
            "LLM Type Generation",
            generation_request_schema(),
            generation_response_schema_for(capability_id).expect("generation.type schema"),
        ),
        "llm.generation.function" => (
            "LLM Function Generation",
            generation_request_schema(),
            generation_response_schema_for(capability_id).expect("generation.function schema"),
        ),
        "llm.generation.cluster" => (
            "LLM Cluster Generation",
            generation_request_schema(),
            generation_response_schema_for(capability_id).expect("generation.cluster schema"),
        ),
        "llm.generation.test" => (
            "LLM Test Generation",
            generation_request_schema(),
            generation_response_schema_for(capability_id).expect("generation.test schema"),
        ),
        "llm.generation.repair" => (
            "LLM Repair Generation",
            generation_request_schema(),
            generation_response_schema_for(capability_id).expect("generation.repair schema"),
        ),
        _ => unreachable!("unknown capability {capability_id}"),
    };
    CapabilityDescriptor {
        capability_id: capability_id.into(),
        version: "1.0.0".into(),
        name: name.into(),
        request_schema: serde_json::to_vec(&request_schema).unwrap_or_default(),
        response_schema: serde_json::to_vec(&response_schema).unwrap_or_default(),
    }
}

/// Lookup the response schema value for a capability (used at Execute time).
pub fn response_schema_for(capability_id: &str) -> Option<Value> {
    analysis_response_schema_for(capability_id)
        .or_else(|| generation_response_schema_for(capability_id))
}

/// Returns the request schema for a capability.
pub fn request_schema_for(capability_id: &str) -> Option<Value> {
    if ANALYSIS_CAPABILITIES.contains(&capability_id) {
        Some(request_schema())
    } else if GENERATION_CAPABILITIES.contains(&capability_id) {
        Some(generation_request_schema())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_schemas_compile_as_valid_json_schema() {
        for id in ANALYSIS_CAPABILITIES {
            let schema = response_schema_for(id).expect("schema defined");
            jsonschema::validator_for(&schema)
                .unwrap_or_else(|e| panic!("schema for {id} invalid: {e}"));
        }
        for id in GENERATION_CAPABILITIES {
            let schema = response_schema_for(id).expect("schema defined");
            jsonschema::validator_for(&schema)
                .unwrap_or_else(|e| panic!("schema for {id} invalid: {e}"));
        }
        jsonschema::validator_for(&request_schema()).expect("request schema valid");
        jsonschema::validator_for(&generation_request_schema())
            .expect("generation request schema valid");
    }

    #[test]
    fn descriptor_has_request_and_response_bytes() {
        for id in ANALYSIS_CAPABILITIES {
            let d = descriptor_for(id);
            assert!(!d.request_schema.is_empty(), "{id} request_schema empty");
            assert!(!d.response_schema.is_empty(), "{id} response_schema empty");
        }
        for id in GENERATION_CAPABILITIES {
            let d = descriptor_for(id);
            assert!(!d.request_schema.is_empty(), "{id} request_schema empty");
            assert!(!d.response_schema.is_empty(), "{id} response_schema empty");
        }
    }
}
