//! JSON Schema definitions for the 7 analysis capabilities.
//!
//! Each capability shares a common request schema (the bounded investigation
//! bundle) and has its own response schema. Schemas are embedded in the
//! binary and exposed via `CapabilityDescriptor.request_schema` and
//! `response_schema` on Negotiate.

use serde_json::{Value, json};

use autore_provider_protocol::v1::CapabilityDescriptor;

#[cfg(test)]
const CAPABILITIES: &[&str] = &[
    "llm.analysis.function",
    "llm.analysis.type",
    "llm.analysis.class",
    "llm.analysis.subsystem",
    "llm.analysis.conflict",
    "llm.analysis.failure",
    "llm.experiment.design",
];

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

/// Build a `CapabilityDescriptor` with embedded JSON Schema bytes.
pub fn descriptor_for(capability_id: &str) -> CapabilityDescriptor {
    let (name, response_schema) = match capability_id {
        "llm.analysis.function" => ("LLM Function Analysis", response_schema_function()),
        "llm.analysis.type" => ("LLM Type Analysis", response_schema_type()),
        "llm.analysis.class" => ("LLM Class Analysis", response_schema_class()),
        "llm.analysis.subsystem" => ("LLM Subsystem Analysis", response_schema_subsystem()),
        "llm.analysis.conflict" => ("LLM Conflict Analysis", response_schema_conflict()),
        "llm.analysis.failure" => ("LLM Failure Analysis", response_schema_failure()),
        "llm.experiment.design" => ("LLM Experiment Design", response_schema_experiment_design()),
        _ => unreachable!("unknown capability {capability_id}"),
    };
    CapabilityDescriptor {
        capability_id: capability_id.into(),
        version: "1.0.0".into(),
        name: name.into(),
        request_schema: serde_json::to_vec(&request_schema()).unwrap_or_default(),
        response_schema: serde_json::to_vec(&response_schema).unwrap_or_default(),
    }
}

/// Lookup the response schema value for a capability (used at Execute time).
pub fn response_schema_for(capability_id: &str) -> Option<Value> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_schemas_compile_as_valid_json_schema() {
        for id in CAPABILITIES {
            let schema = response_schema_for(id).expect("schema defined");
            jsonschema::validator_for(&schema)
                .unwrap_or_else(|e| panic!("schema for {id} invalid: {e}"));
        }
        jsonschema::validator_for(&request_schema()).expect("request schema valid");
    }

    #[test]
    fn descriptor_has_request_and_response_bytes() {
        for id in CAPABILITIES {
            let d = descriptor_for(id);
            assert!(!d.request_schema.is_empty(), "{id} request_schema empty");
            assert!(!d.response_schema.is_empty(), "{id} response_schema empty");
        }
    }
}
