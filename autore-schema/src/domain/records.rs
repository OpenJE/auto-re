//! Stage 0 record types — Project, Artifact, and supporting types per §7 + §8.
//!
//! These are the top-level domain records that persistence layers store and
//! query. Artifact kinds are registered as `NamespacedId` constants (NOT enum
//! variants) to allow runtime extensibility.

use std::collections::BTreeMap;
use std::path::PathBuf;

use autore_core::operation::OperationState;

use crate::domain::{Confidence, ContentHash, ExtensionData, EvidenceValue, MetadataMap, NamespacedId, SchemaVersion, StableEntityKey, Timestamp};
use crate::ids::{ArtifactId, ContradictionId, EntityId, EvidenceRecordId, GenerationTargetId, HypothesisId, NativeArtifactId, OperationId, PackageId, ProjectEventId, ProjectId, ProviderId, ProviderRunId, VerificationRecordId};

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// A project — the top-level workspace container for analysis.
///
/// Projects group artifacts, entities, evidence, hypotheses, and operations
/// under a single durable identity with a schema version for forward
/// migration.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub schema_version: SchemaVersion,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub metadata: MetadataMap,
}

impl Project {
    /// Creates a new project with the given name, using the current schema
    /// version (2.0) and timestamp.
    pub fn new(name: impl Into<String>) -> Self {
        let now = Timestamp::now();
        Project {
            id: ProjectId::new(),
            name: name.into(),
            schema_version: SchemaVersion::new(2, 0),
            created_at: now,
            updated_at: now,
            metadata: MetadataMap::new(),
        }
    }

    /// Bumps `updated_at` to the current time.
    pub fn touch(&mut self) {
        self.updated_at = Timestamp::now();
    }
}

// ---------------------------------------------------------------------------
// Endianness
// ---------------------------------------------------------------------------

/// Byte order of a binary artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Endianness {
    Little,
    Big,
}

// ---------------------------------------------------------------------------
// BinaryArtifactMetadata
// ---------------------------------------------------------------------------

/// Metadata specific to binary artifacts (executables, shared libraries,
/// firmware images).
///
/// Stage 0 stores these fields but does NOT inspect or parse the binary
/// content (§8: "Stage 0 does not inspect or parse the binary").
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BinaryArtifactMetadata {
    pub format: Option<NamespacedId>,
    pub architecture: Option<NamespacedId>,
    pub endianness: Option<Endianness>,
    pub preferred_image_base: Option<u64>,
}

// ---------------------------------------------------------------------------
// ArtifactStorage
// ---------------------------------------------------------------------------

/// How an artifact's content is physically stored.
///
/// Extensible via serde adjacently-tagged enum (`#[serde(tag, content)]`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ArtifactStorage {
    /// A blob managed by the auto-re storage layer (copied into the project
    /// directory tree).
    ManagedBlob { relative_path: PathBuf },
    /// A file outside the project directory, referenced by its canonical path.
    ExternalFile { canonical_path: PathBuf },
}

// ---------------------------------------------------------------------------
// Artifact
// ---------------------------------------------------------------------------

/// An immutable, content-addressed artifact associated with a project.
///
/// Artifacts are identified by their `ContentHash` (SHA-256 by default).
/// Managed artifacts are copied into the project's storage directory;
/// external artifacts are referenced by path and verified on demand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Artifact {
    pub id: ArtifactId,
    pub project: ProjectId,
    pub kind: NamespacedId,
    pub content_hash: ContentHash,
    pub size: u64,
    pub storage: ArtifactStorage,
    pub created_at: Timestamp,
    pub metadata: MetadataMap,
}

// ---------------------------------------------------------------------------
// Artifact kind constants (§8)
// ---------------------------------------------------------------------------

/// Artifact kind: a binary file (executable, shared library, firmware image).
pub static ARTIFACT_KIND_BINARY: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.binary").unwrap());

/// Artifact kind: a source tree (source files, patches, translation units).
pub static ARTIFACT_KIND_SOURCE_TREE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.source-tree").unwrap());

/// Artifact kind: output from a native analysis provider.
pub static ARTIFACT_KIND_NATIVE_PROVIDER_OUTPUT: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.native-provider-output").unwrap());

/// Artifact kind: a configuration file.
pub static ARTIFACT_KIND_CONFIGURATION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.configuration").unwrap());

/// Artifact kind: a log file.
pub static ARTIFACT_KIND_LOG: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.log").unwrap());

/// Artifact kind: an execution trace.
pub static ARTIFACT_KIND_TRACE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.trace").unwrap());

/// Artifact kind: a generated candidate (e.g., proposed source code).
pub static ARTIFACT_KIND_GENERATED_CANDIDATE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.generated-candidate").unwrap());

// ---------------------------------------------------------------------------
// SemanticEntity
// ---------------------------------------------------------------------------

/// A semantic entity discovered during analysis — a function, type, global
/// variable, string literal, external function, or source symbol.
///
/// Entities are identified by a UUIDv7 `EntityId`. An optional `StableEntityKey`
/// provides cross-revision stability for entities that can be uniquely located
/// within a binary or artifact. Multiple entities in the same project may share
/// a `kind` but a non-NULL `stable_key` must be unique per project.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SemanticEntity {
    pub id: EntityId,
    pub project: ProjectId,
    pub kind: NamespacedId,
    pub stable_key: Option<StableEntityKey>,
    pub display_name: Option<String>,
    pub created_at: Timestamp,
    pub metadata: MetadataMap,
}

impl SemanticEntity {
    pub fn new(
        project: ProjectId,
        kind: NamespacedId,
        stable_key: Option<StableEntityKey>,
        display_name: Option<String>,
    ) -> Self {
        SemanticEntity {
            id: EntityId::new(),
            project,
            kind,
            stable_key,
            display_name,
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Entity kind constants
// ---------------------------------------------------------------------------

/// Entity kind: a function within a binary.
pub static ENTITY_KIND_FUNCTION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.function").unwrap());

/// Entity kind: a type definition.
pub static ENTITY_KIND_TYPE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.type").unwrap());

/// Entity kind: a global variable.
pub static ENTITY_KIND_GLOBAL: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.global").unwrap());

/// Entity kind: a string literal.
pub static ENTITY_KIND_STRING: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.string").unwrap());

/// Entity kind: an external (imported) function.
pub static ENTITY_KIND_EXTERNAL_FUNCTION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.external-function").unwrap());

/// Entity kind: a source-level symbol (from debug info or source analysis).
pub static ENTITY_KIND_SOURCE_SYMBOL: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.source-symbol").unwrap());

// ---------------------------------------------------------------------------
// ProviderRunStatus
// ---------------------------------------------------------------------------

/// The finite state machine for a provider run's lifecycle.
///
/// Valid transitions:
/// - Running -> Completed, Failed, Cancelled, Inconclusive
/// - All other transitions are invalid (terminal states are terminal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ProviderRunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
    Inconclusive,
}

impl ProviderRunStatus {
    /// Validates a state transition from `self` to `target`.
    ///
    /// Only `Running` may transition to a terminal state (`Completed`,
    /// `Failed`, `Cancelled`, `Inconclusive`). Terminal states cannot
    /// transition further.
    pub fn transition(&self, target: ProviderRunStatus) -> autore_core::Result<()> {
        match (self, target) {
            (ProviderRunStatus::Running, ProviderRunStatus::Completed)
            | (ProviderRunStatus::Running, ProviderRunStatus::Failed)
            | (ProviderRunStatus::Running, ProviderRunStatus::Cancelled)
            | (ProviderRunStatus::Running, ProviderRunStatus::Inconclusive) => Ok(()),
            _ => Err(autore_core::Error::InvalidStateTransition(format!(
                "{self:?} -> {target:?}"
            ))),
        }
    }

    /// Returns `true` if this status is a terminal state.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, ProviderRunStatus::Running)
    }
}

impl std::fmt::Display for ProviderRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderRunStatus::Running => write!(f, "Running"),
            ProviderRunStatus::Completed => write!(f, "Completed"),
            ProviderRunStatus::Failed => write!(f, "Failed"),
            ProviderRunStatus::Cancelled => write!(f, "Cancelled"),
            ProviderRunStatus::Inconclusive => write!(f, "Inconclusive"),
        }
    }
}

// ---------------------------------------------------------------------------
// EnvironmentIdentity
// ---------------------------------------------------------------------------

/// Describes the execution environment in which a provider run took place.
///
/// Captures OS, architecture, optional isolation backend and image digest,
/// plus an extensible `ExtensionData` for environment-specific metadata.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EnvironmentIdentity {
    pub operating_system: NamespacedId,
    pub architecture: NamespacedId,
    pub isolation_backend: Option<NamespacedId>,
    pub image_digest: Option<ContentHash>,
    pub extension: Option<ExtensionData>,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// An analysis provider — a tool, model, or human that produces observations.
///
/// Providers are NOT canonical entities (§3.2); they bridge to canonical
/// `SemanticEntity` via `ProviderEntityAlias` (task 16).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Provider {
    pub id: ProviderId,
    pub package_id: Option<PackageId>,
    pub name: String,
    pub kind: NamespacedId,
    pub version: String,
    pub executable_hash: Option<ContentHash>,
}

impl Provider {
    /// Creates a new provider with the given name, kind, and version.
    pub fn new(name: impl Into<String>, kind: NamespacedId, version: impl Into<String>) -> Self {
        Provider {
            id: ProviderId::new(),
            package_id: None,
            name: name.into(),
            kind,
            version: version.into(),
            executable_hash: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Provider kind constants (§10)
// ---------------------------------------------------------------------------

/// Provider kind: a disassembler (e.g., IDA, Ghidra, objdump).
pub static PROVIDER_KIND_DISASSEMBLER: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("provider.disassembler").unwrap());

/// Provider kind: a decompiler (e.g., Hex-Rays, Ghidra decompiler).
pub static PROVIDER_KIND_DECOMPILER: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("provider.decompiler").unwrap());

/// Provider kind: a debugger (e.g., GDB, LLDB).
pub static PROVIDER_KIND_DEBUGGER: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("provider.debugger").unwrap());

/// Provider kind: a symbolic executor (e.g., angr, Z3).
pub static PROVIDER_KIND_SYMBOLIC_EXECUTOR: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("provider.symbolic-executor").unwrap());

/// Provider kind: a large language model.
pub static PROVIDER_KIND_LLM: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("provider.llm").unwrap());

/// Provider kind: a human analyst.
pub static PROVIDER_KIND_HUMAN: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("provider.human").unwrap());

// ---------------------------------------------------------------------------
// ProviderRun
// ---------------------------------------------------------------------------

/// A single execution run of an analysis provider within a project.
///
/// Tracks the provider, operation, input artifacts, configuration,
/// environment identity, timestamps, and status.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRun {
    pub id: ProviderRunId,
    pub project: ProjectId,
    pub provider: ProviderId,
    pub operation: NamespacedId,
    pub input_artifacts: Vec<ArtifactId>,
    pub configuration_artifact: Option<ArtifactId>,
    pub configuration_hash: ContentHash,
    pub environment: EnvironmentIdentity,
    pub started_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub status: ProviderRunStatus,
}

impl ProviderRun {
    /// Transitions this run's status to the target, validating the
    /// state machine. Sets `completed_at` when moving to a terminal state.
    pub fn complete(&mut self, target: ProviderRunStatus) -> autore_core::Result<()> {
        self.status.transition(target)?;
        self.status = target;
        self.completed_at = Some(Timestamp::now());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ProviderEntityAlias
// ---------------------------------------------------------------------------

/// Maps a provider-specific identifier to a canonical `SemanticEntity`.
///
/// Providers use their own naming conventions (function names, addresses,
/// symbol IDs). Aliases bridge these to canonical entities without
/// cross-provider alignment — each provider's aliases are independent.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProviderEntityAlias {
    pub provider_run: ProviderRunId,
    pub provider_kind: NamespacedId,
    pub provider_identifier: String,
    pub entity: EntityId,
}

// ---------------------------------------------------------------------------
// NativeArtifact
// ---------------------------------------------------------------------------

/// A native-format artifact produced by a provider run.
///
/// Links a content-addressed `Artifact` to the provider run that created it,
/// with a format identifier and optional subject entities. The content is
/// opaque — Stage 0 does NOT parse native artifacts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NativeArtifact {
    pub id: NativeArtifactId,
    pub provider_run: ProviderRunId,
    pub artifact: ArtifactId,
    pub format: NamespacedId,
    pub subject_entities: Vec<EntityId>,
    pub description: Option<String>,
}

// ---------------------------------------------------------------------------
// Native format constants
// ---------------------------------------------------------------------------

/// Native format: Hex-Rays decompiler pseudocode output.
pub static NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("ida.hexrays.pseudocode").unwrap());

/// Native format: IDA microcode output.
pub static NATIVE_FORMAT_IDA_MICROCODE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("ida.microcode").unwrap());

/// Native format: Ghidra P-code output.
pub static NATIVE_FORMAT_GHIDRA_PCODE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("ghidra.pcode").unwrap());

/// Native format: GDB execution trace.
pub static NATIVE_FORMAT_GDB_TRACE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("gdb.trace").unwrap());

/// Native format: Z3 solver model output.
pub static NATIVE_FORMAT_Z3_MODEL: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("z3.model").unwrap());

/// Native format: Raw LLM response text.
pub static NATIVE_FORMAT_LLM_RAW_RESPONSE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("llm.raw-response").unwrap());

// ---------------------------------------------------------------------------
// EvidenceLifecycleState
// ---------------------------------------------------------------------------

/// The lifecycle state of an evidence record.
///
/// Evidence records are immutable once inserted. Their lifecycle is tracked
/// via append-only `EvidenceLifecycleEvent` records. The valid states are:
/// - `Active`: the evidence is current and usable.
/// - `Superseded`: newer evidence replaces this one.
/// - `Invalidated`: the evidence was found to be incorrect.
/// - `Unavailable`: the evidence source is no longer accessible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EvidenceLifecycleState {
    Active,
    Superseded,
    Invalidated,
    Unavailable,
}

impl std::fmt::Display for EvidenceLifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvidenceLifecycleState::Active => write!(f, "Active"),
            EvidenceLifecycleState::Superseded => write!(f, "Superseded"),
            EvidenceLifecycleState::Invalidated => write!(f, "Invalidated"),
            EvidenceLifecycleState::Unavailable => write!(f, "Unavailable"),
        }
    }
}

// ---------------------------------------------------------------------------
// Assumption
// ---------------------------------------------------------------------------

/// An assumption underlying a piece of evidence.
///
/// Each assumption has a human-readable description and an optional reference
/// to another evidence record that supports it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Assumption {
    pub description: String,
    pub evidence: Option<EvidenceRecordId>,
}

// ---------------------------------------------------------------------------
// EvidenceRecord
// ---------------------------------------------------------------------------

/// An immutable evidence record supporting or refuting observations about
/// a semantic entity within a project.
///
/// Evidence records are append-only: once inserted, they are NEVER updated or
/// deleted. Lifecycle changes (supersession, invalidation) are tracked via
/// `EvidenceLifecycleEvent` records in a separate table.
///
/// The `value` field carries the actual observation (typed `EvidenceValue`),
/// and `derivation` records how the evidence was produced. The `native_artifacts`
/// field stores opaque references to `NativeArtifact` IDs (no FK — stored as
/// a JSON array of UUID strings).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceRecord {
    pub id: EvidenceRecordId,
    pub project: ProjectId,
    pub subject: EntityId,
    pub predicate: NamespacedId,
    pub value: EvidenceValue,
    pub derivation: crate::domain::Derivation,
    pub provider_run: Option<ProviderRunId>,
    pub native_artifacts: Vec<NativeArtifactId>,
    pub assumptions: Vec<Assumption>,
    pub created_at: Timestamp,
}

// ---------------------------------------------------------------------------
// EvidenceLifecycleEvent
// ---------------------------------------------------------------------------

/// An append-only event recording a lifecycle state change for an evidence
/// record.
///
/// Events are never updated or deleted. The full lifecycle history of an
/// evidence record is the ordered sequence of events for that evidence ID.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvidenceLifecycleEvent {
    pub evidence: EvidenceRecordId,
    pub timestamp: Timestamp,
    pub state: EvidenceLifecycleState,
    pub reason: Option<String>,
    pub caused_by: Option<EvidenceRecordId>,
}

// ---------------------------------------------------------------------------
// Evidence predicate constants
// ---------------------------------------------------------------------------

/// Evidence predicate: the name of a function.
pub static EVIDENCE_PREDICATE_FUNCTION_NAME: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("evidence.predicate.function-name").unwrap());

/// Evidence predicate: the signature of a function.
pub static EVIDENCE_PREDICATE_FUNCTION_SIGNATURE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("evidence.predicate.function-signature").unwrap());

/// Evidence predicate: a call target reference.
pub static EVIDENCE_PREDICATE_CALL_TARGET: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("evidence.predicate.call-target").unwrap());

/// Evidence predicate: a string reference.
pub static EVIDENCE_PREDICATE_STRING_REFERENCE: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("evidence.predicate.string-reference").unwrap());

/// Evidence predicate: type information.
pub static EVIDENCE_PREDICATE_TYPE_INFO: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("evidence.predicate.type-info").unwrap());

/// Evidence predicate: control flow observation.
pub static EVIDENCE_PREDICATE_CONTROL_FLOW: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("evidence.predicate.control-flow").unwrap());

// ---------------------------------------------------------------------------
// HypothesisStatus
// ---------------------------------------------------------------------------

/// The lifecycle status of a hypothesis.
///
/// Valid transitions (§13):
/// - Proposed -> UnderInvestigation
/// - UnderInvestigation -> Accepted, Rejected
/// - Accepted -> Superseded { by }
/// - All other transitions are invalid.
///
/// Changing confidence does NOT change status (§13).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HypothesisStatus {
    Proposed,
    UnderInvestigation,
    Accepted,
    Rejected,
    Superseded { by: HypothesisId },
}

impl HypothesisStatus {
    /// Validates a state transition from `self` to `target`.
    ///
    /// Only the documented transitions are allowed; all others
    /// return `Error::InvalidStateTransition`.
    pub fn transition(&self, target: &HypothesisStatus) -> autore_core::Result<()> {
        match (self, target) {
            (HypothesisStatus::Proposed, HypothesisStatus::UnderInvestigation) => Ok(()),
            (HypothesisStatus::UnderInvestigation, HypothesisStatus::Accepted) => Ok(()),
            (HypothesisStatus::UnderInvestigation, HypothesisStatus::Rejected) => Ok(()),
            (HypothesisStatus::Accepted, HypothesisStatus::Superseded { .. }) => Ok(()),
            _ => Err(autore_core::Error::InvalidStateTransition(format!(
                "{self} -> {target}"
            ))),
        }
    }

    /// Returns the discriminant string for database storage and filtering.
    pub fn kind(&self) -> &'static str {
        match self {
            HypothesisStatus::Proposed => "Proposed",
            HypothesisStatus::UnderInvestigation => "UnderInvestigation",
            HypothesisStatus::Accepted => "Accepted",
            HypothesisStatus::Rejected => "Rejected",
            HypothesisStatus::Superseded { .. } => "Superseded",
        }
    }

    /// Returns `true` if this status is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            HypothesisStatus::Accepted | HypothesisStatus::Rejected | HypothesisStatus::Superseded { .. }
        )
    }
}

impl std::fmt::Display for HypothesisStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HypothesisStatus::Proposed => write!(f, "Proposed"),
            HypothesisStatus::UnderInvestigation => write!(f, "UnderInvestigation"),
            HypothesisStatus::Accepted => write!(f, "Accepted"),
            HypothesisStatus::Rejected => write!(f, "Rejected"),
            HypothesisStatus::Superseded { by } => write!(f, "Superseded({by})"),
        }
    }
}

// ---------------------------------------------------------------------------
// Hypothesis status constants
// ---------------------------------------------------------------------------

pub static HYPOTHESIS_STATUS_PROPOSED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("hypothesis.status.proposed").unwrap());

pub static HYPOTHESIS_STATUS_UNDER_INVESTIGATION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("hypothesis.status.under-investigation").unwrap());

pub static HYPOTHESIS_STATUS_ACCEPTED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("hypothesis.status.accepted").unwrap());

pub static HYPOTHESIS_STATUS_REJECTED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("hypothesis.status.rejected").unwrap());

pub static HYPOTHESIS_STATUS_SUPERSEDED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("hypothesis.status.superseded").unwrap());

// ---------------------------------------------------------------------------
// Hypothesis
// ---------------------------------------------------------------------------

/// A hypothesis about a semantic entity within a project.
///
/// Hypotheses are proposed explanations or predictions supported by
/// evidence. They follow a state machine (Proposed → UnderInvestigation
/// → Accepted/Rejected → Superseded). Accepting one hypothesis does NOT
/// auto-delete competitors (§13). Supersession chains must be acyclic.
///
/// NOTE: `supporting_evidence` and `contradicting_evidence` use
/// `EvidenceRecordId` (Stage 0), NOT the M1 `EvidenceId`. This is an
/// intentional deviation from the plan text to align with Stage 0's
/// evidence model (Task 17).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hypothesis {
    pub id: HypothesisId,
    pub project: ProjectId,
    pub subject: EntityId,
    pub predicate: NamespacedId,
    pub candidate: EvidenceValue,
    pub supporting_evidence: Vec<EvidenceRecordId>,
    pub contradicting_evidence: Vec<EvidenceRecordId>,
    pub derived_from: Vec<HypothesisId>,
    pub confidence: Confidence,
    pub status: HypothesisStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Hypothesis {
    /// Creates a new hypothesis in `Proposed` status.
    pub fn new(
        project: ProjectId,
        subject: EntityId,
        predicate: NamespacedId,
        candidate: EvidenceValue,
    ) -> Self {
        let now = Timestamp::now();
        Hypothesis {
            id: HypothesisId::new(),
            project,
            subject,
            predicate,
            candidate,
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            derived_from: vec![],
            confidence: Confidence::new(0.5).expect("0.5 is valid"),
            status: HypothesisStatus::Proposed,
            created_at: now,
            updated_at: now,
        }
    }

    /// Transitions this hypothesis to the target status, validating the
    /// state machine.
    pub fn transition(&mut self, target: HypothesisStatus) -> autore_core::Result<()> {
        self.status.transition(&target)?;
        self.status = target;
        self.updated_at = Timestamp::now();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ContradictionStatus
// ---------------------------------------------------------------------------

/// The lifecycle status of a contradiction.
///
/// Valid transitions (§14):
/// - Open -> Investigating, Resolved, Deferred
/// - Investigating -> Resolved, Deferred
/// - Deferred -> Open (reopened for further analysis)
/// - Resolved is terminal.
///
/// `ContradictionResolution` captures the resolution metadata when the
/// contradiction transitions to `Resolved`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ContradictionStatus {
    Open,
    Investigating,
    Resolved,
    Deferred,
}

impl ContradictionStatus {
    /// Validates a state transition from `self` to `target`.
    pub fn transition(&self, target: &ContradictionStatus) -> autore_core::Result<()> {
        match (self, target) {
            (ContradictionStatus::Open, ContradictionStatus::Investigating) => Ok(()),
            (ContradictionStatus::Open, ContradictionStatus::Resolved) => Ok(()),
            (ContradictionStatus::Open, ContradictionStatus::Deferred) => Ok(()),
            (ContradictionStatus::Investigating, ContradictionStatus::Resolved) => Ok(()),
            (ContradictionStatus::Investigating, ContradictionStatus::Deferred) => Ok(()),
            (ContradictionStatus::Deferred, ContradictionStatus::Open) => Ok(()),
            _ => Err(autore_core::Error::InvalidStateTransition(format!(
                "{self} -> {target}"
            ))),
        }
    }

    /// Returns the discriminant string for database storage and filtering.
    pub fn kind(&self) -> &'static str {
        match self {
            ContradictionStatus::Open => "Open",
            ContradictionStatus::Investigating => "Investigating",
            ContradictionStatus::Resolved => "Resolved",
            ContradictionStatus::Deferred => "Deferred",
        }
    }

    /// Returns `true` if this status is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, ContradictionStatus::Resolved)
    }
}

impl std::fmt::Display for ContradictionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContradictionStatus::Open => write!(f, "Open"),
            ContradictionStatus::Investigating => write!(f, "Investigating"),
            ContradictionStatus::Resolved => write!(f, "Resolved"),
            ContradictionStatus::Deferred => write!(f, "Deferred"),
        }
    }
}

// ---------------------------------------------------------------------------
// Contradiction status constants
// ---------------------------------------------------------------------------

pub static CONTRADICTION_STATUS_OPEN: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("contradiction.status.open").unwrap());

pub static CONTRADICTION_STATUS_INVESTIGATING: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("contradiction.status.investigating").unwrap());

pub static CONTRADICTION_STATUS_RESOLVED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("contradiction.status.resolved").unwrap());

pub static CONTRADICTION_STATUS_DEFERRED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("contradiction.status.deferred").unwrap());

// ---------------------------------------------------------------------------
// ContradictionResolution
// ---------------------------------------------------------------------------

/// Resolution metadata attached when a contradiction transitions to
/// `Resolved`.
///
/// `chosen` lists the hypotheses that were selected as the preferred
/// explanation; `rationale` explains the decision. `resolution` is a
/// namespaced identifier for the resolution kind (extensible).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContradictionResolution {
    pub resolved_at: Timestamp,
    pub resolution: NamespacedId,
    pub chosen: Vec<HypothesisId>,
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Contradiction
// ---------------------------------------------------------------------------

/// A detected contradiction between two or more hypotheses about the same
/// subject entity and predicate.
///
/// Contradictions are RECORDED (not auto-detected — §14). They track the
/// set of competing hypotheses and supporting/contradicting evidence.
/// Resolving a contradiction attaches a `ContradictionResolution` but
/// does NOT auto-delete the competing hypotheses.
///
/// NOTE: `evidence` uses `EvidenceRecordId` (Stage 0), NOT the M1
/// `EvidenceId`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Contradiction {
    pub id: ContradictionId,
    pub project: ProjectId,
    pub subject: EntityId,
    pub predicate: NamespacedId,
    pub evidence: Vec<EvidenceRecordId>,
    pub hypotheses: Vec<HypothesisId>,
    pub status: ContradictionStatus,
    pub resolution: Option<ContradictionResolution>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Contradiction {
    /// Creates a new contradiction in `Open` status with empty resolution.
    pub fn new(
        project: ProjectId,
        subject: EntityId,
        predicate: NamespacedId,
        evidence: Vec<EvidenceRecordId>,
        hypotheses: Vec<HypothesisId>,
    ) -> Self {
        let now = Timestamp::now();
        Contradiction {
            id: ContradictionId::new(),
            project,
            subject,
            predicate,
            evidence,
            hypotheses,
            status: ContradictionStatus::Open,
            resolution: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Transitions this contradiction to the target status, validating the
    /// state machine. When transitioning to `Resolved`, a resolution MUST
    /// be provided; for other transitions, `resolution` must be `None`.
    pub fn transition(
        &mut self,
        target: ContradictionStatus,
        resolution: Option<ContradictionResolution>,
    ) -> autore_core::Result<()> {
        self.status.transition(&target)?;
        match (&target, &resolution) {
            (ContradictionStatus::Resolved, Some(_)) => {}
            (ContradictionStatus::Resolved, None) => {
                return Err(autore_core::Error::Validation(
                    "resolution is required when transitioning to Resolved".into(),
                ));
            }
            (_, Some(_)) => {
                return Err(autore_core::Error::Validation(
                    "resolution must be None when not transitioning to Resolved".into(),
                ));
            }
            (_, None) => {}
        }
        self.status = target;
        self.resolution = resolution;
        self.updated_at = Timestamp::now();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// VerificationSubject
// ---------------------------------------------------------------------------

/// The subject of a verification record — one of four supported domain
/// types (§15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VerificationSubject {
    Entity(EntityId),
    Hypothesis(HypothesisId),
    Artifact(ArtifactId),
    GenerationTarget(GenerationTargetId),
}

impl VerificationSubject {
    /// Returns the discriminator string used in database storage.
    pub fn kind(&self) -> &'static str {
        match self {
            VerificationSubject::Entity(_) => "Entity",
            VerificationSubject::Hypothesis(_) => "Hypothesis",
            VerificationSubject::Artifact(_) => "Artifact",
            VerificationSubject::GenerationTarget(_) => "GenerationTarget",
        }
    }

    /// Returns the inner UUID (across all variants).
    pub fn id_uuid(&self) -> uuid::Uuid {
        match self {
            VerificationSubject::Entity(id) => *id.as_uuid(),
            VerificationSubject::Hypothesis(id) => *id.as_uuid(),
            VerificationSubject::Artifact(id) => *id.as_uuid(),
            VerificationSubject::GenerationTarget(id) => *id.as_uuid(),
        }
    }
}

impl std::fmt::Display for VerificationSubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationSubject::Entity(id) => write!(f, "Entity({id})"),
            VerificationSubject::Hypothesis(id) => write!(f, "Hypothesis({id})"),
            VerificationSubject::Artifact(id) => write!(f, "Artifact({id})"),
            VerificationSubject::GenerationTarget(id) => write!(f, "GenerationTarget({id})"),
        }
    }
}

// ---------------------------------------------------------------------------
// VerificationState
// ---------------------------------------------------------------------------

/// The state of a verification check (§15). Closed finite state machine.
///
/// Valid transitions:
/// - NotChecked -> Pending
/// - Pending -> Passed, Failed, Inconclusive, Blocked
/// - Blocked -> Pending (retry after unblocking)
/// - All terminal states (Passed, Failed, Inconclusive) cannot transition
///   further.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VerificationState {
    NotChecked,
    Pending,
    Passed,
    Failed,
    Inconclusive,
    Blocked,
}

impl VerificationState {
    /// Validates a state transition from `self` to `target`.
    pub fn transition(&self, target: &VerificationState) -> autore_core::Result<()> {
        match (self, target) {
            (VerificationState::NotChecked, VerificationState::Pending) => Ok(()),
            (VerificationState::Pending, VerificationState::Passed)
            | (VerificationState::Pending, VerificationState::Failed)
            | (VerificationState::Pending, VerificationState::Inconclusive)
            | (VerificationState::Pending, VerificationState::Blocked) => Ok(()),
            (VerificationState::Blocked, VerificationState::Pending) => Ok(()),
            _ => Err(autore_core::Error::InvalidStateTransition(format!(
                "{self:?} -> {target:?}"
            ))),
        }
    }

    /// Returns the discriminant string for database storage.
    pub fn kind(&self) -> &'static str {
        match self {
            VerificationState::NotChecked => "NotChecked",
            VerificationState::Pending => "Pending",
            VerificationState::Passed => "Passed",
            VerificationState::Failed => "Failed",
            VerificationState::Inconclusive => "Inconclusive",
            VerificationState::Blocked => "Blocked",
        }
    }

    /// Returns `true` if this state is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            VerificationState::Passed | VerificationState::Failed | VerificationState::Inconclusive
        )
    }
}

impl std::fmt::Display for VerificationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerificationState::NotChecked => write!(f, "NotChecked"),
            VerificationState::Pending => write!(f, "Pending"),
            VerificationState::Passed => write!(f, "Passed"),
            VerificationState::Failed => write!(f, "Failed"),
            VerificationState::Inconclusive => write!(f, "Inconclusive"),
            VerificationState::Blocked => write!(f, "Blocked"),
        }
    }
}

// ---------------------------------------------------------------------------
// Verification check constants (§15)
// ---------------------------------------------------------------------------

/// Verification check: artifact content hash matches expected.
pub static VERIFICATION_CHECK_ARTIFACT_HASH: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.artifact.hash").unwrap());

/// Verification check: project integrity validation.
pub static VERIFICATION_CHECK_PROJECT_INTEGRITY: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.project.integrity").unwrap());

/// Verification check: build verification.
pub static VERIFICATION_CHECK_BUILD: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("verification.build").unwrap());

/// Verification check: ABI layout verification.
pub static VERIFICATION_CHECK_ABI_LAYOUT: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("verification.abi.layout").unwrap());

/// Verification check: differential behavior verification.
pub static VERIFICATION_CHECK_DIFFERENTIAL_BEHAVIOR: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("verification.differential.behavior").unwrap());

// ---------------------------------------------------------------------------
// VerificationRecord
// ---------------------------------------------------------------------------

/// A generic verification record — the result of running a named check
/// against a subject (entity, hypothesis, artifact, or generation target).
///
/// Verification is generic (§15): Stage 0 does not interpret the check
/// semantics. Recording a verification does NOT change hypothesis
/// confidence (§3.5 / §15).
///
/// NOTE: `evidence` uses `EvidenceRecordId` (Stage 0).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VerificationRecord {
    pub id: VerificationRecordId,
    pub project: ProjectId,
    pub subject: VerificationSubject,
    pub check: NamespacedId,
    pub state: VerificationState,
    pub provider_run: Option<ProviderRunId>,
    pub evidence: Vec<EvidenceRecordId>,
    pub details: Option<ExtensionData>,
    pub created_at: Timestamp,
}

impl VerificationRecord {
    /// Creates a new verification record in `NotChecked` state.
    pub fn new(
        project: ProjectId,
        subject: VerificationSubject,
        check: NamespacedId,
    ) -> Self {
        VerificationRecord {
            id: VerificationRecordId::new(),
            project,
            subject,
            check,
            state: VerificationState::NotChecked,
            provider_run: None,
            evidence: vec![],
            details: None,
            created_at: Timestamp::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// EventSource + EventSubject (shared by Operation + future ProjectEvent)
// ---------------------------------------------------------------------------

/// The source of a domain event — identifies what kind of record triggered it.
///
/// Designed to be shareable between `Operation` events and `ProjectEvent`
/// records (Task 21).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EventSource {
    Operation,
    Project,
    Artifact,
    Entity,
    Evidence,
    Hypothesis,
    Contradiction,
    Verification,
    Provider,
}

impl std::fmt::Display for EventSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSource::Operation => write!(f, "Operation"),
            EventSource::Project => write!(f, "Project"),
            EventSource::Artifact => write!(f, "Artifact"),
            EventSource::Entity => write!(f, "Entity"),
            EventSource::Evidence => write!(f, "Evidence"),
            EventSource::Hypothesis => write!(f, "Hypothesis"),
            EventSource::Contradiction => write!(f, "Contradiction"),
            EventSource::Verification => write!(f, "Verification"),
            EventSource::Provider => write!(f, "Provider"),
        }
    }
}

/// The subject of a domain event — identifies which specific record it refers to.
///
/// Uses a discriminator + UUID pattern matching `VerificationSubject`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "id")]
pub enum EventSubject {
    Operation(OperationId),
    Project(ProjectId),
    Artifact(ArtifactId),
    Entity(EntityId),
    Evidence(EvidenceRecordId),
    Hypothesis(HypothesisId),
    Contradiction(ContradictionId),
    Verification(VerificationRecordId),
}

// ---------------------------------------------------------------------------
// MetricMap
// ---------------------------------------------------------------------------

/// Structured metrics attached to a progress update.
///
/// Keys are namespaced identifiers; values are floating-point measurements.
/// Uses `BTreeMap` for deterministic serialization ordering.
pub type MetricMap = BTreeMap<NamespacedId, f64>;

// ---------------------------------------------------------------------------
// OperationFailure
// ---------------------------------------------------------------------------

/// Failure details captured when an operation transitions to `Failed`.
///
/// Stored as JSON TEXT in the `operations.failure` column.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OperationFailure {
    /// A namespaced identifier for the failure kind (extensible).
    pub code: NamespacedId,
    /// Human-readable description of the failure.
    pub message: String,
    /// Optional structured details for diagnostics.
    pub details: Option<ExtensionData>,
}

// ---------------------------------------------------------------------------
// Operation
// ---------------------------------------------------------------------------

/// A long-running operation within a project.
///
/// Operations track work like artifact imports, project validation,
/// migrations, and index rebuilds. They follow a state machine
/// (`OperationState`) with structured progress updates and cooperative
/// cancellation.
///
/// `subject` is optional JSON describing what the operation targets
/// (stored as `Option<EventSubject>`). `requested_by` is a string
/// discriminant for the requester (e.g. "cli", "tui", "system").
/// `failure` is optional JSON with `OperationFailure` details.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Operation {
    pub id: OperationId,
    pub project: ProjectId,
    pub kind: NamespacedId,
    pub state: OperationState,
    pub subject: Option<EventSubject>,
    pub requested_by: String,
    pub parent: Option<OperationId>,
    pub failure: Option<OperationFailure>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Operation {
    /// Creates a new operation in `Queued` state.
    pub fn new(
        project: ProjectId,
        kind: NamespacedId,
        requested_by: impl Into<String>,
    ) -> Self {
        let now = Timestamp::now();
        Operation {
            id: OperationId::new(),
            project,
            kind,
            state: OperationState::Queued,
            subject: None,
            requested_by: requested_by.into(),
            parent: None,
            failure: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Transitions this operation to the target state, validating the
    /// state machine.
    pub fn transition(&mut self, target: OperationState) -> autore_core::Result<()> {
        self.state.transition(&target)?;
        self.state = target;
        self.updated_at = Timestamp::now();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ProgressUpdate
// ---------------------------------------------------------------------------

/// A structured progress update for an operation.
///
/// Sequence numbers are per-operation (not global). Each operation
/// maintains its own monotonic sequence starting from 0.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProgressUpdate {
    pub id: uuid::Uuid,
    pub operation_id: OperationId,
    pub sequence: u64,
    pub message: String,
    pub metrics: MetricMap,
    pub created_at: Timestamp,
}

impl ProgressUpdate {
    /// Creates a new progress update with the given sequence number.
    pub fn new(
        operation_id: OperationId,
        sequence: u64,
        message: impl Into<String>,
        metrics: MetricMap,
    ) -> Self {
        ProgressUpdate {
            id: uuid::Uuid::now_v7(),
            operation_id,
            sequence,
            message: message.into(),
            metrics,
            created_at: Timestamp::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// CancellationRequest
// ---------------------------------------------------------------------------

/// A cooperative cancellation request for an operation.
///
/// Cancellation is cooperative — the request is recorded, not forced.
/// The operation checks for pending requests and transitions to
/// `Cancelling` when it next yields.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CancellationRequest {
    pub id: uuid::Uuid,
    pub operation_id: OperationId,
    pub requested_by: String,
    pub reason: Option<String>,
    pub created_at: Timestamp,
}

impl CancellationRequest {
    /// Creates a new cancellation request.
    pub fn new(
        operation_id: OperationId,
        requested_by: impl Into<String>,
        reason: Option<String>,
    ) -> Self {
        CancellationRequest {
            id: uuid::Uuid::now_v7(),
            operation_id,
            requested_by: requested_by.into(),
            reason,
            created_at: Timestamp::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 0 operation kind constants
// ---------------------------------------------------------------------------

/// Operation kind: importing an artifact.
pub static OPERATION_KIND_ARTIFACT_IMPORT: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.artifact.import").unwrap());

/// Operation kind: project validation.
pub static OPERATION_KIND_PROJECT_VALIDATION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.project.validation").unwrap());

/// Operation kind: project migration.
pub static OPERATION_KIND_PROJECT_MIGRATION: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.project.migration").unwrap());

/// Operation kind: rebuilding derived indexes.
pub static OPERATION_KIND_PROJECT_REBUILD_INDEXES: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.project.rebuild-indexes").unwrap());

/// Operation kind: external artifact integrity check.
pub static OPERATION_KIND_EXTERNAL_ARTIFACT_CHECK: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| {
        NamespacedId::parse("core.project.external-artifact-check").unwrap()
    });

// ---------------------------------------------------------------------------
// ProjectEvent
// ---------------------------------------------------------------------------

/// An append-only event in a project's event stream.
///
/// Every meaningful state change emits a `ProjectEvent` in the same
/// SQLite transaction as the state mutation. Events are ordered by a
/// monotonic per-project `sequence` number (not by timestamp).
///
/// `kind` is a `NamespacedId` describing what happened (e.g.
/// `core.project.created`). `subject` identifies the record affected.
/// `source` says which domain entity triggered the event. `payload`
/// carries optional structured extension data.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectEvent {
    pub id: ProjectEventId,
    pub project: ProjectId,
    pub sequence: u64,
    pub kind: NamespacedId,
    pub subject: Option<EventSubject>,
    pub source: EventSource,
    pub payload: Option<ExtensionData>,
    pub created_at: Timestamp,
}

impl ProjectEvent {
    /// Creates a new project event.
    ///
    /// The `sequence` must be computed inside the transaction that
    /// inserts it (via `next_project_event_sequence`). Pass 0 as a
    /// placeholder if the sequence will be overwritten before insert.
    pub fn new(
        project: ProjectId,
        sequence: u64,
        kind: NamespacedId,
        source: EventSource,
        subject: Option<EventSubject>,
        payload: Option<ExtensionData>,
    ) -> Self {
        ProjectEvent {
            id: ProjectEventId::new(),
            project,
            sequence,
            kind,
            subject,
            source,
            payload,
            created_at: Timestamp::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Stage 0 project event kind constants
// ---------------------------------------------------------------------------

pub static EVENT_KIND_PROJECT_CREATED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.project.created").unwrap());

pub static EVENT_KIND_ARTIFACT_REGISTERED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.artifact.registered").unwrap());

pub static EVENT_KIND_ARTIFACT_EXTERNAL_CHANGED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.artifact.external-changed").unwrap());

pub static EVENT_KIND_ENTITY_CREATED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.entity.created").unwrap());

pub static EVENT_KIND_EVIDENCE_ADDED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.evidence.added").unwrap());

pub static EVENT_KIND_EVIDENCE_INVALIDATED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.evidence.invalidated").unwrap());

pub static EVENT_KIND_HYPOTHESIS_PROPOSED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.hypothesis.proposed").unwrap());

pub static EVENT_KIND_HYPOTHESIS_ACCEPTED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.hypothesis.accepted").unwrap());

pub static EVENT_KIND_HYPOTHESIS_REJECTED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.hypothesis.rejected").unwrap());

pub static EVENT_KIND_CONTRADICTION_CREATED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.contradiction.created").unwrap());

pub static EVENT_KIND_VERIFICATION_RECORDED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.verification.recorded").unwrap());

pub static EVENT_KIND_OPERATION_QUEUED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.operation.queued").unwrap());

pub static EVENT_KIND_OPERATION_STARTED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.operation.started").unwrap());

pub static EVENT_KIND_OPERATION_PROGRESS: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.operation.progress").unwrap());

pub static EVENT_KIND_OPERATION_COMPLETED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.operation.completed").unwrap());

pub static EVENT_KIND_OPERATION_FAILED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.operation.failed").unwrap());

pub static EVENT_KIND_PROJECT_VALIDATION_FAILED: std::sync::LazyLock<NamespacedId> =
    std::sync::LazyLock::new(|| NamespacedId::parse("core.project.validation-failed").unwrap());

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ExtensionData, MetadataMap, NamespacedId};
    use crate::ids::ArtifactId;
    use std::path::PathBuf;

    fn sample_project() -> Project {
        Project::new("test-project")
    }

    fn sample_artifact(project: &Project) -> Artifact {
        Artifact {
            id: ArtifactId::new(),
            project: project.id,
            kind: ARTIFACT_KIND_BINARY.clone(),
            content_hash: ContentHash::sha256(b"hello world"),
            size: 11,
            storage: ArtifactStorage::ManagedBlob {
                relative_path: PathBuf::from("sha256/ab/cdef0123"),
            },
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        }
    }

    #[test]
    fn project_new_sets_name_and_timestamps() {
        let p = Project::new("my-project");
        assert_eq!(p.name, "my-project");
        assert_eq!(p.schema_version, SchemaVersion::new(2, 0));
        assert!(p.metadata.is_empty());
        assert_eq!(p.created_at, p.updated_at);
    }

    #[test]
    fn project_touch_updates_updated_at() {
        let mut p = Project::new("touch-test");
        let original = *p.updated_at.as_offset_datetime();
        std::thread::sleep(std::time::Duration::from_millis(5));
        p.touch();
        assert!(p.updated_at.as_offset_datetime() > &original);
    }

    #[test]
    fn project_round_trip_json() {
        let p = sample_project();
        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(p.id, back.id);
        assert_eq!(p.name, back.name);
        assert_eq!(p.schema_version, back.schema_version);
        assert_eq!(p.created_at, back.created_at);
        assert_eq!(p.metadata, back.metadata);
    }

    #[test]
    fn project_metadata_is_typed_not_raw_json() {
        let mut p = sample_project();
        let schema = NamespacedId::parse("core.test").unwrap();
        let ext = ExtensionData::new(schema.clone(), 1, serde_json::json!({"key": "value"}));
        p.metadata.insert(schema, ext);
        assert_eq!(p.metadata.len(), 1);

        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.metadata.len(), 1);
    }

    #[test]
    fn artifact_round_trip_managed() {
        let p = sample_project();
        let a = sample_artifact(&p);
        let json = serde_json::to_string_pretty(&a).unwrap();
        let back: Artifact = serde_json::from_str(&json).unwrap();
        assert_eq!(a.id, back.id);
        assert_eq!(a.project, back.project);
        assert_eq!(a.kind, back.kind);
        assert_eq!(a.content_hash, back.content_hash);
        assert_eq!(a.size, back.size);
        assert_eq!(a.storage, back.storage);
    }

    #[test]
    fn artifact_round_trip_external() {
        let p = sample_project();
        let a = Artifact {
            id: ArtifactId::new(),
            project: p.id,
            kind: ARTIFACT_KIND_SOURCE_TREE.clone(),
            content_hash: ContentHash::sha256(b"external content"),
            size: 1024,
            storage: ArtifactStorage::ExternalFile {
                canonical_path: PathBuf::from("/usr/lib/libc.so.6"),
            },
            created_at: Timestamp::now(),
            metadata: MetadataMap::new(),
        };
        let json = serde_json::to_string_pretty(&a).unwrap();
        let back: Artifact = serde_json::from_str(&json).unwrap();
        assert_eq!(a.storage, back.storage);
    }

    #[test]
    fn artifact_kinds_registered() {
        assert_eq!(ARTIFACT_KIND_BINARY.to_string(), "core.binary");
        assert_eq!(ARTIFACT_KIND_SOURCE_TREE.to_string(), "core.source-tree");
        assert_eq!(
            ARTIFACT_KIND_NATIVE_PROVIDER_OUTPUT.to_string(),
            "core.native-provider-output"
        );
        assert_eq!(ARTIFACT_KIND_CONFIGURATION.to_string(), "core.configuration");
        assert_eq!(ARTIFACT_KIND_LOG.to_string(), "core.log");
        assert_eq!(ARTIFACT_KIND_TRACE.to_string(), "core.trace");
        assert_eq!(
            ARTIFACT_KIND_GENERATED_CANDIDATE.to_string(),
            "core.generated-candidate"
        );
    }

    #[test]
    fn binary_artifact_metadata_round_trip() {
        let meta = BinaryArtifactMetadata {
            format: Some(NamespacedId::parse("core.elf").unwrap()),
            architecture: Some(NamespacedId::parse("core.x86-64").unwrap()),
            endianness: Some(Endianness::Little),
            preferred_image_base: Some(0x400000),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: BinaryArtifactMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn binary_artifact_metadata_all_none() {
        let meta = BinaryArtifactMetadata {
            format: None,
            architecture: None,
            endianness: None,
            preferred_image_base: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: BinaryArtifactMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn endianness_variants_serialize() {
        let le = serde_json::to_string(&Endianness::Little).unwrap();
        let be = serde_json::to_string(&Endianness::Big).unwrap();
        assert_eq!(le, "\"Little\"");
        assert_eq!(be, "\"Big\"");
        let le_back: Endianness = serde_json::from_str(&le).unwrap();
        assert_eq!(le_back, Endianness::Little);
    }

    // Fixture round-trip tests — enabled after fixtures are generated.
    #[test]
    fn project_fixture_round_trip() {
        let fixture = include_str!("../../tests/fixtures/project.json");
        let p: Project = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string_pretty(&p).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    #[test]
    fn artifact_fixture_managed_round_trip() {
        let fixture = include_str!("../../tests/fixtures/artifact_managed.json");
        let a: Artifact = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string_pretty(&a).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    #[test]
    fn artifact_fixture_external_round_trip() {
        let fixture = include_str!("../../tests/fixtures/artifact_external.json");
        let a: Artifact = serde_json::from_str(fixture).unwrap();
        let re_serialized = serde_json::to_string_pretty(&a).unwrap();
        assert_eq!(fixture.trim(), re_serialized.trim());
    }

    /// Helper to generate fixture JSON — run once, capture output, commit fixtures.
    #[test]
    fn generate_fixtures() {
        use crate::ids::ProjectId;
        use uuid::Uuid;

        let project_uuid = Uuid::parse_str("01906789-abcd-7000-8000-000000000001").unwrap();
        let artifact_uuid = Uuid::parse_str("01906789-abcd-7000-8000-000000000002").unwrap();
        let project_id = ProjectId::from_uuid(project_uuid);
        let artifact_id = ArtifactId::from_uuid(artifact_uuid);

        let ts = Timestamp::from_offset_datetime(
            time::OffsetDateTime::parse(
                "2026-01-15T10:30:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        );

        let project = Project {
            id: project_id,
            name: "fixture-project".into(),
            schema_version: SchemaVersion::new(2, 0),
            created_at: ts,
            updated_at: ts,
            metadata: MetadataMap::new(),
        };
        let project_json = serde_json::to_string_pretty(&project).unwrap();
        eprintln!("=== project.json ===\n{project_json}\n=== end ===");

        let artifact_managed = Artifact {
            id: artifact_id,
            project: project_id,
            kind: ARTIFACT_KIND_BINARY.clone(),
            content_hash: ContentHash::sha256(b"fixture binary content"),
            size: 2048,
            storage: ArtifactStorage::ManagedBlob {
                relative_path: PathBuf::from("sha256/b9/4d27b993456789abcdef0123456789abcdef0123456789abcdef01234567"),
            },
            created_at: ts,
            metadata: MetadataMap::new(),
        };
        let managed_json = serde_json::to_string_pretty(&artifact_managed).unwrap();
        eprintln!("=== artifact_managed.json ===\n{managed_json}\n=== end ===");

        let artifact_external = Artifact {
            id: ArtifactId::from_uuid(Uuid::parse_str("01906789-abcd-7000-8000-000000000003").unwrap()),
            project: project_id,
            kind: ARTIFACT_KIND_SOURCE_TREE.clone(),
            content_hash: ContentHash::sha256(b"external source tree"),
            size: 4096,
            storage: ArtifactStorage::ExternalFile {
                canonical_path: PathBuf::from("/home/user/projects/target-binary"),
            },
            created_at: ts,
            metadata: MetadataMap::new(),
        };
        let external_json = serde_json::to_string_pretty(&artifact_external).unwrap();
        eprintln!("=== artifact_external.json ===\n{external_json}\n=== end ===");
    }

    #[test]
    fn semantic_entity_round_trip_json() {
        use crate::domain::values::{BinaryLocation, ModuleIdentity};
        use crate::ids::BinaryArtifactId;

        let p = sample_project();
        let module = ModuleIdentity::new(
            Some(".text".into()),
            ContentHash::sha256(b"test module content"),
            Some(0),
        );
        let mut entity = SemanticEntity::new(
            p.id,
            ENTITY_KIND_FUNCTION.clone(),
            Some(StableEntityKey::BinaryLocation(BinaryLocation::new(
                BinaryArtifactId::new(),
                module,
                0x1000,
            ))),
            Some("main".to_string()),
        );
        entity.metadata = MetadataMap::new();

        let json = serde_json::to_string_pretty(&entity).unwrap();
        let back: SemanticEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(entity.id, back.id);
        assert_eq!(entity.project, back.project);
        assert_eq!(entity.kind, back.kind);
        assert_eq!(entity.stable_key, back.stable_key);
        assert_eq!(entity.display_name, back.display_name);
        assert_eq!(entity.created_at, back.created_at);
        assert_eq!(entity.metadata, back.metadata);
    }

    #[test]
    fn semantic_entity_null_stable_key_round_trip() {
        let p = sample_project();
        let entity = SemanticEntity::new(
            p.id,
            ENTITY_KIND_STRING.clone(),
            None,
            Some("hello world".to_string()),
        );

        let json = serde_json::to_string(&entity).unwrap();
        let back: SemanticEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(back.stable_key, None);
        assert_eq!(back.display_name, Some("hello world".to_string()));
    }

    #[test]
    fn entity_kinds_registered() {
        assert_eq!(ENTITY_KIND_FUNCTION.to_string(), "core.function");
        assert_eq!(ENTITY_KIND_TYPE.to_string(), "core.type");
        assert_eq!(ENTITY_KIND_GLOBAL.to_string(), "core.global");
        assert_eq!(ENTITY_KIND_STRING.to_string(), "core.string");
        assert_eq!(ENTITY_KIND_EXTERNAL_FUNCTION.to_string(), "core.external-function");
        assert_eq!(ENTITY_KIND_SOURCE_SYMBOL.to_string(), "core.source-symbol");
    }

    #[test]
    fn provider_run_status_valid_transitions() {
        let running = ProviderRunStatus::Running;
        assert!(running.transition(ProviderRunStatus::Completed).is_ok());
        assert!(running.transition(ProviderRunStatus::Failed).is_ok());
        assert!(running.transition(ProviderRunStatus::Cancelled).is_ok());
        assert!(running.transition(ProviderRunStatus::Inconclusive).is_ok());
    }

    #[test]
    fn provider_run_status_invalid_transitions() {
        assert!(ProviderRunStatus::Completed.transition(ProviderRunStatus::Running).is_err());
        assert!(ProviderRunStatus::Failed.transition(ProviderRunStatus::Completed).is_err());
        assert!(ProviderRunStatus::Cancelled.transition(ProviderRunStatus::Failed).is_err());
        assert!(ProviderRunStatus::Inconclusive.transition(ProviderRunStatus::Running).is_err());
        assert!(ProviderRunStatus::Running.transition(ProviderRunStatus::Running).is_err());
    }

    #[test]
    fn provider_run_status_terminal() {
        assert!(!ProviderRunStatus::Running.is_terminal());
        assert!(ProviderRunStatus::Completed.is_terminal());
        assert!(ProviderRunStatus::Failed.is_terminal());
        assert!(ProviderRunStatus::Cancelled.is_terminal());
        assert!(ProviderRunStatus::Inconclusive.is_terminal());
    }

    #[test]
    fn provider_run_status_display() {
        assert_eq!(ProviderRunStatus::Running.to_string(), "Running");
        assert_eq!(ProviderRunStatus::Completed.to_string(), "Completed");
        assert_eq!(ProviderRunStatus::Failed.to_string(), "Failed");
        assert_eq!(ProviderRunStatus::Cancelled.to_string(), "Cancelled");
        assert_eq!(ProviderRunStatus::Inconclusive.to_string(), "Inconclusive");
    }

    #[test]
    fn provider_run_status_serialize_round_trip() {
        let status = ProviderRunStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        let back: ProviderRunStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }

    #[test]
    fn provider_kinds_registered() {
        assert_eq!(PROVIDER_KIND_DISASSEMBLER.to_string(), "provider.disassembler");
        assert_eq!(PROVIDER_KIND_DECOMPILER.to_string(), "provider.decompiler");
        assert_eq!(PROVIDER_KIND_DEBUGGER.to_string(), "provider.debugger");
        assert_eq!(PROVIDER_KIND_SYMBOLIC_EXECUTOR.to_string(), "provider.symbolic-executor");
        assert_eq!(PROVIDER_KIND_LLM.to_string(), "provider.llm");
        assert_eq!(PROVIDER_KIND_HUMAN.to_string(), "provider.human");
    }

    #[test]
    fn provider_round_trip_json() {
        let p = Provider::new(
            "IDA Pro",
            PROVIDER_KIND_DECOMPILER.clone(),
            "8.3",
        );
        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: Provider = serde_json::from_str(&json).unwrap();
        assert_eq!(p.id, back.id);
        assert_eq!(p.name, back.name);
        assert_eq!(p.kind, back.kind);
        assert_eq!(p.version, back.version);
        assert_eq!(p.executable_hash, back.executable_hash);
    }

    #[test]
    fn environment_identity_round_trip_json() {
        let env = EnvironmentIdentity {
            operating_system: NamespacedId::parse("core.linux").unwrap(),
            architecture: NamespacedId::parse("core.x86-64").unwrap(),
            isolation_backend: Some(NamespacedId::parse("core.docker").unwrap()),
            image_digest: Some(ContentHash::sha256(b"test image")),
            extension: None,
        };
        let json = serde_json::to_string_pretty(&env).unwrap();
        let back: EnvironmentIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(env, back);
    }

    #[test]
    fn provider_run_complete_transitions() {
        use crate::ids::ProviderRunId;

        let env = EnvironmentIdentity {
            operating_system: NamespacedId::parse("core.linux").unwrap(),
            architecture: NamespacedId::parse("core.x86-64").unwrap(),
            isolation_backend: None,
            image_digest: None,
            extension: None,
        };
        let mut run = ProviderRun {
            id: ProviderRunId::new(),
            project: ProjectId::new(),
            provider: ProviderId::new(),
            operation: NamespacedId::parse("core.disassemble").unwrap(),
            input_artifacts: vec![],
            configuration_artifact: None,
            configuration_hash: ContentHash::sha256(b"config"),
            environment: env,
            started_at: Timestamp::now(),
            completed_at: None,
            status: ProviderRunStatus::Running,
        };
        assert!(run.completed_at.is_none());
        run.complete(ProviderRunStatus::Completed).unwrap();
        assert_eq!(run.status, ProviderRunStatus::Completed);
        assert!(run.completed_at.is_some());
    }

    #[test]
    fn provider_run_complete_rejects_invalid_transition() {
        use crate::ids::ProviderRunId;

        let env = EnvironmentIdentity {
            operating_system: NamespacedId::parse("core.linux").unwrap(),
            architecture: NamespacedId::parse("core.x86-64").unwrap(),
            isolation_backend: None,
            image_digest: None,
            extension: None,
        };
        let mut run = ProviderRun {
            id: ProviderRunId::new(),
            project: ProjectId::new(),
            provider: ProviderId::new(),
            operation: NamespacedId::parse("core.disassemble").unwrap(),
            input_artifacts: vec![],
            configuration_artifact: None,
            configuration_hash: ContentHash::sha256(b"config"),
            environment: env,
            started_at: Timestamp::now(),
            completed_at: None,
            status: ProviderRunStatus::Completed,
        };
        let result = run.complete(ProviderRunStatus::Failed);
        assert!(result.is_err());
    }

    #[test]
    fn provider_entity_alias_round_trip_json() {
        use crate::ids::ProviderRunId;

        let alias = ProviderEntityAlias {
            provider_run: ProviderRunId::new(),
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "sub_401000".to_string(),
            entity: EntityId::new(),
        };
        let json = serde_json::to_string_pretty(&alias).unwrap();
        let back: ProviderEntityAlias = serde_json::from_str(&json).unwrap();
        assert_eq!(alias, back);
    }

    #[test]
    fn native_artifact_round_trip_json() {
        use crate::ids::{NativeArtifactId, ProviderRunId};

        let na = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: ProviderRunId::new(),
            artifact: ArtifactId::new(),
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![EntityId::new(), EntityId::new()],
            description: Some("decompiled main".to_string()),
        };
        let json = serde_json::to_string_pretty(&na).unwrap();
        let back: NativeArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(na, back);
    }

    #[test]
    fn native_artifact_no_subjects_no_description() {
        use crate::ids::{NativeArtifactId, ProviderRunId};

        let na = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: ProviderRunId::new(),
            artifact: ArtifactId::new(),
            format: NATIVE_FORMAT_GHIDRA_PCODE.clone(),
            subject_entities: vec![],
            description: None,
        };
        let json = serde_json::to_string(&na).unwrap();
        let back: NativeArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.subject_entities, vec![]);
        assert_eq!(back.description, None);
    }

    #[test]
    fn native_format_constants_registered() {
        assert_eq!(NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.to_string(), "ida.hexrays.pseudocode");
        assert_eq!(NATIVE_FORMAT_IDA_MICROCODE.to_string(), "ida.microcode");
        assert_eq!(NATIVE_FORMAT_GHIDRA_PCODE.to_string(), "ghidra.pcode");
        assert_eq!(NATIVE_FORMAT_GDB_TRACE.to_string(), "gdb.trace");
        assert_eq!(NATIVE_FORMAT_Z3_MODEL.to_string(), "z3.model");
        assert_eq!(NATIVE_FORMAT_LLM_RAW_RESPONSE.to_string(), "llm.raw-response");
    }

    #[test]
    fn evidence_lifecycle_state_display() {
        assert_eq!(EvidenceLifecycleState::Active.to_string(), "Active");
        assert_eq!(EvidenceLifecycleState::Superseded.to_string(), "Superseded");
        assert_eq!(EvidenceLifecycleState::Invalidated.to_string(), "Invalidated");
        assert_eq!(EvidenceLifecycleState::Unavailable.to_string(), "Unavailable");
    }

    #[test]
    fn evidence_lifecycle_state_serialize_round_trip() {
        for state in [
            EvidenceLifecycleState::Active,
            EvidenceLifecycleState::Superseded,
            EvidenceLifecycleState::Invalidated,
            EvidenceLifecycleState::Unavailable,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: EvidenceLifecycleState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }

    #[test]
    fn assumption_round_trip_json() {
        let a = Assumption {
            description: "binary is stripped".to_string(),
            evidence: Some(EvidenceRecordId::new()),
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: Assumption = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);

        let a_none = Assumption {
            description: "manual assertion".to_string(),
            evidence: None,
        };
        let json = serde_json::to_string(&a_none).unwrap();
        let back: Assumption = serde_json::from_str(&json).unwrap();
        assert_eq!(a_none, back);
    }

    #[test]
    fn evidence_record_round_trip_json() {
        use crate::domain::Derivation;
        use crate::domain::values::DerivationMethod;

        let rec = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: ProjectId::new(),
            subject: EntityId::new(),
            predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
            value: EvidenceValue::String("main".to_string()),
            derivation: Derivation::new(
                DerivationMethod::DirectObservation,
                NamespacedId::parse("core.observe").unwrap(),
                vec![],
                vec![],
            ),
            provider_run: None,
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        };
        let json = serde_json::to_string_pretty(&rec).unwrap();
        let back: EvidenceRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(rec.id, back.id);
        assert_eq!(rec.project, back.project);
        assert_eq!(rec.subject, back.subject);
        assert_eq!(rec.predicate, back.predicate);
        assert_eq!(rec.value, back.value);
        assert_eq!(rec.derivation, back.derivation);
    }

    #[test]
    fn evidence_lifecycle_event_round_trip_json() {
        let ev = EvidenceLifecycleEvent {
            evidence: EvidenceRecordId::new(),
            timestamp: Timestamp::now(),
            state: EvidenceLifecycleState::Superseded,
            reason: Some("replaced by newer analysis".to_string()),
            caused_by: Some(EvidenceRecordId::new()),
        };
        let json = serde_json::to_string_pretty(&ev).unwrap();
        let back: EvidenceLifecycleEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev.evidence, back.evidence);
        assert_eq!(ev.state, back.state);
        assert_eq!(ev.reason, back.reason);
        assert_eq!(ev.caused_by, back.caused_by);
    }

    #[test]
    fn evidence_predicate_constants_registered() {
        assert_eq!(EVIDENCE_PREDICATE_FUNCTION_NAME.to_string(), "evidence.predicate.function-name");
        assert_eq!(EVIDENCE_PREDICATE_FUNCTION_SIGNATURE.to_string(), "evidence.predicate.function-signature");
        assert_eq!(EVIDENCE_PREDICATE_CALL_TARGET.to_string(), "evidence.predicate.call-target");
        assert_eq!(EVIDENCE_PREDICATE_STRING_REFERENCE.to_string(), "evidence.predicate.string-reference");
        assert_eq!(EVIDENCE_PREDICATE_TYPE_INFO.to_string(), "evidence.predicate.type-info");
        assert_eq!(EVIDENCE_PREDICATE_CONTROL_FLOW.to_string(), "evidence.predicate.control-flow");
    }

    // -- HypothesisStatus tests --

    #[test]
    fn hypothesis_state_transitions_valid() {
        let proposed = HypothesisStatus::Proposed;
        assert!(proposed.transition(&HypothesisStatus::UnderInvestigation).is_ok());

        let investigating = HypothesisStatus::UnderInvestigation;
        assert!(investigating.transition(&HypothesisStatus::Accepted).is_ok());
        assert!(investigating.transition(&HypothesisStatus::Rejected).is_ok());

        let accepted = HypothesisStatus::Accepted;
        let superseded = HypothesisStatus::Superseded { by: HypothesisId::new() };
        assert!(accepted.transition(&superseded).is_ok());
    }

    #[test]
    fn hypothesis_state_transitions_reject_invalid() {
        assert!(HypothesisStatus::Proposed.transition(&HypothesisStatus::Accepted).is_err());
        assert!(HypothesisStatus::Proposed.transition(&HypothesisStatus::Rejected).is_err());
        assert!(HypothesisStatus::UnderInvestigation.transition(&HypothesisStatus::Proposed).is_err());
        assert!(HypothesisStatus::Accepted.transition(&HypothesisStatus::Rejected).is_err());
        assert!(HypothesisStatus::Accepted.transition(&HypothesisStatus::UnderInvestigation).is_err());
        assert!(HypothesisStatus::Rejected.transition(&HypothesisStatus::Accepted).is_err());
        assert!(HypothesisStatus::Rejected.transition(&HypothesisStatus::Proposed).is_err());
        let s = HypothesisStatus::Superseded { by: HypothesisId::new() };
        assert!(s.transition(&HypothesisStatus::Accepted).is_err());
        assert!(s.transition(&HypothesisStatus::Proposed).is_err());
    }

    #[test]
    fn hypothesis_supersession_cycle_rejected() {
        use autore_core::validation::validate_no_cycle;

        let h1 = HypothesisId::new();
        let h2 = HypothesisId::new();
        let h3 = HypothesisId::new();

        let ids = vec![h1.to_string(), h2.to_string(), h3.to_string()];
        let cyclic_edges = vec![(0, 1), (1, 2), (2, 0)];
        let result = validate_no_cycle(&ids, &cyclic_edges);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle detected"));

        let acyclic_edges = vec![(0, 1), (1, 2)];
        assert!(validate_no_cycle(&ids, &acyclic_edges).is_ok());
    }

    #[test]
    fn confidence_independent_of_status() {
        let mut h = Hypothesis::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("hypothesis.predicate.test").unwrap(),
            EvidenceValue::String("candidate".to_string()),
        );
        assert_eq!(h.status, HypothesisStatus::Proposed);
        let original_status = h.status.clone();

        h.confidence = Confidence::new(0.9).unwrap();
        assert_eq!(h.status, original_status, "changing confidence must not change status");

        h.confidence = Confidence::new(0.1).unwrap();
        assert_eq!(h.status, original_status, "changing confidence must not change status");
    }

    #[test]
    fn hypothesis_status_display() {
        assert_eq!(HypothesisStatus::Proposed.to_string(), "Proposed");
        assert_eq!(HypothesisStatus::UnderInvestigation.to_string(), "UnderInvestigation");
        assert_eq!(HypothesisStatus::Accepted.to_string(), "Accepted");
        assert_eq!(HypothesisStatus::Rejected.to_string(), "Rejected");
        let s = HypothesisStatus::Superseded { by: HypothesisId::new() };
        assert!(s.to_string().starts_with("Superseded("));
    }

    #[test]
    fn hypothesis_status_kind() {
        assert_eq!(HypothesisStatus::Proposed.kind(), "Proposed");
        assert_eq!(HypothesisStatus::UnderInvestigation.kind(), "UnderInvestigation");
        assert_eq!(HypothesisStatus::Accepted.kind(), "Accepted");
        assert_eq!(HypothesisStatus::Rejected.kind(), "Rejected");
        let s = HypothesisStatus::Superseded { by: HypothesisId::new() };
        assert_eq!(s.kind(), "Superseded");
    }

    #[test]
    fn hypothesis_status_terminal() {
        assert!(!HypothesisStatus::Proposed.is_terminal());
        assert!(!HypothesisStatus::UnderInvestigation.is_terminal());
        assert!(HypothesisStatus::Accepted.is_terminal());
        assert!(HypothesisStatus::Rejected.is_terminal());
        assert!(HypothesisStatus::Superseded { by: HypothesisId::new() }.is_terminal());
    }

    #[test]
    fn hypothesis_status_serialize_round_trip() {
        for status in [
            HypothesisStatus::Proposed,
            HypothesisStatus::UnderInvestigation,
            HypothesisStatus::Accepted,
            HypothesisStatus::Rejected,
            HypothesisStatus::Superseded { by: HypothesisId::new() },
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: HypothesisStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn hypothesis_status_constants_registered() {
        assert_eq!(HYPOTHESIS_STATUS_PROPOSED.to_string(), "hypothesis.status.proposed");
        assert_eq!(HYPOTHESIS_STATUS_UNDER_INVESTIGATION.to_string(), "hypothesis.status.under-investigation");
        assert_eq!(HYPOTHESIS_STATUS_ACCEPTED.to_string(), "hypothesis.status.accepted");
        assert_eq!(HYPOTHESIS_STATUS_REJECTED.to_string(), "hypothesis.status.rejected");
        assert_eq!(HYPOTHESIS_STATUS_SUPERSEDED.to_string(), "hypothesis.status.superseded");
    }

    #[test]
    fn hypothesis_round_trip_json() {
        let h = Hypothesis {
            id: HypothesisId::new(),
            project: ProjectId::new(),
            subject: EntityId::new(),
            predicate: NamespacedId::parse("hypothesis.predicate.test").unwrap(),
            candidate: EvidenceValue::String("candidate".to_string()),
            supporting_evidence: vec![EvidenceRecordId::new()],
            contradicting_evidence: vec![],
            derived_from: vec![HypothesisId::new()],
            confidence: Confidence::with_rationale(0.7, "strong signal").unwrap(),
            status: HypothesisStatus::UnderInvestigation,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        let json = serde_json::to_string_pretty(&h).unwrap();
        let back: Hypothesis = serde_json::from_str(&json).unwrap();
        assert_eq!(h.id, back.id);
        assert_eq!(h.project, back.project);
        assert_eq!(h.subject, back.subject);
        assert_eq!(h.predicate, back.predicate);
        assert_eq!(h.candidate, back.candidate);
        assert_eq!(h.supporting_evidence, back.supporting_evidence);
        assert_eq!(h.contradicting_evidence, back.contradicting_evidence);
        assert_eq!(h.derived_from, back.derived_from);
        assert_eq!(h.confidence, back.confidence);
        assert_eq!(h.status, back.status);
    }

    #[test]
    fn hypothesis_new_defaults() {
        let h = Hypothesis::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("hypothesis.predicate.test").unwrap(),
            EvidenceValue::Null,
        );
        assert_eq!(h.status, HypothesisStatus::Proposed);
        assert!(h.supporting_evidence.is_empty());
        assert!(h.contradicting_evidence.is_empty());
        assert!(h.derived_from.is_empty());
        assert!((h.confidence.score() - 0.5).abs() < f32::EPSILON);
        assert!(h.confidence.rationale().is_none());
    }

    #[test]
    fn hypothesis_transition_updates_status() {
        let mut h = Hypothesis::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("hypothesis.predicate.test").unwrap(),
            EvidenceValue::Null,
        );
        assert_eq!(h.status, HypothesisStatus::Proposed);

        h.transition(HypothesisStatus::UnderInvestigation).unwrap();
        assert_eq!(h.status, HypothesisStatus::UnderInvestigation);

        h.transition(HypothesisStatus::Accepted).unwrap();
        assert_eq!(h.status, HypothesisStatus::Accepted);
    }

    #[test]
    fn hypothesis_transition_rejects_invalid() {
        let mut h = Hypothesis::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("hypothesis.predicate.test").unwrap(),
            EvidenceValue::Null,
        );
        let result = h.transition(HypothesisStatus::Accepted);
        assert!(result.is_err());
        assert_eq!(h.status, HypothesisStatus::Proposed, "status unchanged on invalid transition");
    }

    #[test]
    fn contradiction_status_transitions_valid() {
        let open = ContradictionStatus::Open;
        assert!(open.transition(&ContradictionStatus::Investigating).is_ok());
        assert!(open.transition(&ContradictionStatus::Resolved).is_ok());
        assert!(open.transition(&ContradictionStatus::Deferred).is_ok());

        let investigating = ContradictionStatus::Investigating;
        assert!(investigating.transition(&ContradictionStatus::Resolved).is_ok());
        assert!(investigating.transition(&ContradictionStatus::Deferred).is_ok());

        let deferred = ContradictionStatus::Deferred;
        assert!(deferred.transition(&ContradictionStatus::Open).is_ok());
    }

    #[test]
    fn contradiction_status_transitions_reject_invalid() {
        assert!(ContradictionStatus::Resolved.transition(&ContradictionStatus::Open).is_err());
        assert!(ContradictionStatus::Resolved.transition(&ContradictionStatus::Investigating).is_err());
        assert!(ContradictionStatus::Resolved.transition(&ContradictionStatus::Deferred).is_err());
        assert!(ContradictionStatus::Deferred.transition(&ContradictionStatus::Resolved).is_err());
        assert!(ContradictionStatus::Deferred.transition(&ContradictionStatus::Investigating).is_err());
        assert!(ContradictionStatus::Investigating.transition(&ContradictionStatus::Open).is_err());
        assert!(ContradictionStatus::Investigating.transition(&ContradictionStatus::Investigating).is_err());
        assert!(ContradictionStatus::Open.transition(&ContradictionStatus::Open).is_err());
    }

    #[test]
    fn contradiction_status_terminal() {
        assert!(!ContradictionStatus::Open.is_terminal());
        assert!(!ContradictionStatus::Investigating.is_terminal());
        assert!(ContradictionStatus::Resolved.is_terminal());
        assert!(!ContradictionStatus::Deferred.is_terminal());
    }

    #[test]
    fn contradiction_status_kind() {
        assert_eq!(ContradictionStatus::Open.kind(), "Open");
        assert_eq!(ContradictionStatus::Investigating.kind(), "Investigating");
        assert_eq!(ContradictionStatus::Resolved.kind(), "Resolved");
        assert_eq!(ContradictionStatus::Deferred.kind(), "Deferred");
    }

    #[test]
    fn contradiction_status_display() {
        assert_eq!(ContradictionStatus::Open.to_string(), "Open");
        assert_eq!(ContradictionStatus::Investigating.to_string(), "Investigating");
        assert_eq!(ContradictionStatus::Resolved.to_string(), "Resolved");
        assert_eq!(ContradictionStatus::Deferred.to_string(), "Deferred");
    }

    #[test]
    fn contradiction_status_serialize_round_trip() {
        for status in [
            ContradictionStatus::Open,
            ContradictionStatus::Investigating,
            ContradictionStatus::Resolved,
            ContradictionStatus::Deferred,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: ContradictionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, back);
        }
    }

    #[test]
    fn contradiction_status_constants_registered() {
        assert_eq!(CONTRADICTION_STATUS_OPEN.to_string(), "contradiction.status.open");
        assert_eq!(CONTRADICTION_STATUS_INVESTIGATING.to_string(), "contradiction.status.investigating");
        assert_eq!(CONTRADICTION_STATUS_RESOLVED.to_string(), "contradiction.status.resolved");
        assert_eq!(CONTRADICTION_STATUS_DEFERRED.to_string(), "contradiction.status.deferred");
    }

    #[test]
    fn contradiction_new_defaults() {
        let c = Contradiction::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("core.test").unwrap(),
            vec![EvidenceRecordId::new()],
            vec![HypothesisId::new(), HypothesisId::new()],
        );
        assert_eq!(c.status, ContradictionStatus::Open);
        assert!(c.resolution.is_none());
        assert_eq!(c.evidence.len(), 1);
        assert_eq!(c.hypotheses.len(), 2);
    }

    #[test]
    fn contradiction_transition_to_resolved_requires_resolution() {
        let mut c = Contradiction::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("core.test").unwrap(),
            vec![],
            vec![],
        );
        let result = c.transition(ContradictionStatus::Resolved, None);
        assert!(result.is_err());
        assert_eq!(c.status, ContradictionStatus::Open, "status unchanged on failed transition");
    }

    #[test]
    fn contradiction_transition_to_non_resolved_rejects_resolution() {
        let mut c = Contradiction::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("core.test").unwrap(),
            vec![],
            vec![],
        );
        let resolution = ContradictionResolution {
            resolved_at: Timestamp::now(),
            resolution: NamespacedId::parse("core.resolution.test").unwrap(),
            chosen: vec![],
            rationale: "rationale".into(),
        };
        let result = c.transition(ContradictionStatus::Investigating, Some(resolution));
        assert!(result.is_err());
    }

    #[test]
    fn contradiction_transition_open_to_resolved() {
        let mut c = Contradiction::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("core.test").unwrap(),
            vec![],
            vec![HypothesisId::new()],
        );
        let chosen = c.hypotheses.clone();
        let resolution = ContradictionResolution {
            resolved_at: Timestamp::now(),
            resolution: NamespacedId::parse("core.resolution.chosen-preferred").unwrap(),
            chosen,
            rationale: "preferred hypothesis supported by stronger evidence".into(),
        };
        c.transition(ContradictionStatus::Resolved, Some(resolution)).unwrap();
        assert_eq!(c.status, ContradictionStatus::Resolved);
        assert!(c.resolution.is_some());
    }

    #[test]
    fn contradiction_round_trip_json() {
        let mut c = Contradiction::new(
            ProjectId::new(),
            EntityId::new(),
            NamespacedId::parse("core.test").unwrap(),
            vec![EvidenceRecordId::new()],
            vec![HypothesisId::new(), HypothesisId::new()],
        );
        let chosen = vec![c.hypotheses[0]];
        let resolution = ContradictionResolution {
            resolved_at: Timestamp::now(),
            resolution: NamespacedId::parse("core.resolution.test").unwrap(),
            chosen,
            rationale: "chosen".into(),
        };
        c.transition(ContradictionStatus::Resolved, Some(resolution)).unwrap();
        let json = serde_json::to_string_pretty(&c).unwrap();
        let back: Contradiction = serde_json::from_str(&json).unwrap();
        assert_eq!(c.id, back.id);
        assert_eq!(c.project, back.project);
        assert_eq!(c.subject, back.subject);
        assert_eq!(c.predicate, back.predicate);
        assert_eq!(c.evidence, back.evidence);
        assert_eq!(c.hypotheses, back.hypotheses);
        assert_eq!(c.status, back.status);
        assert_eq!(c.resolution, back.resolution);
    }

    #[test]
    fn verification_subject_kinds() {
        assert_eq!(VerificationSubject::Entity(EntityId::new()).kind(), "Entity");
        assert_eq!(VerificationSubject::Hypothesis(HypothesisId::new()).kind(), "Hypothesis");
        assert_eq!(VerificationSubject::Artifact(ArtifactId::new()).kind(), "Artifact");
        assert_eq!(
            VerificationSubject::GenerationTarget(GenerationTargetId::new()).kind(),
            "GenerationTarget"
        );
    }

    #[test]
    fn verification_subject_round_trip_json() {
        let subjects = vec![
            VerificationSubject::Entity(EntityId::new()),
            VerificationSubject::Hypothesis(HypothesisId::new()),
            VerificationSubject::Artifact(ArtifactId::new()),
            VerificationSubject::GenerationTarget(GenerationTargetId::new()),
        ];
        for s in subjects {
            let json = serde_json::to_string(&s).unwrap();
            let back: VerificationSubject = serde_json::from_str(&json).unwrap();
            assert_eq!(s, back);
        }
    }

    #[test]
    fn verification_state_transitions_valid() {
        let nc = VerificationState::NotChecked;
        assert!(nc.transition(&VerificationState::Pending).is_ok());

        let pending = VerificationState::Pending;
        assert!(pending.transition(&VerificationState::Passed).is_ok());
        assert!(pending.transition(&VerificationState::Failed).is_ok());
        assert!(pending.transition(&VerificationState::Inconclusive).is_ok());
        assert!(pending.transition(&VerificationState::Blocked).is_ok());

        let blocked = VerificationState::Blocked;
        assert!(blocked.transition(&VerificationState::Pending).is_ok());
    }

    #[test]
    fn verification_state_transitions_reject_invalid() {
        assert!(VerificationState::Passed.transition(&VerificationState::Failed).is_err());
        assert!(VerificationState::Failed.transition(&VerificationState::Passed).is_err());
        assert!(VerificationState::Inconclusive.transition(&VerificationState::Pending).is_err());
        assert!(VerificationState::NotChecked.transition(&VerificationState::Passed).is_err());
        assert!(VerificationState::NotChecked.transition(&VerificationState::Failed).is_err());
        assert!(VerificationState::Blocked.transition(&VerificationState::Passed).is_err());
        assert!(VerificationState::Pending.transition(&VerificationState::NotChecked).is_err());
    }

    #[test]
    fn verification_state_terminal() {
        assert!(!VerificationState::NotChecked.is_terminal());
        assert!(!VerificationState::Pending.is_terminal());
        assert!(VerificationState::Passed.is_terminal());
        assert!(VerificationState::Failed.is_terminal());
        assert!(VerificationState::Inconclusive.is_terminal());
        assert!(!VerificationState::Blocked.is_terminal());
    }

    #[test]
    fn verification_state_kind() {
        assert_eq!(VerificationState::NotChecked.kind(), "NotChecked");
        assert_eq!(VerificationState::Pending.kind(), "Pending");
        assert_eq!(VerificationState::Passed.kind(), "Passed");
        assert_eq!(VerificationState::Failed.kind(), "Failed");
        assert_eq!(VerificationState::Inconclusive.kind(), "Inconclusive");
        assert_eq!(VerificationState::Blocked.kind(), "Blocked");
    }

    #[test]
    fn verification_state_serialize_round_trip() {
        for state in [
            VerificationState::NotChecked,
            VerificationState::Pending,
            VerificationState::Passed,
            VerificationState::Failed,
            VerificationState::Inconclusive,
            VerificationState::Blocked,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: VerificationState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }

    #[test]
    fn verification_check_constants_registered() {
        assert_eq!(VERIFICATION_CHECK_ARTIFACT_HASH.to_string(), "core.artifact.hash");
        assert_eq!(VERIFICATION_CHECK_PROJECT_INTEGRITY.to_string(), "core.project.integrity");
        assert_eq!(VERIFICATION_CHECK_BUILD.to_string(), "verification.build");
        assert_eq!(VERIFICATION_CHECK_ABI_LAYOUT.to_string(), "verification.abi.layout");
        assert_eq!(
            VERIFICATION_CHECK_DIFFERENTIAL_BEHAVIOR.to_string(),
            "verification.differential.behavior"
        );
    }

    #[test]
    fn verification_record_new_defaults() {
        let rec = VerificationRecord::new(
            ProjectId::new(),
            VerificationSubject::Entity(EntityId::new()),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        assert_eq!(rec.state, VerificationState::NotChecked);
        assert!(rec.evidence.is_empty());
        assert!(rec.provider_run.is_none());
        assert!(rec.details.is_none());
    }

    #[test]
    fn verification_record_round_trip_json() {
        let schema = NamespacedId::parse("core.verification.details").unwrap();
        let rec = VerificationRecord {
            id: VerificationRecordId::new(),
            project: ProjectId::new(),
            subject: VerificationSubject::Hypothesis(HypothesisId::new()),
            check: VERIFICATION_CHECK_BUILD.clone(),
            state: VerificationState::Passed,
            provider_run: Some(ProviderRunId::new()),
            evidence: vec![EvidenceRecordId::new(), EvidenceRecordId::new()],
            details: Some(ExtensionData::new(schema, 1, serde_json::json!({"notes": "ok"}))),
            created_at: Timestamp::now(),
        };
        let json = serde_json::to_string_pretty(&rec).unwrap();
        let back: VerificationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(rec.id, back.id);
        assert_eq!(rec.project, back.project);
        assert_eq!(rec.subject, back.subject);
        assert_eq!(rec.check, back.check);
        assert_eq!(rec.state, back.state);
        assert_eq!(rec.provider_run, back.provider_run);
        assert_eq!(rec.evidence, back.evidence);
        assert_eq!(rec.details, back.details);
    }

    #[test]
    fn verification_does_not_change_confidence() {
        let project = ProjectId::new();
        let subject = EntityId::new();
        let mut h = Hypothesis::new(
            project,
            subject,
            NamespacedId::parse("hypothesis.predicate.test").unwrap(),
            EvidenceValue::Null,
        );
        let original_confidence = h.confidence.score();

        let _vr = VerificationRecord::new(
            project,
            VerificationSubject::Hypothesis(h.id),
            VERIFICATION_CHECK_BUILD.clone(),
        );

        assert!(
            (h.confidence.score() - original_confidence).abs() < f32::EPSILON,
            "recording a VerificationRecord must NOT change hypothesis confidence (§3.5)"
        );

        h.confidence = Confidence::new(0.9).unwrap();
        assert!(
            (h.confidence.score() - 0.9).abs() < f32::EPSILON,
            "confidence updates must remain independent of verification records"
        );
    }

    // -- Operation tests --

    use super::{
        CancellationRequest, EventSource, EventSubject, MetricMap, Operation, OperationFailure,
        ProgressUpdate, OPERATION_KIND_ARTIFACT_IMPORT, OPERATION_KIND_EXTERNAL_ARTIFACT_CHECK,
        OPERATION_KIND_PROJECT_MIGRATION, OPERATION_KIND_PROJECT_REBUILD_INDEXES,
        OPERATION_KIND_PROJECT_VALIDATION,
    };
    use autore_core::operation::OperationState;

    #[test]
    fn operation_new_defaults() {
        let op = Operation::new(
            ProjectId::new(),
            OPERATION_KIND_ARTIFACT_IMPORT.clone(),
            "cli",
        );
        assert_eq!(op.state, OperationState::Queued);
        assert!(op.subject.is_none());
        assert!(op.parent.is_none());
        assert!(op.failure.is_none());
        assert_eq!(op.requested_by, "cli");
    }

    #[test]
    fn operation_transition_queued_to_running() {
        let mut op = Operation::new(
            ProjectId::new(),
            OPERATION_KIND_ARTIFACT_IMPORT.clone(),
            "cli",
        );
        assert!(op.transition(OperationState::Running).is_ok());
        assert_eq!(op.state, OperationState::Running);
    }

    #[test]
    fn operation_transition_full_lifecycle() {
        let mut op = Operation::new(
            ProjectId::new(),
            OPERATION_KIND_PROJECT_VALIDATION.clone(),
            "system",
        );
        assert!(op.transition(OperationState::Running).is_ok());
        assert!(op.transition(OperationState::Paused).is_ok());
        assert!(op.transition(OperationState::Running).is_ok());
        assert!(op.transition(OperationState::Completed).is_ok());
        assert!(op.state.is_terminal());
    }

    #[test]
    fn operation_transition_cancellation_flow() {
        let mut op = Operation::new(
            ProjectId::new(),
            OPERATION_KIND_PROJECT_MIGRATION.clone(),
            "tui",
        );
        assert!(op.transition(OperationState::Running).is_ok());
        assert!(op.transition(OperationState::Cancelling).is_ok());
        assert!(op.transition(OperationState::Cancelled).is_ok());
        assert!(op.state.is_terminal());
    }

    #[test]
    fn operation_round_trip_json() {
        let mut op = Operation::new(
            ProjectId::new(),
            OPERATION_KIND_PROJECT_REBUILD_INDEXES.clone(),
            "cli",
        );
        op.subject = Some(EventSubject::Project(op.project));
        op.parent = Some(OperationId::new());
        let json = serde_json::to_string_pretty(&op).unwrap();
        let back: Operation = serde_json::from_str(&json).unwrap();
        assert_eq!(op.id, back.id);
        assert_eq!(op.project, back.project);
        assert_eq!(op.kind, back.kind);
        assert_eq!(op.state, back.state);
        assert_eq!(op.subject, back.subject);
        assert_eq!(op.parent, back.parent);
    }

    #[test]
    fn operation_failure_round_trip() {
        let failure = OperationFailure {
            code: NamespacedId::parse("core.error.timeout").unwrap(),
            message: "operation timed out after 60s".into(),
            details: None,
        };
        let json = serde_json::to_string(&failure).unwrap();
        let back: OperationFailure = serde_json::from_str(&json).unwrap();
        assert_eq!(failure.code, back.code);
        assert_eq!(failure.message, back.message);
    }

    #[test]
    fn progress_update_round_trip() {
        let mut metrics: MetricMap = BTreeMap::new();
        metrics.insert(
            NamespacedId::parse("progress.percent").unwrap(),
            42.5,
        );
        let pu = ProgressUpdate::new(OperationId::new(), 0, "analyzing", metrics.clone());
        let json = serde_json::to_string(&pu).unwrap();
        let back: ProgressUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(pu.id, back.id);
        assert_eq!(pu.operation_id, back.operation_id);
        assert_eq!(pu.sequence, 0);
        assert_eq!(back.metrics.len(), 1);
    }

    #[test]
    fn cancellation_request_round_trip() {
        let cr = CancellationRequest::new(
            OperationId::new(),
            "user",
            Some("no longer needed".into()),
        );
        let json = serde_json::to_string(&cr).unwrap();
        let back: CancellationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(cr.id, back.id);
        assert_eq!(cr.operation_id, back.operation_id);
        assert_eq!(cr.requested_by, "user");
        assert_eq!(cr.reason, Some("no longer needed".into()));
    }

    #[test]
    fn event_source_display() {
        assert_eq!(EventSource::Operation.to_string(), "Operation");
        assert_eq!(EventSource::Project.to_string(), "Project");
    }

    #[test]
    fn event_subject_round_trip() {
        let subject = EventSubject::Operation(OperationId::new());
        let json = serde_json::to_string(&subject).unwrap();
        let back: EventSubject = serde_json::from_str(&json).unwrap();
        assert_eq!(subject, back);
    }

    #[test]
    fn operation_kind_constants() {
        assert_eq!(OPERATION_KIND_ARTIFACT_IMPORT.to_string(), "core.artifact.import");
        assert_eq!(OPERATION_KIND_PROJECT_VALIDATION.to_string(), "core.project.validation");
        assert_eq!(OPERATION_KIND_PROJECT_MIGRATION.to_string(), "core.project.migration");
        assert_eq!(
            OPERATION_KIND_PROJECT_REBUILD_INDEXES.to_string(),
            "core.project.rebuild-indexes"
        );
        assert_eq!(
            OPERATION_KIND_EXTERNAL_ARTIFACT_CHECK.to_string(),
            "core.project.external-artifact-check"
        );
    }

    // -- ProjectEvent tests --

    use super::{
        ProjectEvent, EVENT_KIND_ARTIFACT_EXTERNAL_CHANGED, EVENT_KIND_ARTIFACT_REGISTERED,
        EVENT_KIND_CONTRADICTION_CREATED, EVENT_KIND_ENTITY_CREATED,
        EVENT_KIND_EVIDENCE_ADDED, EVENT_KIND_EVIDENCE_INVALIDATED,
        EVENT_KIND_HYPOTHESIS_ACCEPTED, EVENT_KIND_HYPOTHESIS_PROPOSED,
        EVENT_KIND_HYPOTHESIS_REJECTED, EVENT_KIND_OPERATION_COMPLETED,
        EVENT_KIND_OPERATION_FAILED, EVENT_KIND_OPERATION_PROGRESS,
        EVENT_KIND_OPERATION_QUEUED, EVENT_KIND_OPERATION_STARTED,
        EVENT_KIND_PROJECT_CREATED, EVENT_KIND_PROJECT_VALIDATION_FAILED,
        EVENT_KIND_VERIFICATION_RECORDED,
    };

    #[test]
    fn project_event_new_defaults() {
        let ev = ProjectEvent::new(
            ProjectId::new(),
            1,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
        );
        assert_eq!(ev.sequence, 1);
        assert_eq!(ev.kind, *EVENT_KIND_PROJECT_CREATED);
        assert_eq!(ev.source, EventSource::Project);
        assert!(ev.subject.is_none());
        assert!(ev.payload.is_none());
    }

    #[test]
    fn project_event_round_trip_json() {
        let pid = ProjectId::new();
        let schema = NamespacedId::parse("core.event.details").unwrap();
        let ev = ProjectEvent::new(
            pid,
            42,
            EVENT_KIND_ARTIFACT_REGISTERED.clone(),
            EventSource::Artifact,
            Some(EventSubject::Project(pid)),
            Some(ExtensionData::new(schema, 1, serde_json::json!({"key": "val"}))),
        );
        let json = serde_json::to_string_pretty(&ev).unwrap();
        let back: ProjectEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(ev.id, back.id);
        assert_eq!(ev.project, back.project);
        assert_eq!(ev.sequence, back.sequence);
        assert_eq!(ev.kind, back.kind);
        assert_eq!(ev.subject, back.subject);
        assert_eq!(ev.source, back.source);
        assert_eq!(ev.payload, back.payload);
    }

    #[test]
    fn project_event_kind_constants_registered() {
        assert_eq!(EVENT_KIND_PROJECT_CREATED.to_string(), "core.project.created");
        assert_eq!(EVENT_KIND_ARTIFACT_REGISTERED.to_string(), "core.artifact.registered");
        assert_eq!(EVENT_KIND_ARTIFACT_EXTERNAL_CHANGED.to_string(), "core.artifact.external-changed");
        assert_eq!(EVENT_KIND_ENTITY_CREATED.to_string(), "core.entity.created");
        assert_eq!(EVENT_KIND_EVIDENCE_ADDED.to_string(), "core.evidence.added");
        assert_eq!(EVENT_KIND_EVIDENCE_INVALIDATED.to_string(), "core.evidence.invalidated");
        assert_eq!(EVENT_KIND_HYPOTHESIS_PROPOSED.to_string(), "core.hypothesis.proposed");
        assert_eq!(EVENT_KIND_HYPOTHESIS_ACCEPTED.to_string(), "core.hypothesis.accepted");
        assert_eq!(EVENT_KIND_HYPOTHESIS_REJECTED.to_string(), "core.hypothesis.rejected");
        assert_eq!(EVENT_KIND_CONTRADICTION_CREATED.to_string(), "core.contradiction.created");
        assert_eq!(EVENT_KIND_VERIFICATION_RECORDED.to_string(), "core.verification.recorded");
        assert_eq!(EVENT_KIND_OPERATION_QUEUED.to_string(), "core.operation.queued");
        assert_eq!(EVENT_KIND_OPERATION_STARTED.to_string(), "core.operation.started");
        assert_eq!(EVENT_KIND_OPERATION_PROGRESS.to_string(), "core.operation.progress");
        assert_eq!(EVENT_KIND_OPERATION_COMPLETED.to_string(), "core.operation.completed");
        assert_eq!(EVENT_KIND_OPERATION_FAILED.to_string(), "core.operation.failed");
        assert_eq!(EVENT_KIND_PROJECT_VALIDATION_FAILED.to_string(), "core.project.validation-failed");
    }

    #[test]
    fn project_event_with_subject_and_source() {
        let pid = ProjectId::new();
        let op_id = OperationId::new();
        let ev = ProjectEvent::new(
            pid,
            5,
            EVENT_KIND_OPERATION_STARTED.clone(),
            EventSource::Operation,
            Some(EventSubject::Operation(op_id)),
            None,
        );
        assert_eq!(ev.source, EventSource::Operation);
        assert_eq!(ev.subject, Some(EventSubject::Operation(op_id)));
    }
}
