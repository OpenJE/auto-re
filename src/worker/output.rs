//! Worker output types and JSON schema validation.
//!
//! Defines `FunctionAnalysisOutput` (spec §22) — the structured result
//! a worker produces after analyzing a single function — and provides
//! `validate_output()` to verify JSON output against the generated schema.

use crate::domain::{
    Address, AddressSpace, ClaimPredicate, ClaimValue, Confidence, EvidenceKind, EvidenceLocation,
    SymbolName,
};
use crate::ids::FunctionId;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use std::borrow::Cow;

// ---------------------------------------------------------------------------
// JsonSchema impls for domain types
//
// These live here (not in `domain/`) so the domain module stays free of
// schema-generation concerns. All types are local to this crate, so
// implementing a foreign trait is valid under the orphan rule.
// ---------------------------------------------------------------------------

macro_rules! impl_schema_delegate {
    ($ty:ty, $name:literal => $inner:ty) => {
        impl JsonSchema for $ty {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed($name)
            }
            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                <$inner as JsonSchema>::json_schema(generator)
            }
        }
    };
}

// Simple newtypes delegate to their inner representation.
impl_schema_delegate!(FunctionId, "FunctionId" => String);
impl_schema_delegate!(SymbolName, "SymbolName" => String);
impl_schema_delegate!(Confidence, "Confidence" => f64);
// AddressSpace uses a custom Display/FromStr serde impl → schema is a string.
impl_schema_delegate!(AddressSpace, "AddressSpace" => String);

// Complex types use private helper structs/enums that derive JsonSchema,
// then delegate the real type's implementation to the helper.

#[derive(JsonSchema)]
#[schemars(rename = "Address")]
#[allow(dead_code)]
struct AddressRepr {
    space: AddressSpace,
    value: u128,
}

impl JsonSchema for Address {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("Address")
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        AddressRepr::json_schema(generator)
    }
}

#[derive(JsonSchema)]
#[schemars(rename = "EvidenceLocation")]
#[allow(dead_code)]
struct EvidenceLocationRepr {
    address: Option<Address>,
    path: Option<String>,
}

impl JsonSchema for EvidenceLocation {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EvidenceLocation")
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        EvidenceLocationRepr::json_schema(generator)
    }
}

#[derive(JsonSchema)]
#[schemars(rename = "ClaimPredicate")]
#[allow(dead_code)]
enum ClaimPredicateRepr {
    FunctionName,
    FunctionSignature,
    FunctionAddress,
    FunctionSize,
    TypeRecovery,
    StructureLayout,
    CallingConvention,
    ControlFlowGraph,
    DataFlowFact,
    CrossReference,
    StringReference,
    GlobalReference,
    Comment,
    RuntimeObservation,
    ReimplementationCorrectness,
    TestResult,
    Custom(String),
}

impl JsonSchema for ClaimPredicate {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ClaimPredicate")
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ClaimPredicateRepr::json_schema(generator)
    }
}

#[derive(JsonSchema)]
#[schemars(rename = "ClaimValue")]
#[allow(dead_code)]
enum ClaimValueRepr {
    String(String),
    Integer(u64),
    Float(f64),
    Boolean(bool),
    Bytes(Vec<u8>),
    TypeDescriptor(String),
    Map(Vec<(String, String)>),
    Json(serde_json::Value),
}

impl JsonSchema for ClaimValue {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ClaimValue")
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        ClaimValueRepr::json_schema(generator)
    }
}

#[derive(JsonSchema)]
#[schemars(rename = "EvidenceKind")]
#[allow(dead_code)]
enum EvidenceKindRepr {
    Decompilation,
    Disassembly,
    ControlFlowGraph,
    CallGraph,
    Trace,
    StringReference,
    GlobalReference,
    Comment,
    RuntimeObservation,
    ModelResponse,
    TypeDescriptor,
    StructureLayout,
    CallingConventionDescriptor,
    TestOutput,
    CrossReferenceListing,
    Patch,
    Screenshot,
    Custom(String),
}

impl JsonSchema for EvidenceKind {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("EvidenceKind")
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        EvidenceKindRepr::json_schema(generator)
    }
}

// ---------------------------------------------------------------------------
// Output types (spec §22)
// ---------------------------------------------------------------------------

/// A claim proposed by a worker, pending review and acceptance.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ProposedClaim {
    /// What property is being asserted.
    pub predicate: ClaimPredicate,
    /// The asserted value.
    pub value: ClaimValue,
    /// Confidence in this claim [0.0, 1.0].
    pub confidence: Confidence,
    /// Predicates of other claims this one depends on.
    #[serde(default)]
    pub dependencies: Vec<ClaimPredicate>,
}

/// Evidence proposed by a worker to support or refute a claim.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ProposedEvidence {
    /// What kind of evidence this is.
    pub kind: EvidenceKind,
    /// Optional location within the binary or source.
    pub location: Option<EvidenceLocation>,
    /// Human-readable description of the evidence.
    pub description: String,
    /// Confidence in this evidence [0.0, 1.0].
    pub confidence: Confidence,
}

/// The structured output of a worker's analysis of a single function.
///
/// This is the canonical wire format (spec §22) that workers produce
/// and the claim-conversion layer consumes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct FunctionAnalysisOutput {
    /// The function this analysis is about.
    pub function_id: FunctionId,
    /// Optional symbol name recovered from the binary.
    pub symbol_name: Option<SymbolName>,
    /// Entry address of the function.
    pub address: Address,
    /// Overall confidence in the analysis [0.0, 1.0].
    pub confidence: Confidence,
    /// Claims asserted about the function.
    pub claims: Vec<ProposedClaim>,
    /// Evidence supporting the claims.
    pub evidence: Vec<ProposedEvidence>,
    /// Optional extra fields (extensibility).
    #[serde(default)]
    pub metadata: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validates a JSON string against the `FunctionAnalysisOutput` schema.
///
/// Generates the JSON schema from `FunctionAnalysisOutput` via `schemars`,
/// validates the input against it via `jsonschema`, and on success
/// deserializes and returns the typed output.
///
/// # Errors
///
/// Returns `Error::Validation(String)` on schema mismatch. The error
/// message includes the JSON pointer path to each failing field.
pub fn validate_output(json: &str) -> crate::Result<FunctionAnalysisOutput> {
    let schema = schemars::schema_for!(FunctionAnalysisOutput);
    let schema_value = schema.as_value();

    let instance: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| crate::Error::Validation(format!("invalid JSON: {e}")))?;

    let validator = jsonschema::validator_for(schema_value)
        .map_err(|e| crate::Error::Validation(format!("schema compilation failed: {e}")))?;

    let errors: Vec<_> = validator.iter_errors(&instance).collect();
    if !errors.is_empty() {
        let messages: Vec<String> = errors
            .iter()
            .map(|e| {
                let path = e.instance_path.to_string();
                let pointer = if path.is_empty() { "/" } else { &path };
                format!("{pointer}: {e}")
            })
            .collect();
        return Err(crate::Error::Validation(messages.join("; ")));
    }

    serde_json::from_value(instance)
        .map_err(|e| crate::Error::Validation(format!("deserialization failed: {e}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AddressSpace, ClaimValue};

    fn valid_output() -> FunctionAnalysisOutput {
        FunctionAnalysisOutput {
            function_id: FunctionId::new(),
            symbol_name: Some(SymbolName::new("main")),
            address: Address::new(AddressSpace::Virtual, 0x401000),
            confidence: Confidence::new(0.9).unwrap(),
            claims: vec![ProposedClaim {
                predicate: ClaimPredicate::FunctionName,
                value: ClaimValue::String("main".into()),
                confidence: Confidence::new(0.95).unwrap(),
                dependencies: vec![],
            }],
            evidence: vec![ProposedEvidence {
                kind: EvidenceKind::Disassembly,
                location: Some(EvidenceLocation::new(
                    Some(Address::new(AddressSpace::Virtual, 0x401000)),
                    None,
                )),
                description: "Disassembly shows push rbp; mov rbp, rsp".into(),
                confidence: Confidence::new(0.8).unwrap(),
            }],
            metadata: serde_json::json!({}),
        }
    }

    #[test]
    fn valid_output_passes_schema() {
        let output = valid_output();
        let json = serde_json::to_string(&output).unwrap();
        let result = validate_output(&json);
        assert!(result.is_ok(), "valid output should pass: {result:?}");
    }

    #[test]
    fn malformed_output_fails_schema() {
        // Missing required fields: claims, evidence, address, confidence
        let json = r#"{"function_id": "not-a-uuid"}"#;
        let result = validate_output(json);
        assert!(result.is_err(), "malformed output should fail schema");
    }

    #[test]
    fn schema_error_includes_pointer() {
        // confidence is a string instead of a number
        let json = r#"{
            "function_id": "550e8400-e29b-41d4-a716-446655440000",
            "symbol_name": null,
            "address": {"space": "Virtual", "value": 42},
            "confidence": "not_a_number",
            "claims": [],
            "evidence": [],
            "metadata": {}
        }"#;
        let result = validate_output(json);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("/confidence"),
            "error should mention /confidence pointer, got: {err_msg}"
        );
    }

    #[test]
    fn output_roundtrips_via_json() {
        let original = valid_output();
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: FunctionAnalysisOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(original, deserialized);
    }
}
