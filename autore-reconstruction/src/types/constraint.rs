//! Deterministic layout constraint model for type reconstruction.
//!
//! Layout constraints are observations about the memory layout of a semantic
//! entity (usually a type or class). They are canonicalised as evidence
//! records and then reconciled deterministically into a proposed layout
//! hypothesis.

use autore_schema::domain::records::EvidenceRecord;
use autore_schema::domain::{Derivation, DerivationMethod, EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, EntityId, EvidenceRecordId, ProjectId};

/// Namespaced predicate used for layout-constraint evidence records.
pub static EVIDENCE_PREDICATE_LAYOUT_CONSTRAINT: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| {
        NamespacedId::parse("evidence.predicate.layout-constraint").unwrap()
    });

/// Namespaced operation identifier for deterministic layout reconciliation.
pub static OPERATION_LAYOUT_RECONCILIATION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("recon.layout.reconciliation").unwrap());

/// A single deterministic layout constraint about a semantic entity.
///
/// See spec §10.2 for the full list of constraint kinds. Every constraint
/// carries optional provenance linking it back to the artifact that produced
/// it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayoutConstraint {
    pub kind: LayoutConstraintKind,
    pub evidence_artifact_id: Option<ArtifactId>,
    pub recorded_at: Timestamp,
}

impl LayoutConstraint {
    /// Creates a constraint with the current timestamp and no artifact provenance.
    pub fn new(kind: LayoutConstraintKind) -> Self {
        Self {
            kind,
            evidence_artifact_id: None,
            recorded_at: Timestamp::now(),
        }
    }

    /// Creates a constraint tied to the artifact that produced it.
    pub fn with_artifact(kind: LayoutConstraintKind, artifact_id: ArtifactId) -> Self {
        Self {
            kind,
            evidence_artifact_id: Some(artifact_id),
            recorded_at: Timestamp::now(),
        }
    }

    /// Returns the primary entity this constraint is about.
    pub fn primary_entity(&self) -> EntityId {
        self.kind.primary_entity()
    }
}

/// The 11 deterministic layout constraint kinds from spec §10.2.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum LayoutConstraintKind {
    /// A field of `entity` was observed at `offset`.
    FieldObservedAtOffset { entity: EntityId, offset: usize },
    ReadWidth {
        entity: EntityId,
        offset: usize,
        width_bytes: usize,
    },
    WriteWidth {
        entity: EntityId,
        offset: usize,
        width_bytes: usize,
    },
    /// `entity` is allocated with `size_bytes` bytes.
    ObjectAllocationSize { entity: EntityId, size_bytes: usize },
    /// `entity` has a vtable pointer at `offset`.
    VtablePointerLocation { entity: EntityId, offset: usize },
    /// Virtual slot `slot_idx` in `vtable` calls `called`.
    VirtualSlotTarget {
        slot_idx: usize,
        vtable: EntityId,
        called: EntityId,
    },
    /// `entity` adjusts its base `base_entity` by `offset`.
    BaseAdjustment {
        entity: EntityId,
        base_entity: EntityId,
        offset: usize,
    },
    /// Parameter `param_idx` of `func` is used according to `kind`.
    FunctionParameterUsage {
        func: EntityId,
        param_idx: usize,
        kind: String,
    },
    /// The return value of `func` is used according to `kind`.
    ReturnValueUse { func: EntityId, kind: String },
    /// `entity` is an array with stride `stride_bytes`.
    ArrayStride {
        entity: EntityId,
        stride_bytes: usize,
    },
    /// `entity` requires `alignment`-byte alignment.
    AlignmentRequirement { entity: EntityId, alignment: usize },
}

impl LayoutConstraintKind {
    /// Returns the entity this constraint is primarily about.
    pub fn primary_entity(&self) -> EntityId {
        match self {
            Self::FieldObservedAtOffset { entity, .. }
            | Self::ReadWidth { entity, .. }
            | Self::WriteWidth { entity, .. }
            | Self::ObjectAllocationSize { entity, .. }
            | Self::VtablePointerLocation { entity, .. }
            | Self::BaseAdjustment { entity, .. }
            | Self::ArrayStride { entity, .. }
            | Self::AlignmentRequirement { entity, .. } => *entity,
            Self::VirtualSlotTarget { vtable, .. } => *vtable,
            Self::FunctionParameterUsage { func, .. } | Self::ReturnValueUse { func, .. } => *func,
        }
    }
}

/// In-memory store for layout constraints before they are canonicalised as
/// evidence.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayoutConstraintStore {
    constraints: Vec<LayoutConstraint>,
}

impl LayoutConstraintStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a constraint to the store.
    pub fn add(&mut self, constraint: LayoutConstraint) {
        self.constraints.push(constraint);
    }

    /// Returns all stored constraints.
    pub fn constraints(&self) -> &[LayoutConstraint] {
        &self.constraints
    }

    /// Serialises the store to a deterministic JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Builds an `EvidenceRecord` that can be issued via
    /// `ApplicationCommand::AddEvidence`.
    ///
    /// The record's value is the JSON-serialised constraint store (as an
    /// `EvidenceValue::String` because `EvidenceValue::Json` does not exist
    /// in the current schema).
    pub fn to_evidence_record(&self, project: ProjectId, subject: EntityId) -> EvidenceRecord {
        let value = EvidenceValue::String(self.to_json().unwrap_or_else(|_| "{}".into()));
        EvidenceRecord {
            id: EvidenceRecordId::new(),
            project,
            subject,
            predicate: EVIDENCE_PREDICATE_LAYOUT_CONSTRAINT.clone(),
            value,
            derivation: Derivation::new(
                DerivationMethod::DeterministicAnalysis,
                OPERATION_LAYOUT_RECONCILIATION.clone(),
                vec![],
                vec![],
            ),
            provider_run: None,
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_serialises_to_json() {
        let mut store = LayoutConstraintStore::new();
        let entity = EntityId::new();
        store.add(LayoutConstraint::new(
            LayoutConstraintKind::ObjectAllocationSize {
                entity,
                size_bytes: 32,
            },
        ));
        let json = store.to_json().expect("serialise");
        assert!(json.contains("ObjectAllocationSize"));
        assert!(json.contains("\"size_bytes\":32"));
    }

    #[test]
    fn evidence_record_carries_layout_predicate() {
        let entity = EntityId::new();
        let project = ProjectId::new();
        let mut store = LayoutConstraintStore::new();
        store.add(LayoutConstraint::new(
            LayoutConstraintKind::AlignmentRequirement {
                entity,
                alignment: 8,
            },
        ));
        let record = store.to_evidence_record(project, entity);
        assert_eq!(record.project, project);
        assert_eq!(record.subject, entity);
        assert_eq!(record.predicate, *EVIDENCE_PREDICATE_LAYOUT_CONSTRAINT);
    }
}
