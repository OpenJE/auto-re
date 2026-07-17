//! Evidence entity — data supporting or refuting a claim.
//!
//! Evidence is the raw material that backs analysis claims. Each piece of
//! evidence records *what* kind of data it is, *where* it came from
//! (address, file path, provenance), and an optional artifact reference.

use crate::domain::{Address, Provenance};
use crate::ids::{
    BinaryRevisionId, CampaignId, ClaimId, EvidenceId, FunctionId, ModuleId, TaskId, WorkerRunId,
};
use crate::worker::output::{FunctionAnalysisOutput, ProposedEvidence};

use uuid::Uuid;

// ---------------------------------------------------------------------------
// EntityId
// ---------------------------------------------------------------------------

/// A typed identifier for any domain entity that can be the subject of
/// a claim or the target of evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EntityId {
    /// A function within a binary.
    Function(FunctionId),
    /// A specific binary revision.
    BinaryRevision(BinaryRevisionId),
    /// A module within a binary.
    Module(ModuleId),
    /// An analysis campaign.
    Campaign(CampaignId),
    /// A single task within a campaign.
    Task(TaskId),
    /// A claim about an entity.
    Claim(ClaimId),
    /// A piece of evidence.
    Evidence(EvidenceId),
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntityId::Function(id) => write!(f, "Function({id})"),
            EntityId::BinaryRevision(id) => write!(f, "BinaryRevision({id})"),
            EntityId::Module(id) => write!(f, "Module({id})"),
            EntityId::Campaign(id) => write!(f, "Campaign({id})"),
            EntityId::Task(id) => write!(f, "Task({id})"),
            EntityId::Claim(id) => write!(f, "Claim({id})"),
            EntityId::Evidence(id) => write!(f, "Evidence({id})"),
        }
    }
}

// ---------------------------------------------------------------------------
// ArtifactId
// ---------------------------------------------------------------------------

/// Identifies an analysis artifact (a blob of data produced during analysis).
///
/// Artifacts are content-addressed. The ID is a UUID that maps to a
/// content-hash-keyed artifact in the storage layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ArtifactId(Uuid);

impl ArtifactId {
    /// Creates a new random artifact ID.
    pub fn new() -> Self {
        ArtifactId(Uuid::new_v4())
    }

    /// Wraps an existing UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        ArtifactId(uuid)
    }

    /// Returns a reference to the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ArtifactId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ---------------------------------------------------------------------------
// EvidenceLocation
// ---------------------------------------------------------------------------

/// A specific location within a binary or source that evidence refers to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EvidenceLocation {
    /// The address (if in a binary context).
    pub address: Option<Address>,
    /// A file path or module-relative path.
    pub path: Option<String>,
}

impl EvidenceLocation {
    /// Creates a new evidence location with an optional address and path.
    pub fn new(address: Option<Address>, path: Option<String>) -> Self {
        EvidenceLocation { address, path }
    }
}

// ---------------------------------------------------------------------------
// EvidenceKind
// ---------------------------------------------------------------------------

/// The kind of analysis evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EvidenceKind {
    /// Output from a decompiler (pseudocode, AST, etc.).
    Decompilation,
    /// Raw disassembly of instructions.
    Disassembly,
    /// A control-flow graph representation.
    ControlFlowGraph,
    /// A call graph or sub-graph.
    CallGraph,
    /// An execution trace or log.
    Trace,
    /// A string reference discovered in the binary.
    StringReference,
    /// A reference to a global variable.
    GlobalReference,
    /// An annotation or comment from an analyst or tool.
    Comment,
    /// A runtime observation (register values, memory, etc.).
    RuntimeObservation,
    /// Raw response from a model provider.
    ModelResponse,
    /// A type descriptor recovered during analysis.
    TypeDescriptor,
    /// A struct/class layout diagram or definition.
    StructureLayout,
    /// A calling convention descriptor.
    CallingConventionDescriptor,
    /// A test result or harness output.
    TestOutput,
    /// A cross-reference listing.
    CrossReferenceListing,
    /// A code patch or diff.
    Patch,
    /// A screenshot or image.
    Screenshot,
    /// A custom evidence kind.
    Custom(String),
}

// ---------------------------------------------------------------------------
// Evidence
// ---------------------------------------------------------------------------

/// A piece of evidence supporting or refuting one or more claims.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Evidence {
    /// Unique identifier for this evidence.
    pub id: EvidenceId,
    /// What kind of evidence this is.
    pub kind: EvidenceKind,
    /// Optional link to a stored artifact.
    pub artifact: Option<ArtifactId>,
    /// Optional link to the entity this evidence describes.
    pub entity: Option<EntityId>,
    /// Optional location within the binary or source.
    pub location: Option<EvidenceLocation>,
    /// How this evidence was produced.
    pub provenance: Provenance,
}

impl Evidence {
    /// Creates a new piece of evidence.
    pub fn new(
        id: EvidenceId,
        kind: EvidenceKind,
        artifact: Option<ArtifactId>,
        entity: Option<EntityId>,
        location: Option<EvidenceLocation>,
        provenance: Provenance,
    ) -> Self {
        Evidence {
            id,
            kind,
            artifact,
            entity,
            location,
            provenance,
        }
    }

    /// Returns `true` if this evidence has a specific location (address or path).
    pub fn has_location(&self) -> bool {
        self.location
            .as_ref()
            .is_some_and(|loc| loc.address.is_some() || loc.path.is_some())
    }

    /// Returns `true` if this evidence links to an artifact.
    pub fn has_artifact(&self) -> bool {
        self.artifact.is_some()
    }

    // -----------------------------------------------------------------------
    // Conversion from worker output
    // -----------------------------------------------------------------------

    /// Creates an `Evidence` from a worker's `ProposedEvidence`.
    ///
    /// Note: `ProposedEvidence.description` and `ProposedEvidence.confidence`
    /// are not representable in the current `Evidence` schema and are dropped.
    pub fn from_proposed(
        function_id: FunctionId,
        proposed: ProposedEvidence,
        worker_run_id: WorkerRunId,
    ) -> crate::Result<Self> {
        Ok(Evidence::new(
            EvidenceId::new(),
            proposed.kind,
            None,
            Some(EntityId::Function(function_id)),
            proposed.location,
            Provenance::Agent { worker_run_id },
        ))
    }

    /// Converts all proposed evidence in a `FunctionAnalysisOutput` into
    /// `Evidence` entities linked to the given function.
    pub fn from_worker_output(
        function_id: FunctionId,
        output: &FunctionAnalysisOutput,
        worker_run_id: WorkerRunId,
    ) -> crate::Result<Vec<Self>> {
        Ok(output
            .evidence
            .iter()
            .map(|pe| {
                Evidence::new(
                    EvidenceId::new(),
                    pe.kind.clone(),
                    None,
                    Some(EntityId::Function(function_id)),
                    pe.location.clone(),
                    Provenance::Agent { worker_run_id },
                )
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Address, AddressSpace, Provenance};

    fn sample_evidence() -> Evidence {
        Evidence::new(
            EvidenceId::new(),
            EvidenceKind::Disassembly,
            None,
            None,
            Some(EvidenceLocation::new(
                Some(Address::new(AddressSpace::Virtual, 0x401000)),
                None,
            )),
            Provenance::StaticAnalysis,
        )
    }

    #[test]
    fn evidence_constructs() {
        let e = sample_evidence();
        assert_eq!(e.kind, EvidenceKind::Disassembly);
        assert!(e.has_location());
        assert!(!e.has_artifact());
    }

    #[test]
    fn evidence_without_location() {
        let e = Evidence::new(
            EvidenceId::new(),
            EvidenceKind::ModelResponse,
            None,
            None,
            None,
            Provenance::Agent {
                worker_run_id: Default::default(),
            },
        );
        assert!(!e.has_location());
    }

    #[test]
    fn evidence_with_artifact() {
        let artifact = ArtifactId::new();
        let e = Evidence::new(
            EvidenceId::new(),
            EvidenceKind::Decompilation,
            Some(artifact),
            Some(EntityId::Function(FunctionId::new())),
            None,
            Provenance::BackendAutogenerated,
        );
        assert!(e.has_artifact());
        assert!(!e.has_location());
    }

    #[test]
    fn evidence_serialize_roundtrip() {
        let e = sample_evidence();
        let json = serde_json::to_string(&e).unwrap();
        let deserialized: Evidence = serde_json::from_str(&json).unwrap();
        assert_eq!(e.id, deserialized.id);
        assert_eq!(deserialized.kind, EvidenceKind::Disassembly);
    }

    #[test]
    fn entity_id_display() {
        let fid = FunctionId::new();
        let eid = EntityId::Function(fid);
        let s = eid.to_string();
        assert!(s.starts_with("Function("));
        assert!(s.ends_with(')'));
    }

    #[test]
    fn artifact_id_roundtrip() {
        let a1 = ArtifactId::new();
        let json = serde_json::to_string(&a1).unwrap();
        let a2: ArtifactId = serde_json::from_str(&json).unwrap();
        assert_eq!(a1, a2);
    }

    #[test]
    fn artifact_id_default_creates_unique() {
        let a1 = ArtifactId::default();
        let a2 = ArtifactId::default();
        assert_ne!(a1, a2);
    }

    #[test]
    fn evidence_location_with_address_only() {
        let loc = EvidenceLocation::new(Some(Address::new(AddressSpace::Virtual, 0x1234)), None);
        assert_eq!(loc.address.as_ref().unwrap().value, 0x1234);
        assert!(loc.path.is_none());
    }

    #[test]
    fn evidence_location_with_path_only() {
        let loc = EvidenceLocation::new(None, Some("/tmp/analysis.log".into()));
        assert!(loc.address.is_none());
        assert_eq!(loc.path.as_deref().unwrap(), "/tmp/analysis.log");
    }
}
