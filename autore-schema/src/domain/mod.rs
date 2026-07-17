//! Domain primitives — core value types for the auto-re system.
//!
//! These types are parse-don't-validate: once constructed, they
//! are guaranteed to be valid for their domain.

use autore_core::{Error, Result};
use crate::ids::WorkerRunId;

// ---------------------------------------------------------------------------
// Address
// ---------------------------------------------------------------------------

/// A memory address within a specific address space.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Address {
    pub space: AddressSpace,
    pub value: u128,
}

impl Address {
    /// Creates an address from a space and value.
    pub fn new(space: AddressSpace, value: u128) -> Self {
        Address { space, value }
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{:#x}", self.space, self.value)
    }
}

// ---------------------------------------------------------------------------
// AddressSpace
// ---------------------------------------------------------------------------

/// The address space an `Address` belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AddressSpace {
    Virtual,
    RelativeVirtual,
    FileOffset,
    Physical,
    Custom(String),
}

impl std::fmt::Display for AddressSpace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddressSpace::Virtual => write!(f, "Virtual"),
            AddressSpace::RelativeVirtual => write!(f, "RelativeVirtual"),
            AddressSpace::FileOffset => write!(f, "FileOffset"),
            AddressSpace::Physical => write!(f, "Physical"),
            AddressSpace::Custom(s) => write!(f, "Custom({s})"),
        }
    }
}

impl serde::Serialize for AddressSpace {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for AddressSpace {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AddressSpaceVisitor;

        impl<'de> serde::de::Visitor<'de> for AddressSpaceVisitor {
            type Value = AddressSpace;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an address space variant string")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<AddressSpace, E> {
                match v {
                    "Virtual" => Ok(AddressSpace::Virtual),
                    "RelativeVirtual" => Ok(AddressSpace::RelativeVirtual),
                    "FileOffset" => Ok(AddressSpace::FileOffset),
                    "Physical" => Ok(AddressSpace::Physical),
                    s if s.starts_with("Custom(") && s.ends_with(')') => {
                        let inner = &s[7..s.len() - 1];
                        Ok(AddressSpace::Custom(inner.to_string()))
                    }
                    _ => Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Str(v),
                        &self,
                    )),
                }
            }
        }

        deserializer.deserialize_str(AddressSpaceVisitor)
    }
}

// ---------------------------------------------------------------------------
// HashAlgorithm
// ---------------------------------------------------------------------------

/// Supported hash algorithms for content-addressed data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Blake3,
    Sha256,
}

impl std::fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashAlgorithm::Blake3 => write!(f, "blake3"),
            HashAlgorithm::Sha256 => write!(f, "sha256"),
        }
    }
}

// ---------------------------------------------------------------------------
// ContentHash
// ---------------------------------------------------------------------------

/// A content-addressed hash of binary data, tagged with its algorithm.
///
/// Serializes as `{ "algorithm": "<algo>", "digest": "<hex>" }`.
/// The `from_bytes` convenience constructor computes BLAKE3 for backward
/// compatibility with existing call sites.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ContentHash {
    pub algorithm: HashAlgorithm,
    #[serde(
        serialize_with = "serialize_hex",
        deserialize_with = "deserialize_hex"
    )]
    pub digest: Vec<u8>,
}

fn serialize_hex<S: serde::Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&bytes_to_hex(bytes))
}

fn deserialize_hex<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    hex_to_bytes(&s).map_err(serde::de::Error::custom)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex string has odd length".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("invalid hex: {e}"))
        })
        .collect()
}

impl ContentHash {
    /// Computes a SHA-256 hash from raw bytes.
    pub fn sha256(data: &[u8]) -> Self {
        use sha2::Digest;
        ContentHash {
            algorithm: HashAlgorithm::Sha256,
            digest: sha2::Sha256::digest(data).to_vec(),
        }
    }

    /// Computes a BLAKE3 hash from raw bytes.
    pub fn blake3(data: &[u8]) -> Self {
        ContentHash {
            algorithm: HashAlgorithm::Blake3,
            digest: blake3::hash(data).as_bytes().to_vec(),
        }
    }

    /// Convenience alias for `blake3(data)` — preserves existing call sites.
    pub fn from_bytes(data: &[u8]) -> Self {
        Self::blake3(data)
    }

    /// Returns the hex-encoded digest string.
    pub fn digest_hex(&self) -> String {
        bytes_to_hex(&self.digest)
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest_hex())
    }
}

// ---------------------------------------------------------------------------
// NamespacedId
// ---------------------------------------------------------------------------

/// Error returned when a `NamespacedId` fails validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespacedIdError(pub String);

impl std::fmt::Display for NamespacedIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid namespaced ID: {}", self.0)
    }
}

impl std::error::Error for NamespacedIdError {}

impl From<NamespacedIdError> for autore_core::Error {
    fn from(e: NamespacedIdError) -> Self {
        autore_core::Error::Validation(e.to_string())
    }
}

/// A dot-separated, lowercase ASCII identifier for extensible schema namespacing.
///
/// Valid: `core.function`, `ida.hexrays.pseudocode`
/// Invalid: `Core.Function`, `core..function`, `.core`, `core/`, ` core`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamespacedId(String);

impl NamespacedId {
    /// Parses a string into a validated `NamespacedId`.
    pub fn parse(s: &str) -> Result<Self, NamespacedIdError> {
        Self::validate(s)?;
        Ok(NamespacedId(s.to_owned()))
    }

    /// Constructs from pre-split segments (e.g. `&["core", "function"]`).
    pub fn new(segments: &[&str]) -> Result<Self, NamespacedIdError> {
        if segments.is_empty() {
            return Err(NamespacedIdError("no segments provided".into()));
        }
        let joined = segments.join(".");
        Self::validate(&joined)?;
        Ok(NamespacedId(joined))
    }

    /// Returns the string form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate(s: &str) -> Result<(), NamespacedIdError> {
        if s.is_empty() {
            return Err(NamespacedIdError("empty string".into()));
        }
        if s.starts_with('.') || s.ends_with('.') {
            return Err(NamespacedIdError(
                "leading or trailing dot".into(),
            ));
        }
        if s.contains('/') || s.contains('\\') {
            return Err(NamespacedIdError("path separator not allowed".into()));
        }
        if s.chars().any(|c| c.is_whitespace()) {
            return Err(NamespacedIdError("whitespace not allowed".into()));
        }
        for segment in s.split('.') {
            if segment.is_empty() {
                return Err(NamespacedIdError("empty segment".into()));
            }
            if segment.chars().any(|c| !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_' && c != '-') {
                return Err(NamespacedIdError(format!(
                    "segment '{segment}' contains invalid characters (must be lowercase ASCII, digits, underscores, or hyphens)"
                )));
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for NamespacedId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for NamespacedId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for NamespacedId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        NamespacedId::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// SchemaVersion
// ---------------------------------------------------------------------------

/// A semantic schema version (`major.minor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SchemaVersion {
    pub major: u32,
    pub minor: u32,
}

impl SchemaVersion {
    pub fn new(major: u32, minor: u32) -> Self {
        SchemaVersion { major, minor }
    }
}

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl serde::Serialize for SchemaVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for SchemaVersion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let mut parts = s.splitn(2, '.');
        let major = parts
            .next()
            .ok_or_else(|| serde::de::Error::custom("missing major version"))?
            .parse::<u32>()
            .map_err(|e| serde::de::Error::custom(format!("invalid major: {e}")))?;
        let minor = parts
            .next()
            .ok_or_else(|| serde::de::Error::custom("missing minor version"))?
            .parse::<u32>()
            .map_err(|e| serde::de::Error::custom(format!("invalid minor: {e}")))?;
        Ok(SchemaVersion { major, minor })
    }
}

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

/// An RFC 3339 timestamp newtype over `time::OffsetDateTime`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timestamp(time::OffsetDateTime);

impl Timestamp {
    /// Creates a timestamp for the current moment.
    pub fn now() -> Self {
        Timestamp(time::OffsetDateTime::now_utc())
    }

    /// Wraps an existing `OffsetDateTime`.
    pub fn from_offset_datetime(dt: time::OffsetDateTime) -> Self {
        Timestamp(dt)
    }

    /// Returns the inner `OffsetDateTime`.
    pub fn as_offset_datetime(&self) -> &time::OffsetDateTime {
        &self.0
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let formatted = self
            .0
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "INVALID_TIMESTAMP".into());
        f.write_str(&formatted)
    }
}

impl serde::Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = self
            .0
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&s)
    }
}

impl<'de> serde::Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let dt = time::OffsetDateTime::parse(
            &s,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Timestamp(dt))
    }
}

// ---------------------------------------------------------------------------
// SymbolName
// ---------------------------------------------------------------------------

/// A symbol name from a binary (function name, variable name, section name, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SymbolName(String);

impl SymbolName {
    /// Creates a new symbol name.
    pub fn new(name: impl Into<String>) -> Self {
        SymbolName(name.into())
    }
}

impl std::fmt::Display for SymbolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// The origin of a claim or piece of analysis data.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Provenance {
    Human,
    BackendAutogenerated,
    ImportedSymbol,
    StaticAnalysis,
    DynamicAnalysis,
    Agent { worker_run_id: WorkerRunId },
    Derived,
    ReimplementationTest,
    ExternalReference,
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provenance::Human => write!(f, "Human"),
            Provenance::BackendAutogenerated => write!(f, "BackendAutogenerated"),
            Provenance::ImportedSymbol => write!(f, "ImportedSymbol"),
            Provenance::StaticAnalysis => write!(f, "StaticAnalysis"),
            Provenance::DynamicAnalysis => write!(f, "DynamicAnalysis"),
            Provenance::Agent { worker_run_id } => {
                write!(f, "Agent({worker_run_id})")
            }
            Provenance::Derived => write!(f, "Derived"),
            Provenance::ReimplementationTest => write!(f, "ReimplementationTest"),
            Provenance::ExternalReference => write!(f, "ExternalReference"),
        }
    }
}

impl serde::Serialize for Provenance {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Provenance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ProvenanceVisitor;

        impl<'de> serde::de::Visitor<'de> for ProvenanceVisitor {
            type Value = Provenance;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a provenance variant string")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Provenance, E> {
                match v {
                    "Human" => Ok(Provenance::Human),
                    "BackendAutogenerated" => Ok(Provenance::BackendAutogenerated),
                    "ImportedSymbol" => Ok(Provenance::ImportedSymbol),
                    "StaticAnalysis" => Ok(Provenance::StaticAnalysis),
                    "DynamicAnalysis" => Ok(Provenance::DynamicAnalysis),
                    "Derived" => Ok(Provenance::Derived),
                    "ReimplementationTest" => Ok(Provenance::ReimplementationTest),
                    "ExternalReference" => Ok(Provenance::ExternalReference),
                    s if s.starts_with("Agent(") && s.ends_with(')') => {
                        let uuid_str = &s[6..s.len() - 1];
                        let uuid: uuid::Uuid = uuid_str.parse().map_err(|_| {
                            serde::de::Error::invalid_value(serde::de::Unexpected::Str(s), &self)
                        })?;
                        Ok(Provenance::Agent {
                            worker_run_id: WorkerRunId::from_uuid(uuid),
                        })
                    }
                    _ => Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Str(v),
                        &self,
                    )),
                }
            }
        }

        deserializer.deserialize_str(ProvenanceVisitor)
    }
}

// ---------------------------------------------------------------------------
// Confidence
// ---------------------------------------------------------------------------

/// A confidence score in the range [0.0, 1.0].
///
/// Constructed via `Confidence::new(value)` which validates the range.
/// Serializes as a bare `f32` and validates on deserialization.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(transparent)]
pub struct Confidence(f32);

impl Confidence {
    /// Creates a new confidence score, validating it is in [0.0, 1.0].
    pub fn new(value: f32) -> Result<Self> {
        if !(0.0..=1.0).contains(&value) {
            return Err(Error::Validation(
                "confidence must be between 0 and 1".into(),
            ));
        }
        Ok(Confidence(value))
    }

    /// Returns the inner f32 value.
    pub fn value(&self) -> f32 {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for Confidence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f32::deserialize(deserializer)?;
        Confidence::new(value).map_err(|_| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Float(value as f64),
                &"a value between 0.0 and 1.0",
            )
        })
    }
}

// ---------------------------------------------------------------------------
// Entity modules
// ---------------------------------------------------------------------------

pub mod campaign;
pub mod claim;
pub mod evidence;
pub mod function;
pub mod records;
pub mod task;
pub mod values;

// Re-export public types from sub-modules at domain level so callers can
// write `use crate::domain::Function` instead of `use crate::domain::function::Function`.
pub use campaign::{Campaign, CampaignState};
pub use claim::{Claim, ClaimPredicate, ClaimState, ClaimValue};
pub use evidence::{ArtifactId, EntityId, Evidence, EvidenceKind, EvidenceLocation};
pub use function::Function;
pub use task::{RequiredCapabilities, Task, TaskKind, TaskPriority, TaskState, TaskSubject};
pub use values::{
    BinaryLocation, Derivation, DerivationMethod, EvidenceValue, ExtensionData, MetadataMap,
    ModuleIdentity, StableEntityKey,
};
pub use records::{
    Artifact, ArtifactStorage, BinaryArtifactMetadata, Endianness, EnvironmentIdentity, Project,
    Provider, ProviderRun, ProviderRunStatus, SemanticEntity, ARTIFACT_KIND_BINARY,
    ARTIFACT_KIND_CONFIGURATION, ARTIFACT_KIND_GENERATED_CANDIDATE, ARTIFACT_KIND_LOG,
    ARTIFACT_KIND_NATIVE_PROVIDER_OUTPUT, ARTIFACT_KIND_SOURCE_TREE, ARTIFACT_KIND_TRACE,
    ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_GLOBAL,
    ENTITY_KIND_SOURCE_SYMBOL, ENTITY_KIND_STRING, ENTITY_KIND_TYPE, PROVIDER_KIND_DEBUGGER,
    PROVIDER_KIND_DECOMPILER, PROVIDER_KIND_DISASSEMBLER, PROVIDER_KIND_HUMAN, PROVIDER_KIND_LLM,
    PROVIDER_KIND_SYMBOLIC_EXECUTOR,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Confidence tests --

    #[test]
    fn confidence_rejects_out_of_range() {
        assert!(Confidence::new(-0.1).is_err());
        assert!(Confidence::new(1.1).is_err());
        assert!(Confidence::new(f32::NEG_INFINITY).is_err());
        assert!(Confidence::new(f32::INFINITY).is_err());
        assert!(Confidence::new(f32::NAN).is_err());
    }

    #[test]
    fn confidence_accepts_boundary_values() {
        let c = Confidence::new(0.0).unwrap();
        assert!((c.value() - 0.0).abs() < f32::EPSILON);
        let c = Confidence::new(1.0).unwrap();
        assert!((c.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn confidence_valid_mid_range() {
        let c = Confidence::new(0.5).unwrap();
        assert!((c.value() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn confidence_serialize_roundtrip() {
        let c = Confidence::new(0.75).unwrap();
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "0.75");
        let deserialized: Confidence = serde_json::from_str(&json).unwrap();
        assert!((deserialized.value() - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn confidence_deserialize_rejects_out_of_range() {
        let result: std::result::Result<Confidence, _> = serde_json::from_str("1.5");
        assert!(result.is_err());
        let result: std::result::Result<Confidence, _> = serde_json::from_str("-0.1");
        assert!(result.is_err());
    }

    #[test]
    fn confidence_error_message() {
        match Confidence::new(42.0) {
            Err(Error::Validation(msg)) => {
                assert!(msg.contains("confidence must be between 0 and 1"));
            }
            _ => panic!("expected Validation error"),
        }
    }

    // -- Address / AddressSpace tests --

    #[test]
    fn address_spaces_serialize() {
        let cases = [
            (AddressSpace::Virtual, "\"Virtual\""),
            (AddressSpace::RelativeVirtual, "\"RelativeVirtual\""),
            (AddressSpace::FileOffset, "\"FileOffset\""),
            (AddressSpace::Physical, "\"Physical\""),
            (AddressSpace::Custom("test".into()), "\"Custom(test)\""),
        ];
        for (space, expected) in &cases {
            let json = serde_json::to_string(space).unwrap();
            assert_eq!(json, *expected, "mismatch for {space}");
        }
    }

    #[test]
    fn address_spaces_deserialize() {
        let space: AddressSpace = serde_json::from_str("\"Virtual\"").unwrap();
        assert_eq!(space, AddressSpace::Virtual);

        let space: AddressSpace = serde_json::from_str("\"Custom(foo)\"").unwrap();
        assert_eq!(space, AddressSpace::Custom("foo".into()));
    }

    #[test]
    fn address_spaces_deserialize_rejects_invalid() {
        let result: std::result::Result<AddressSpace, _> = serde_json::from_str("\"BogusSpace\"");
        assert!(result.is_err());
    }

    #[test]
    fn address_serialize_roundtrip() {
        let addr = Address::new(AddressSpace::Virtual, 0x1234);
        let json = serde_json::to_string(&addr).unwrap();
        let deserialized: Address = serde_json::from_str(&json).unwrap();
        assert_eq!(addr, deserialized);
        assert_eq!(deserialized.value, 0x1234);
        assert_eq!(deserialized.space, AddressSpace::Virtual);
    }

    #[test]
    fn address_display_format() {
        let addr = Address::new(AddressSpace::Virtual, 0x401000);
        let s = addr.to_string();
        assert!(s.contains("Virtual"));
        assert!(s.contains("401000"));
    }

    // -- ContentHash tests --

    #[test]
    fn content_hash_sha256_deterministic() {
        let h1 = ContentHash::sha256(b"hello");
        let h2 = ContentHash::sha256(b"hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.algorithm, HashAlgorithm::Sha256);
        assert_eq!(h1.digest.len(), 32);
    }

    #[test]
    fn content_hash_blake3_deterministic() {
        let h1 = ContentHash::blake3(b"hello");
        let h2 = ContentHash::blake3(b"hello");
        assert_eq!(h1, h2);
        assert_eq!(h1.algorithm, HashAlgorithm::Blake3);
        assert_eq!(h1.digest.len(), 32);
    }

    #[test]
    fn content_hash_from_bytes_is_blake3() {
        let h = ContentHash::from_bytes(b"test");
        assert_eq!(h.algorithm, HashAlgorithm::Blake3);
        assert_eq!(h.digest_hex().len(), 64);
    }

    #[test]
    fn content_hash_different_algorithms_differ() {
        let blake = ContentHash::blake3(b"same input");
        let sha = ContentHash::sha256(b"same input");
        assert_ne!(blake, sha);
        assert_ne!(blake.algorithm, sha.algorithm);
    }

    #[test]
    fn content_hash_serialize_roundtrip() {
        let h = ContentHash::sha256(b"roundtrip test");
        let json = serde_json::to_string(&h).unwrap();
        let deserialized: ContentHash = serde_json::from_str(&json).unwrap();
        assert_eq!(h, deserialized);
        assert_eq!(deserialized.algorithm, HashAlgorithm::Sha256);
    }

    #[test]
    fn content_hash_serialize_tagged_format() {
        let h = ContentHash::sha256(b"test");
        let json = serde_json::to_string(&h).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["algorithm"], "sha256");
        assert!(value["digest"].is_string());
    }

    #[test]
    fn content_hash_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/content_hash_sha256.json");
        let h: ContentHash = serde_json::from_str(fixture).unwrap();
        assert_eq!(h.algorithm, HashAlgorithm::Sha256);
        let re_serialized = serde_json::to_string(&h).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    #[test]
    fn content_hash_display_format() {
        let h = ContentHash::blake3(b"test");
        let s = h.to_string();
        assert!(s.starts_with("blake3:"));
        assert_eq!(s.len(), "blake3:".len() + 64);
    }

    // -- NamespacedId tests --

    #[test]
    fn namespaced_id_valid_simple() {
        let id = NamespacedId::parse("core.function").unwrap();
        assert_eq!(id.to_string(), "core.function");
    }

    #[test]
    fn namespaced_id_valid_three_segments() {
        let id = NamespacedId::parse("ida.hexrays.pseudocode").unwrap();
        assert_eq!(id.as_str(), "ida.hexrays.pseudocode");
    }

    #[test]
    fn namespaced_id_from_segments() {
        let id = NamespacedId::new(&["core", "function"]).unwrap();
        assert_eq!(id.to_string(), "core.function");
    }

    #[test]
    fn namespaced_id_rejects_uppercase() {
        assert!(NamespacedId::parse("Core.Function").is_err());
    }

    #[test]
    fn namespaced_id_rejects_empty_segment() {
        assert!(NamespacedId::parse("core..function").is_err());
    }

    #[test]
    fn namespaced_id_rejects_leading_dot() {
        assert!(NamespacedId::parse(".core").is_err());
    }

    #[test]
    fn namespaced_id_rejects_trailing_dot() {
        assert!(NamespacedId::parse("core.").is_err());
    }

    #[test]
    fn namespaced_id_rejects_slash() {
        assert!(NamespacedId::parse("core/function").is_err());
    }

    #[test]
    fn namespaced_id_rejects_backslash() {
        assert!(NamespacedId::parse("core\\function").is_err());
    }

    #[test]
    fn namespaced_id_rejects_whitespace() {
        assert!(NamespacedId::parse(" core").is_err());
        assert!(NamespacedId::parse("core function").is_err());
    }

    #[test]
    fn namespaced_id_rejects_empty() {
        assert!(NamespacedId::parse("").is_err());
        assert!(NamespacedId::new(&[]).is_err());
    }

    #[test]
    fn namespaced_id_serialize_roundtrip() {
        let id = NamespacedId::parse("core.function").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"core.function\"");
        let deserialized: NamespacedId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn namespaced_id_deserialize_rejects_invalid() {
        let result: std::result::Result<NamespacedId, _> =
            serde_json::from_str("\"Core.Bad\"");
        assert!(result.is_err());
    }

    #[test]
    fn namespaced_id_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/namespaced_id.json");
        let id: NamespacedId = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string(&id).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    // -- SchemaVersion tests --

    #[test]
    fn schema_version_display() {
        let v = SchemaVersion::new(1, 0);
        assert_eq!(v.to_string(), "1.0");
    }

    #[test]
    fn schema_version_serialize_roundtrip() {
        let v = SchemaVersion::new(2, 3);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "\"2.3\"");
        let deserialized: SchemaVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, deserialized);
    }

    #[test]
    fn schema_version_fixture_roundtrip() {
        let fixture = include_str!("../../tests/fixtures/schema_version.json");
        let v: SchemaVersion = serde_json::from_str(fixture).unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 0);
        let re_serialized = serde_json::to_string(&v).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }

    // -- Timestamp tests --

    #[test]
    fn timestamp_now_is_valid() {
        let ts = Timestamp::now();
        let s = ts.to_string();
        assert!(s.contains('T'));
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn timestamp_serialize_roundtrip() {
        let ts = Timestamp::now();
        let json = serde_json::to_string(&ts).unwrap();
        let deserialized: Timestamp = serde_json::from_str(&json).unwrap();
        assert_eq!(ts, deserialized);
    }

    // -- SymbolName tests --

    #[test]
    fn symbol_name_construct_and_display() {
        let name = SymbolName::new("main");
        assert_eq!(name.to_string(), "main");
    }

    #[test]
    fn symbol_name_serialize_roundtrip() {
        let name = SymbolName::new("_start");
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"_start\"");
        let deserialized: SymbolName = serde_json::from_str(&json).unwrap();
        assert_eq!(name, deserialized);
    }

    // -- Provenance tests --

    #[test]
    fn provenances_display_and_serialize() {
        let cases: Vec<(Provenance, &str)> = vec![
            (Provenance::Human, "\"Human\""),
            (Provenance::BackendAutogenerated, "\"BackendAutogenerated\""),
            (Provenance::ImportedSymbol, "\"ImportedSymbol\""),
            (Provenance::StaticAnalysis, "\"StaticAnalysis\""),
            (Provenance::DynamicAnalysis, "\"DynamicAnalysis\""),
            (Provenance::Derived, "\"Derived\""),
            (Provenance::ReimplementationTest, "\"ReimplementationTest\""),
            (Provenance::ExternalReference, "\"ExternalReference\""),
        ];
        for (prov, expected_json) in &cases {
            let json = serde_json::to_string(prov).unwrap();
            assert_eq!(json, *expected_json, "mismatch for {prov}");
        }
    }

    #[test]
    fn provenances_deserialize() {
        let p: Provenance = serde_json::from_str("\"Human\"").unwrap();
        assert_eq!(p, Provenance::Human);

        let p: Provenance = serde_json::from_str("\"StaticAnalysis\"").unwrap();
        assert_eq!(p, Provenance::StaticAnalysis);
    }

    #[test]
    fn provenance_agent_serialize_roundtrip() {
        let wr_id = WorkerRunId::new();
        let p = Provenance::Agent {
            worker_run_id: wr_id,
        };
        let json = serde_json::to_string(&p).unwrap();
        let deserialized: Provenance = serde_json::from_str(&json).unwrap();
        assert_eq!(p, deserialized);
    }

    #[test]
    fn provenance_deserialize_rejects_invalid() {
        let result: std::result::Result<Provenance, _> =
            serde_json::from_str("\"BogusProvenance\"");
        assert!(result.is_err());
    }
}
