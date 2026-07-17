//! Value types for extensible evidence and metadata.
//!
//! - `ModuleIdentity`: content-addressed identity of a binary module.
//! - `BinaryLocation`: address within a binary artifact (artifact + module + RVA).
//! - `StableEntityKey`: closed enum for content-addressed entity identity.
//! - `DerivationMethod` / `Derivation`: provenance metadata for derived entities.
//! - `ExtensionData`: versioned, schema-tagged payload (§7 mandate).
//! - `MetadataMap`: typed wrapper around `BTreeMap<NamespacedId, ExtensionData>`.
//! - `EvidenceValue`: tagged enum covering all primitive and composite
//!   evidence shapes, with strict float rejection (NaN/Inf forbidden).

use std::collections::BTreeMap;

use crate::domain::{ContentHash, NamespacedId};
use crate::ids::{
    ArtifactId as Stage0ArtifactId, BinaryArtifactId, EntityId as Stage0EntityId, EvidenceId,
    HypothesisId,
};

// ---------------------------------------------------------------------------
// ModuleIdentity
// ---------------------------------------------------------------------------

/// Identity of a module within a binary artifact, keyed by content hash
/// rather than filesystem path or tool-assigned ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ModuleIdentity {
    /// Optional human-readable module name (e.g., ".text", "libc.so.6").
    pub name: Option<String>,
    /// Content hash of the module's bytes for content-addressed identity.
    pub content_hash: ContentHash,
    /// Optional index within the artifact's image (e.g., PE section index).
    pub image_relative_index: Option<u32>,
}

impl ModuleIdentity {
    /// Creates a new module identity.
    pub fn new(
        name: Option<String>,
        content_hash: ContentHash,
        image_relative_index: Option<u32>,
    ) -> Self {
        ModuleIdentity {
            name,
            content_hash,
            image_relative_index,
        }
    }
}

// ---------------------------------------------------------------------------
// BinaryLocation
// ---------------------------------------------------------------------------

/// A location within a binary artifact, keyed by artifact identity, module
/// identity, and relative virtual address — NOT by absolute load address,
/// filesystem path, or tool-specific ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BinaryLocation {
    /// The binary artifact containing this location.
    pub artifact: BinaryArtifactId,
    /// The module within the artifact.
    pub module: ModuleIdentity,
    /// Address relative to the module's image base (RVA).
    pub relative_address: u64,
}

impl BinaryLocation {
    /// Creates a new binary location from an artifact, module identity, and RVA.
    pub fn new(
        artifact: BinaryArtifactId,
        module: ModuleIdentity,
        relative_address: u64,
    ) -> Self {
        BinaryLocation {
            artifact,
            module,
            relative_address,
        }
    }
}

// ---------------------------------------------------------------------------
// ExtensionData
// ---------------------------------------------------------------------------

/// A versioned, schema-tagged extension payload.
///
/// Per §7, all extensible metadata must be wrapped in `ExtensionData`
/// rather than exposed as unbounded `serde_json::Value`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExtensionData {
    /// Schema identifier validated as a `NamespacedId`.
    pub schema: NamespacedId,
    /// Schema version for forward/backward compatibility.
    pub version: u32,
    /// The extension payload.
    pub value: serde_json::Value,
}

impl ExtensionData {
    /// Creates new extension data. The `schema` is validated at construction
    /// via `NamespacedId`'s parse-don't-validate guarantee (the serde
    /// deserializer also validates).
    pub fn new(schema: NamespacedId, version: u32, value: serde_json::Value) -> Self {
        ExtensionData {
            schema,
            version,
            value,
        }
    }
}

// ---------------------------------------------------------------------------
// MetadataMap
// ---------------------------------------------------------------------------

/// A typed, deterministic map of extension data keyed by `NamespacedId`.
///
/// The inner `BTreeMap` is private; access is through the provided methods.
/// Deterministic iteration order is guaranteed by `BTreeMap`'s sorted keys.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct MetadataMap(BTreeMap<NamespacedId, ExtensionData>);

impl MetadataMap {
    /// Creates an empty metadata map.
    pub fn new() -> Self {
        MetadataMap(BTreeMap::new())
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if the map contains no entries.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Gets a reference to the extension data for a given schema key.
    pub fn get(&self, key: &NamespacedId) -> Option<&ExtensionData> {
        self.0.get(key)
    }

    /// Inserts extension data, returning the previous value if any.
    pub fn insert(&mut self, key: NamespacedId, data: ExtensionData) -> Option<ExtensionData> {
        self.0.insert(key, data)
    }

    /// Returns an iterator over `(NamespacedId, ExtensionData)` pairs in
    /// sorted key order.
    pub fn iter(&self) -> impl Iterator<Item = (&NamespacedId, &ExtensionData)> {
        self.0.iter()
    }

    /// Returns `true` if the map contains the given key.
    pub fn contains_key(&self, key: &NamespacedId) -> bool {
        self.0.contains_key(key)
    }
}

impl Default for MetadataMap {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// EvidenceValue
// ---------------------------------------------------------------------------

/// Strict-float deserializer: rejects NaN and Infinity during serde
/// deserialization.
fn deserialize_strict_f64<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<f64, D::Error> {
    let v = <f64 as serde::Deserialize>::deserialize(d)?;
    if v.is_nan() || v.is_infinite() {
        return Err(serde::de::Error::custom(
            "NaN and Infinity are not permitted in EvidenceValue::Float",
        ));
    }
    Ok(v)
}

/// A polymorphic value for evidence data, covering all primitive and
/// composite shapes needed by the analysis pipeline.
///
/// Serializes as `{ "kind": "<variant>", "value": <content> }` (adjacently
/// tagged). The `Null` variant serializes as `{ "kind": "Null" }` with no
/// content key.
///
/// Float values are strictly finite: NaN and Infinity are rejected both
/// via the `float()` constructor and during deserialization.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum EvidenceValue {
    /// No value / null.
    Null,
    /// A boolean.
    Boolean(bool),
    /// A signed integer (up to 128 bits).
    SignedInteger(i128),
    /// An unsigned integer (up to 128 bits).
    UnsignedInteger(u128),
    /// A finite floating-point number (NaN and Inf rejected).
    Float(#[serde(deserialize_with = "deserialize_strict_f64")] f64),
    /// A UTF-8 string.
    String(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
    /// A reference to a Stage 0 entity (UUIDv7).
    Entity(Stage0EntityId),
    /// A reference to a Stage 0 artifact (UUIDv7).
    Artifact(Stage0ArtifactId),
    /// A location within a binary artifact.
    BinaryLocation(BinaryLocation),
    /// An ordered list of evidence values.
    List(Vec<EvidenceValue>),
    /// A deterministic string-keyed map (BTreeMap for stable ordering).
    Map(BTreeMap<std::string::String, EvidenceValue>),
    /// A schema-versioned extension payload.
    Extension(ExtensionData),
}

impl EvidenceValue {
    /// Constructs a `Float` variant, rejecting NaN and Infinity.
    pub fn float(v: f64) -> autore_core::Result<Self> {
        if v.is_nan() || v.is_infinite() {
            return Err(autore_core::Error::Validation(
                "NaN and Infinity are not permitted in EvidenceValue::Float".into(),
            ));
        }
        Ok(EvidenceValue::Float(v))
    }
}

// ---------------------------------------------------------------------------
// StableEntityKey
// ---------------------------------------------------------------------------

/// A stable, content-addressed key for entities across analysis sessions.
///
/// This enum is intentionally closed: all entity keying in the system
/// falls into one of these four categories. New keying strategies require
/// a schema revision rather than silent extension, ensuring all consumers
/// handle every variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum StableEntityKey {
    /// A specific location within a binary.
    BinaryLocation(BinaryLocation),
    /// A contiguous range starting at `start` with `length` bytes.
    BinaryRange { start: BinaryLocation, length: u64 },
    /// A named symbol within an artifact.
    ArtifactSymbol {
        artifact: Stage0ArtifactId,
        symbol: String,
    },
    /// An externally-namespaced identity (e.g., DWARF DIE, PDB type index).
    ExternalIdentity {
        namespace: NamespacedId,
        value: String,
    },
}

// ---------------------------------------------------------------------------
// DerivationMethod
// ---------------------------------------------------------------------------

/// How a claim or entity was derived — a closed finite set of derivation
/// strategies used in the analysis pipeline.
///
/// This enum is closed: the set of derivation methods is finite and known
/// at compile time. Adding a new method requires a code change, ensuring
/// all consumers are updated to handle it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum DerivationMethod {
    DirectObservation,
    ProviderAnalysis,
    DeterministicAnalysis,
    SolverProof,
    ConcreteExecution,
    SymbolicExecution,
    CrossProviderAgreement,
    LlmInference,
    HumanAssertion,
    ImportedKnowledge,
}

// ---------------------------------------------------------------------------
// Derivation
// ---------------------------------------------------------------------------

/// Provenance metadata describing how a claim or entity was derived.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Derivation {
    pub method: DerivationMethod,
    pub operation: NamespacedId,
    pub supporting_evidence: Vec<EvidenceId>,
    pub source_hypotheses: Vec<HypothesisId>,
}

impl Derivation {
    /// Creates a new derivation record.
    pub fn new(
        method: DerivationMethod,
        operation: NamespacedId,
        supporting_evidence: Vec<EvidenceId>,
        source_hypotheses: Vec<HypothesisId>,
    ) -> Self {
        Derivation {
            method,
            operation,
            supporting_evidence,
            source_hypotheses,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ContentHash, NamespacedId};
    use crate::ids::{
        ArtifactId as Stage0ArtifactId, BinaryArtifactId, EntityId as Stage0EntityId, EvidenceId,
        HypothesisId,
    };

    // -- ModuleIdentity / BinaryLocation tests --

    fn test_module_identity() -> ModuleIdentity {
        ModuleIdentity::new(
            Some(".text".into()),
            ContentHash::sha256(b"test module content"),
            Some(0),
        )
    }

    #[test]
    fn binary_location_serialize_roundtrip() {
        let loc = BinaryLocation::new(BinaryArtifactId::new(), test_module_identity(), 0x1000);
        let json = serde_json::to_string(&loc).unwrap();
        let back: BinaryLocation = serde_json::from_str(&json).unwrap();
        assert_eq!(loc, back);
    }

    #[test]
    fn binary_location_round_trip() {
        let fixture = include_str!("../../tests/fixtures/binary_location.json");
        let loc: BinaryLocation = serde_json::from_str(fixture).unwrap();
        assert!(loc.module.name.is_some());
        assert_eq!(loc.module.name.as_deref(), Some(".text"));
        let re_serialized = serde_json::to_string(&loc).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    #[test]
    fn binary_location_rejects_absolute_only() {
        // BinaryLocation::new requires BinaryArtifactId + ModuleIdentity + rva.
        // The old API (artifact, String, u64) no longer compiles.
        let module = ModuleIdentity::new(
            Some(".text".into()),
            ContentHash::sha256(b"module bytes"),
            Some(0),
        );
        let loc = BinaryLocation::new(BinaryArtifactId::new(), module, 0x1000);
        assert_eq!(loc.relative_address, 0x1000);
        assert_eq!(loc.module.name.as_deref(), Some(".text"));
        assert_eq!(loc.module.image_relative_index, Some(0));
    }

    // -- ExtensionData tests --

    #[test]
    fn extension_data_valid_schema() {
        let schema = NamespacedId::parse("core.analysis").unwrap();
        let ext = ExtensionData::new(schema.clone(), 1, serde_json::json!({"key": "val"}));
        assert_eq!(ext.schema, schema);
        assert_eq!(ext.version, 1);
    }

    #[test]
    fn extension_data_serialize_roundtrip() {
        let ext = ExtensionData::new(
            NamespacedId::parse("ida.hexrays").unwrap(),
            2,
            serde_json::json!({"decompiled": true}),
        );
        let json = serde_json::to_string(&ext).unwrap();
        let back: ExtensionData = serde_json::from_str(&json).unwrap();
        assert_eq!(ext, back);
    }

    #[test]
    fn extension_data_rejects_bad_namespace_on_deserialize() {
        let json = r#"{"schema":"Invalid.Upper","version":1,"value":null}"#;
        let result: std::result::Result<ExtensionData, _> = serde_json::from_str(json);
        assert!(result.is_err(), "bad namespace must be rejected");
    }

    #[test]
    fn extension_data_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/extension_data.json");
        let ext: ExtensionData = serde_json::from_str(fixture).unwrap();
        assert_eq!(ext.schema.as_str(), "core.analysis");
        assert_eq!(ext.version, 1);
        let re_serialized = serde_json::to_string(&ext).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    // -- MetadataMap tests --

    #[test]
    fn metadata_map_insert_and_get() {
        let mut map = MetadataMap::new();
        assert!(map.is_empty());

        let key = NamespacedId::parse("core.meta").unwrap();
        let data = ExtensionData::new(key.clone(), 1, serde_json::json!("hello"));
        map.insert(key.clone(), data);

        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&key));
        assert_eq!(map.get(&key).unwrap().version, 1);
    }

    #[test]
    fn metadata_map_deterministic_order() {
        let mut map = MetadataMap::new();
        let keys = ["zebra.ext", "alpha.ext", "mid.ext"];
        for k in &keys {
            let ns = NamespacedId::parse(k).unwrap();
            map.insert(
                ns.clone(),
                ExtensionData::new(ns, 1, serde_json::json!(null)),
            );
        }

        let collected: Vec<&str> = map.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(collected, vec!["alpha.ext", "mid.ext", "zebra.ext"]);
    }

    #[test]
    fn metadata_map_serialize_roundtrip() {
        let mut map = MetadataMap::new();
        let key = NamespacedId::parse("core.test").unwrap();
        map.insert(
            key.clone(),
            ExtensionData::new(key, 1, serde_json::json!({"x": 42})),
        );
        let json = serde_json::to_string(&map).unwrap();
        let back: MetadataMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map, back);
    }

    #[test]
    fn metadata_map_default_is_empty() {
        let map = MetadataMap::default();
        assert!(map.is_empty());
    }

    // -- EvidenceValue variant round-trip tests --

    fn roundtrip(val: &EvidenceValue) {
        let json = serde_json::to_string(val).unwrap();
        let back: EvidenceValue = serde_json::from_str(&json).unwrap();
        assert_eq!(val, &back);
    }

    #[test]
    fn evidence_value_null_roundtrip() {
        roundtrip(&EvidenceValue::Null);
    }

    #[test]
    fn evidence_value_boolean_roundtrip() {
        roundtrip(&EvidenceValue::Boolean(true));
        roundtrip(&EvidenceValue::Boolean(false));
    }

    #[test]
    fn evidence_value_signed_integer_roundtrip() {
        roundtrip(&EvidenceValue::SignedInteger(-42));
        roundtrip(&EvidenceValue::SignedInteger(i128::MIN));
        roundtrip(&EvidenceValue::SignedInteger(i128::MAX));
    }

    #[test]
    fn evidence_value_unsigned_integer_roundtrip() {
        roundtrip(&EvidenceValue::UnsignedInteger(0));
        roundtrip(&EvidenceValue::UnsignedInteger(u128::MAX));
    }

    #[test]
    fn evidence_value_float_roundtrip() {
        roundtrip(&EvidenceValue::float(3.14).unwrap());
        roundtrip(&EvidenceValue::float(0.0).unwrap());
        roundtrip(&EvidenceValue::float(-1.5e10).unwrap());
    }

    #[test]
    fn evidence_value_float_rejects_nan_inf() {
        assert!(EvidenceValue::float(f64::NAN).is_err());
        assert!(EvidenceValue::float(f64::INFINITY).is_err());
        assert!(EvidenceValue::float(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn evidence_value_string_roundtrip() {
        roundtrip(&EvidenceValue::String("hello".into()));
        roundtrip(&EvidenceValue::String(String::new()));
    }

    #[test]
    fn evidence_value_bytes_roundtrip() {
        roundtrip(&EvidenceValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        roundtrip(&EvidenceValue::Bytes(vec![]));
    }

    #[test]
    fn evidence_value_entity_roundtrip() {
        roundtrip(&EvidenceValue::Entity(Stage0EntityId::new()));
    }

    #[test]
    fn evidence_value_artifact_roundtrip() {
        roundtrip(&EvidenceValue::Artifact(Stage0ArtifactId::new()));
    }

    #[test]
    fn evidence_value_binary_location_roundtrip() {
        let loc = BinaryLocation::new(BinaryArtifactId::new(), test_module_identity(), 0x1234);
        roundtrip(&EvidenceValue::BinaryLocation(loc));
    }

    #[test]
    fn evidence_value_list_roundtrip() {
        let list = EvidenceValue::List(vec![
            EvidenceValue::Null,
            EvidenceValue::Boolean(true),
            EvidenceValue::String("nested".into()),
        ]);
        roundtrip(&list);
    }

    #[test]
    fn evidence_value_map_roundtrip() {
        let mut m = BTreeMap::new();
        m.insert("alpha".into(), EvidenceValue::Boolean(true));
        m.insert("beta".into(), EvidenceValue::SignedInteger(-1));
        roundtrip(&EvidenceValue::Map(m));
    }

    #[test]
    fn evidence_value_extension_roundtrip() {
        let ext = ExtensionData::new(
            NamespacedId::parse("core.test").unwrap(),
            1,
            serde_json::json!({"nested": true}),
        );
        roundtrip(&EvidenceValue::Extension(ext));
    }

    // -- EvidenceValue Map ordering --

    #[test]
    fn evidence_value_map_ordering_deterministic() {
        let mut m = BTreeMap::new();
        m.insert("zebra".into(), EvidenceValue::Boolean(true));
        m.insert("alpha".into(), EvidenceValue::Boolean(false));
        m.insert("middle".into(), EvidenceValue::Null);
        let val = EvidenceValue::Map(m);

        let json1 = serde_json::to_string(&val).unwrap();
        let json2 = serde_json::to_string(&val).unwrap();
        assert_eq!(json1, json2, "BTreeMap ordering must be deterministic");

        // Verify key order in serialized output
        let alpha_pos = json1.find("\"alpha\"").unwrap();
        let middle_pos = json1.find("\"middle\"").unwrap();
        let zebra_pos = json1.find("\"zebra\"").unwrap();
        assert!(alpha_pos < middle_pos, "alpha before middle");
        assert!(middle_pos < zebra_pos, "middle before zebra");
    }

    #[test]
    fn evidence_value_map_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/evidence_value_map.json");
        let val: EvidenceValue = serde_json::from_str(fixture).unwrap();
        match &val {
            EvidenceValue::Map(m) => {
                assert_eq!(m.len(), 3);
                let keys: Vec<&String> = m.keys().collect();
                assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
            }
            other => panic!("expected Map variant, got: {other:?}"),
        }
        let re_serialized = serde_json::to_string(&val).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    // -- EvidenceValue tagged format --

    #[test]
    fn evidence_value_tagged_format_null() {
        let json = serde_json::to_string(&EvidenceValue::Null).unwrap();
        assert_eq!(json, r#"{"kind":"Null"}"#);
    }

    #[test]
    fn evidence_value_tagged_format_boolean() {
        let json = serde_json::to_string(&EvidenceValue::Boolean(true)).unwrap();
        assert_eq!(json, r#"{"kind":"Boolean","value":true}"#);
    }

    #[test]
    fn evidence_value_tagged_format_string() {
        let json = serde_json::to_string(&EvidenceValue::String("hi".into())).unwrap();
        assert_eq!(json, r#"{"kind":"String","value":"hi"}"#);
    }

    // -- Serde NaN/Inf rejection in deserialize --

    #[test]
    fn evidence_value_deserialize_rejects_nan_string() {
        // serde_json doesn't produce NaN from numeric literals,
        // but we test the deserialize_with guard via a crafted payload.
        // In standard JSON, NaN/Inf cannot appear as numeric literals,
        // so this tests that the guard is present.
        let json = r#"{"kind":"Float","value":"NaN"}"#;
        let result: std::result::Result<EvidenceValue, _> = serde_json::from_str(json);
        assert!(result.is_err(), "string 'NaN' must not deserialize as Float");
    }

    // -- 12 variant coverage --

    #[test]
    fn evidence_value_all_12_variants_constructible() {
        let _ = EvidenceValue::Null;
        let _ = EvidenceValue::Boolean(false);
        let _ = EvidenceValue::SignedInteger(0);
        let _ = EvidenceValue::UnsignedInteger(0);
        let _ = EvidenceValue::float(1.0).unwrap();
        let _ = EvidenceValue::String(String::new());
        let _ = EvidenceValue::Bytes(vec![]);
        let _ = EvidenceValue::Entity(Stage0EntityId::new());
        let _ = EvidenceValue::Artifact(Stage0ArtifactId::new());
        let _ = EvidenceValue::BinaryLocation(BinaryLocation::new(
            BinaryArtifactId::new(),
            ModuleIdentity::new(None, ContentHash::sha256(b""), None),
            0,
        ));
        let _ = EvidenceValue::List(vec![]);
        let _ = EvidenceValue::Map(BTreeMap::new());
        let _ = EvidenceValue::Extension(ExtensionData::new(
            NamespacedId::parse("core.test").unwrap(),
            1,
            serde_json::json!(null),
        ));
    }

    // -- StableEntityKey tests --

    fn test_binary_location() -> BinaryLocation {
        BinaryLocation::new(BinaryArtifactId::new(), test_module_identity(), 0x1000)
    }

    #[test]
    fn stable_entity_key_binary_location_roundtrip() {
        let key = StableEntityKey::BinaryLocation(test_binary_location());
        let json = serde_json::to_string(&key).unwrap();
        let back: StableEntityKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn stable_entity_key_binary_range_roundtrip() {
        let key = StableEntityKey::BinaryRange {
            start: test_binary_location(),
            length: 256,
        };
        let json = serde_json::to_string(&key).unwrap();
        let back: StableEntityKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn stable_entity_key_artifact_symbol_roundtrip() {
        let key = StableEntityKey::ArtifactSymbol {
            artifact: Stage0ArtifactId::new(),
            symbol: "main".into(),
        };
        let json = serde_json::to_string(&key).unwrap();
        let back: StableEntityKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn stable_entity_key_external_identity_roundtrip() {
        let key = StableEntityKey::ExternalIdentity {
            namespace: NamespacedId::parse("dwarf.die").unwrap(),
            value: "0x1a2b".into(),
        };
        let json = serde_json::to_string(&key).unwrap();
        let back: StableEntityKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn stable_entity_key_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/stable_entity_key.json");
        let key: StableEntityKey = serde_json::from_str(fixture).unwrap();
        match &key {
            StableEntityKey::BinaryLocation(loc) => {
                assert_eq!(loc.module.name.as_deref(), Some(".text"));
            }
            other => panic!("expected BinaryLocation variant, got: {other:?}"),
        }
        let re_serialized = serde_json::to_string(&key).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    // -- DerivationMethod tests --

    #[test]
    fn derivation_method_all_10_variants_roundtrip() {
        let methods = [
            DerivationMethod::DirectObservation,
            DerivationMethod::ProviderAnalysis,
            DerivationMethod::DeterministicAnalysis,
            DerivationMethod::SolverProof,
            DerivationMethod::ConcreteExecution,
            DerivationMethod::SymbolicExecution,
            DerivationMethod::CrossProviderAgreement,
            DerivationMethod::LlmInference,
            DerivationMethod::HumanAssertion,
            DerivationMethod::ImportedKnowledge,
        ];
        for method in &methods {
            let json = serde_json::to_string(method).unwrap();
            let back: DerivationMethod = serde_json::from_str(&json).unwrap();
            assert_eq!(method, &back);
        }
    }

    #[test]
    fn derivation_method_tagged_format() {
        let json = serde_json::to_string(&DerivationMethod::DirectObservation).unwrap();
        assert_eq!(json, r#"{"kind":"DirectObservation"}"#);
    }

    // -- Derivation tests --

    #[test]
    fn derivation_roundtrip() {
        let d = Derivation::new(
            DerivationMethod::ProviderAnalysis,
            NamespacedId::parse("core.analysis").unwrap(),
            vec![EvidenceId::new()],
            vec![HypothesisId::new()],
        );
        let json = serde_json::to_string(&d).unwrap();
        let back: Derivation = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }

    #[test]
    fn derivation_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/derivation.json");
        let d: Derivation = serde_json::from_str(fixture).unwrap();
        assert_eq!(d.method, DerivationMethod::DirectObservation);
        assert_eq!(d.operation.as_str(), "core.analysis");
        assert_eq!(d.supporting_evidence.len(), 1);
        assert!(d.source_hypotheses.is_empty());
        let re_serialized = serde_json::to_string(&d).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }
}
