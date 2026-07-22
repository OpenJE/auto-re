//! Spec §8.6 validation invariants for parsed LLM responses.

use std::collections::HashSet;

use serde_json::Value;

use crate::analysis::bundle::InvestigationBundle;
use crate::analysis::schemas::response_schema_for;

/// Maximum parsed response size (1 MiB).
const PARSED_RESPONSE_MAX_BYTES: usize = 1024 * 1024;

/// Collects all validation errors from a parsed LLM response.
pub struct ImportValidation {
    pub errors: Vec<String>,
}

impl ImportValidation {
    /// Runs all validation checks and collects error messages.
    pub fn validate(capability_id: &str, parsed: &Value, bundle: &InvestigationBundle) -> Self {
        let mut errors = Vec::new();

        // 1. Parsed result JSON size ≤ 1 MiB.
        let size = serde_json::to_vec(parsed).map(|v| v.len()).unwrap_or(0);
        if size > PARSED_RESPONSE_MAX_BYTES {
            errors.push(format!(
                "parsed response size {size} exceeds limit {PARSED_RESPONSE_MAX_BYTES}"
            ));
        }

        // 2. Schema validation.
        if let Some(schema) = response_schema_for(capability_id) {
            if let Ok(validator) = jsonschema::validator_for(&schema) {
                for err in validator.iter_errors(parsed) {
                    errors.push(format!("schema: {err}"));
                }
            } else {
                errors.push(format!("schema compilation failed for {capability_id}"));
            }
        } else {
            errors.push(format!("unknown capability: {capability_id}"));
        }

        // 3. Entity existence: evidence_references must reference bundle entities.
        let known_ids = collect_known_ids(bundle);
        if let Some(refs) = parsed.get("evidence_references").and_then(Value::as_array) {
            for r in refs {
                if let Some(s) = r.as_str()
                    && !known_ids.contains(s)
                {
                    errors.push(format!("evidence_reference '{s}' not found in bundle"));
                }
            }
        }

        // 4. Confidence in [0.0, 1.0].
        if let Some(conf) = parsed.get("confidence").and_then(Value::as_f64)
            && !(0.0..=1.0).contains(&conf)
        {
            errors.push(format!("confidence {conf} outside [0.0, 1.0]"));
        }

        // 5. Type analysis: struct sizes and offsets within 1..4096.
        if capability_id == "llm.analysis.type" {
            validate_type_invariants(parsed, &mut errors);
        }

        // 6. Experiment design: only debug.* capability names.
        if capability_id == "llm.experiment.design" {
            validate_experiment_capabilities(parsed, &mut errors);
        }

        Self { errors }
    }

    /// Returns `true` if no validation errors were found.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Collects all string-representable IDs from the bundle that
/// `evidence_references` may legally reference.
fn collect_known_ids(bundle: &InvestigationBundle) -> HashSet<String> {
    let mut ids = HashSet::new();

    if let Some(eid) = bundle.subject_entity_id {
        ids.insert(eid.to_string());
    }
    for eid in &bundle.relevant_types {
        ids.insert(eid.to_string());
    }
    for eid in &bundle.relevant_globals {
        ids.insert(eid.to_string());
    }
    for cs in &bundle.callers_and_callees {
        ids.insert(cs.work_item_id.to_string());
    }
    for hid in &bundle.accepted_hypotheses {
        ids.insert(hid.to_string());
    }
    for cid in &bundle.unresolved_conflicts {
        ids.insert(cid.to_string());
    }
    ids
}

/// Validates that type-analysis layout sizes and offsets are within 1..4096.
fn validate_type_invariants(parsed: &Value, errors: &mut Vec<String>) {
    if let Some(size) = parsed.get("size_bytes").and_then(Value::as_i64)
        && !(1..=4096).contains(&size)
    {
        errors.push(format!("size_bytes {size} outside 1..4096"));
    }
    if let Some(layout) = parsed.get("proposed_layout").and_then(Value::as_array) {
        for (i, field) in layout.iter().enumerate() {
            if let Some(offset) = field.get("offset").and_then(Value::as_i64)
                && !(0..4096).contains(&offset)
            {
                errors.push(format!("field[{i}].offset {offset} outside 0..4096"));
            }
            if let Some(size) = field.get("size").and_then(Value::as_i64)
                && !(1..=4096).contains(&size)
            {
                errors.push(format!("field[{i}].size {size} outside 1..4096"));
            }
        }
    }
}

/// Validates that experiment proposals reference only `debug.*` capabilities.
fn validate_experiment_capabilities(parsed: &Value, errors: &mut Vec<String>) {
    if let Some(caps) = parsed
        .get("required_capabilities")
        .and_then(Value::as_array)
    {
        for cap in caps {
            if let Some(name) = cap.as_str()
                && !name.starts_with("debug.")
            {
                errors.push(format!(
                    "experiment capability '{name}' is not a debug.* capability"
                ));
            }
        }
    }
}
