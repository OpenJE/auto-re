//! Typed ID macro and system-wide identifiers.
//!
//! All system identifiers are newtypes over `uuid::Uuid` with
//! full trait support for storage, comparison, and serialization.
//! The type system prevents mixing different ID kinds — a `ProjectId`
//! cannot be assigned where a `TaskId` is expected.

/// Creates a strongly-typed ID newtype over `uuid::Uuid`.
///
/// Generates a `#[repr(transparent)]` wrapper with:
/// - `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash`
/// - `serde::Serialize, serde::Deserialize`
/// - `new()`, `from_uuid()`, `as_uuid()`
/// - `Default` (generates a new random UUID)
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
            /// Creates a new random ID.
            pub fn new() -> Self {
                $name(uuid::Uuid::new_v4())
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
// §8 Identifiers
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
        // Verify the type system prevents assignment.
        // `ProjectId` and `TaskId` are distinct types even though both wrap UUID.
        let project = ProjectId::new();
        let task = TaskId::new();
        // Inner UUIDs differ (virtually guaranteed by v4 randomness)
        assert_ne!(project.as_uuid(), task.as_uuid());
        // Both implement Display via UUID's lower-hex format
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
        let copied = id; // Copy (not move)
        assert_eq!(id, copied);
    }

    #[test]
    fn ids_from_uuid_roundtrip() {
        let uuid = uuid::Uuid::new_v4();
        let id = CampaignId::from_uuid(uuid);
        assert_eq!(id.as_uuid(), &uuid);
    }

    #[test]
    fn ids_all_types_constructible() {
        // Sanity check: all 13 types can be constructed
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
    }

    #[test]
    fn ids_serialize_are_distinct_across_types() {
        // Same UUID wrapped in different ID types must roundtrip correctly
        let uuid = uuid::Uuid::new_v4();
        let task_id = TaskId::from_uuid(uuid);
        let json = serde_json::to_string(&task_id).unwrap();
        let deserialized: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(task_id, deserialized);
        // A CampaignId created from the same UUID is NOT interchangeable
        let campaign_id = CampaignId::from_uuid(uuid);
        // (Compile-time check: comparing task_id and campaign_id would fail)
        assert_eq!(campaign_id.as_uuid(), task_id.as_uuid());
    }

    #[test]
    fn uuid_v7_sorts() {
        // UUIDv7 embeds a Unix timestamp in the leading 48 bits, so two v7
        // UUIDs generated 1ms apart MUST sort lexicographically in creation order.
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
}
