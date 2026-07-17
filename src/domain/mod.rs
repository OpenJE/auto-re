//! Domain primitives — core value types for the auto-re system.
//!
//! These types are parse-don't-validate: once constructed, they
//! are guaranteed to be valid for their domain.

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
// ContentHash
// ---------------------------------------------------------------------------

/// A content-addressed hash of binary data, computed with BLAKE3.
///
/// The inner string is the lower-hex encoding (64 characters for the
/// 32-byte BLAKE3 output).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ContentHash(String);

impl ContentHash {
    /// Computes a BLAKE3 hash from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Self {
        ContentHash(blake3::hash(data).to_hex().to_string())
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
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
    pub fn new(value: f32) -> crate::Result<Self> {
        if !(0.0..=1.0).contains(&value) {
            return Err(crate::Error::Validation(
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
pub mod task;

// Re-export public types from sub-modules at domain level so callers can
// write `use crate::domain::Function` instead of `use crate::domain::function::Function`.
pub use campaign::{Campaign, CampaignState};
pub use claim::{Claim, ClaimPredicate, ClaimState, ClaimValue};
pub use evidence::{ArtifactId, EntityId, Evidence, EvidenceKind, EvidenceLocation};
pub use function::Function;
pub use task::{RequiredCapabilities, Task, TaskKind, TaskPriority, TaskState, TaskSubject};

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
            Err(crate::Error::Validation(msg)) => {
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
    fn content_hash_deterministic() {
        let data = b"hello world";
        let h1 = ContentHash::from_bytes(data);
        let h2 = ContentHash::from_bytes(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.to_string(), h2.to_string());
    }

    #[test]
    fn content_hash_length() {
        let h = ContentHash::from_bytes(b"test");
        // BLAKE3 output is 32 bytes = 64 hex characters
        assert_eq!(h.to_string().len(), 64);
    }

    #[test]
    fn content_hash_different_inputs_differ() {
        let h1 = ContentHash::from_bytes(b"hello");
        let h2 = ContentHash::from_bytes(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn content_hash_serialize_roundtrip() {
        let h = ContentHash::from_bytes(b"roundtrip test");
        let json = serde_json::to_string(&h).unwrap();
        let deserialized: ContentHash = serde_json::from_str(&json).unwrap();
        assert_eq!(h, deserialized);
    }

    #[test]
    fn content_hash_empty_input() {
        let h = ContentHash::from_bytes(b"");
        // Even empty input produces a valid 64-char hash
        assert_eq!(h.to_string().len(), 64);
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
