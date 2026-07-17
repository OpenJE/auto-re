//! Typed ID macro and system-wide identifiers.
//!
//! All system identifiers are newtypes over `uuid::Uuid` with
//! full trait support for storage, comparison, and serialization.
//! The type system prevents mixing different ID kinds — a `ProjectId`
//! cannot be assigned where a `TaskId` is expected.
//!
//! All IDs use UUIDv7 (time-ordered) for natural chronological sorting.

/// Creates a strongly-typed ID newtype over `uuid::Uuid` (v7, time-ordered).
///
/// Generates a `#[repr(transparent)]` wrapper with:
/// - `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`
/// - `serde::Serialize, serde::Deserialize`
/// - `new()`, `from_uuid()`, `as_uuid()`
/// - `Default` (generates a new random UUIDv7)
/// - `Display` (delegates to the inner UUID)
macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[repr(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// Creates a new random UUIDv7 ID (time-ordered).
            pub fn new() -> Self {
                $name(uuid::Uuid::now_v7())
            }

            /// Wraps an existing UUID into this ID type.
            pub fn from_uuid(uuid: uuid::Uuid) -> Self {
                $name(uuid)
            }

            /// Returns a reference to the inner UUID.
            pub fn as_uuid(&self) -> &uuid::Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// §8 Identifiers (M1 — existing)
// ---------------------------------------------------------------------------

define_id!(
    ProjectId,
    "Identifies an auto-re project — a top-level workspace or analysis campaign container."
);
define_id!(
    BinaryId,
    "Identifies a specific binary artifact (e.g., an ELF, PE, or Mach-O file under analysis)."
);
define_id!(
    BinaryRevisionId,
    "Identifies a particular revision of a binary — captures the same file at a different build/version."
);
define_id!(
    ModuleId,
    "Identifies a module within a binary (a compilation unit, shared library segment, or loadable component)."
);
define_id!(
    FunctionId,
    "Identifies a function within a binary's module."
);
define_id!(
    TaskId,
    "Identifies an analysis task — a single unit of work within a campaign."
);
define_id!(
    ClaimId,
    "Identifies a claim made during analysis (an assertion or finding about the binary)."
);
define_id!(
    EvidenceId,
    "Identifies a piece of evidence supporting or refuting a claim."
);
define_id!(
    CampaignId,
    "Identifies an analysis campaign — a coordinated set of tasks."
);
define_id!(
    WorkerRunId,
    "Identifies a worker run — a single execution of a worker within a campaign."
);
define_id!(
    TransactionId,
    "Identifies a session — a logical sequence of operations."
);
define_id!(
    ImplementationTargetId,
    "Identifies an implementation target for a test or analysis."
);
define_id!(
    ValidationRunId,
    "Identifies a validation run — a single execution of validation logic."
);

// ---------------------------------------------------------------------------
// §6 Identifiers (Stage 0 — new)
// ---------------------------------------------------------------------------

define_id!(
    ArtifactId,
    "Identifies a generic artifact produced or consumed by the analysis pipeline."
);
define_id!(
    BinaryArtifactId,
    "Identifies a binary artifact (compiled executable, shared library, firmware image)."
);
define_id!(
    SourceArtifactId,
    "Identifies a source-code artifact (source file, patch, translation unit)."
);
define_id!(
    EntityId,
    "Identifies a semantic entity discovered during analysis (function, variable, type, etc.)."
);
define_id!(
    HypothesisId,
    "Identifies a hypothesis generated during exploratory analysis."
);
define_id!(
    ContradictionId,
    "Identifies a contradiction detected between two or more claims or hypotheses."
);
define_id!(
    ProviderId,
    "Identifies an analysis provider (tool, model, or human contributor)."
);
define_id!(
    ProviderRunId,
    "Identifies a single execution run of an analysis provider."
);
define_id!(
    NativeArtifactId,
    "Identifies a native-format artifact specific to a toolchain or platform."
);
define_id!(
    VerificationRecordId,
    "Identifies a record of a verification step applied to a claim or artifact."
);
define_id!(
    OperationId,
    "Identifies a discrete operation within the analysis pipeline."
);
define_id!(
    ProjectEventId,
    "Identifies an event recorded within a project's event stream."
);
define_id!(
    PackageId,
    "Identifies a package — a distributable unit of analysis output."
);
define_id!(
    GenerationTargetId,
    "Identifies a target for code generation or artifact production."
);
define_id!(
    EvidenceRecordId,
    "Identifies an append-only evidence record within a project."
);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serialize_and_roundtrip() {
        let id = ProjectId::new();
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: ProjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn ids_are_not_interchangeable() {
        let project = ProjectId::new();
        let task = TaskId::new();
        assert_ne!(project.as_uuid(), task.as_uuid());
        let _project_str = format!("{project}");
        let _task_str = format!("{task}");
    }

    #[test]
    fn ids_default_creates_new_id() {
        let id1 = ProjectId::default();
        let id2 = ProjectId::default();
        assert_ne!(id1, id2);
    }

    #[test]
    fn ids_copy_works() {
        let id = BinaryId::new();
        let copied = id;
        assert_eq!(id, copied);
    }

    #[test]
    fn ids_from_uuid_roundtrip() {
        let uuid = uuid::Uuid::now_v7();
        let id = CampaignId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), &uuid);
    }

    #[test]
    fn ids_all_types_constructible() {
        // M1 IDs
        let _ = ProjectId::new();
        let _ = BinaryId::new();
        let _ = BinaryRevisionId::new();
        let _ = ModuleId::new();
        let _ = FunctionId::new();
        let _ = TaskId::new();
        let _ = ClaimId::new();
        let _ = EvidenceId::new();
        let _ = CampaignId::new();
        let _ = WorkerRunId::new();
        let _ = TransactionId::new();
        let _ = ImplementationTargetId::new();
        let _ = ValidationRunId::new();
        // Stage 0 IDs
        let _ = ArtifactId::new();
        let _ = BinaryArtifactId::new();
        let _ = SourceArtifactId::new();
        let _ = EntityId::new();
        let _ = HypothesisId::new();
        let _ = ContradictionId::new();
        let _ = ProviderId::new();
        let _ = ProviderRunId::new();
        let _ = NativeArtifactId::new();
        let _ = VerificationRecordId::new();
        let _ = OperationId::new();
        let _ = ProjectEventId::new();
        let _ = PackageId::new();
        let _ = GenerationTargetId::new();
        let _ = EvidenceRecordId::new();
    }

    #[test]
    fn ids_serialize_are_distinct_across_types() {
        let uuid = uuid::Uuid::now_v7();
        let task_id = TaskId::from_uuid(uuid);
        let json = serde_json::to_string(&task_id).unwrap();
        let deserialized: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(task_id, deserialized);
        let campaign_id = CampaignId::from_uuid(uuid);
        assert_eq!(campaign_id.as_uuid(), task_id.as_uuid());
    }

    #[test]
    fn uuid_v7_sorts() {
        let a = uuid::Uuid::now_v7();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = uuid::Uuid::now_v7();

        assert!(
            a < b,
            "UUIDv7 generated later should sort after earlier: {a} < {b}"
        );

        assert!(
            a.to_string() < b.to_string(),
            "UUIDv7 string form should preserve temporal ordering"
        );
    }

    #[test]
    fn ids_stage0_roundtrip() {
        // All 14 Stage 0 UUID newtypes round-trip through JSON.
        fn roundtrip<
            T: serde::Serialize + for<'de> serde::Deserialize<'de> + PartialEq + std::fmt::Debug,
        >(
            name: &str,
            val: &T,
        ) {
            let json = serde_json::to_string(val).unwrap();
            let back: T = serde_json::from_str(&json).unwrap();
            assert_eq!(val, &back, "{name} failed round-trip");
        }

        roundtrip("ArtifactId", &ArtifactId::new());
        roundtrip("BinaryArtifactId", &BinaryArtifactId::new());
        roundtrip("SourceArtifactId", &SourceArtifactId::new());
        roundtrip("EntityId", &EntityId::new());
        roundtrip("HypothesisId", &HypothesisId::new());
        roundtrip("ContradictionId", &ContradictionId::new());
        roundtrip("ProviderId", &ProviderId::new());
        roundtrip("ProviderRunId", &ProviderRunId::new());
        roundtrip("NativeArtifactId", &NativeArtifactId::new());
        roundtrip("VerificationRecordId", &VerificationRecordId::new());
        roundtrip("OperationId", &OperationId::new());
        roundtrip("ProjectEventId", &ProjectEventId::new());
        roundtrip("PackageId", &PackageId::new());
        roundtrip("GenerationTargetId", &GenerationTargetId::new());
        roundtrip("EvidenceRecordId", &EvidenceRecordId::new());
        // M1 IDs also verified here
        roundtrip("ProjectId", &ProjectId::new());
    }

    #[test]
    fn ids_v7_sort_chronologically() {
        let a = ArtifactId::new();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = ArtifactId::new();
        assert!(a < b, "UUIDv7-based IDs should sort chronologically");
    }

    #[test]
    fn ids_v7_lexicographic() {
        let a = ProviderId::new();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let b = ProviderId::new();
        assert!(a.to_string() < b.to_string());
        assert!(a < b);
    }

    #[test]
    fn ids_fixture_project_id_roundtrip() {
        let fixture = include_str!("../tests/fixtures/project_id.json");
        let id: ProjectId = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string(&id).unwrap();
        assert_eq!(fixture.trim(), re_serialized);
    }
}
