//! Whole-program reconstruction: canonical entity identity and observation
//! import for the auto-re Stage 1 pipeline.
//!
//! The [`identity`] module owns the canonical key that pins an entity to a
//! specific location in a binary revision, independent of any single
//! provider's row ids. The importer consumes provider observations and
//! routes every canonical mutation through [`ApplicationCommand`] so the
//! Stage 0 event store remains the single source of truth.

pub mod analysis;
pub mod build;
pub mod coordinator;
pub mod dynamic;
pub mod fingerprint;
pub mod generation;
pub mod identity;
pub mod types;
pub mod verification;
pub mod work_graph;

#[cfg(test)]
mod tests_support;

pub use analysis::{
    BUNDLE_MAX_BYTES, BuildDiagnosticSummary, BundleBuilder, BundleStore, CallSiteSummary,
    InvestigationBundle, LlmImportError, LlmImportResult, LlmImporter, StaticArtifactSet,
    StringSnippet,
};
pub use build::{
    BuildConfigured, BuildDiagnostic as BuildDiag, BuildLogs, BuildProviderError,
    BuildProviderTrait, BuildResult, CompileResult, CompileUnit, DiagnosticSeverity,
    DockerMsvc2002BuildProvider, DockerMsvc2002Config, GeneratorManifest, LinkResult,
    RunTestResult, SuggestedWorkKind,
};
pub use coordinator::{
    CompletionPolicy, Coordinator, CoordinatorConfig, CoordinatorState, CoordinatorWorkItem,
    DispatchKind, HandlerOutput, NoProgressDetector, NoProgressKind, ProviderHealth, TickResult,
    WorkKindHandlers,
};
pub use dynamic::{
    AddressRange, Scenario, ScenarioValidationError, ScenarioVerifier, SetupOp, Step, StopOp,
};
pub use fingerprint::{
    FingerprintComparison, FingerprintInput, FingerprintSnapshot, InMemorySnapshot,
    InvalidationPropagator, compare_fingerprint, compute_fingerprint,
};
pub use generation::{
    CandidatePatch, FileRole, GeneratedFile, GeneratedSourceMappingIntent, PatchError,
    PatchOutcome, PatchPipeline, ProjectSkeletonBuilder, SkeletonManifest, StubPolicy,
};
pub use identity::{
    CanonicalEntityKey, ImportSummary, ObservationImporter, entity_kind_for_observation_kind,
    entity_kind_from_observation, work_item_kind_for_entity,
};
pub use types::{
    LayoutConstraint, LayoutConstraintKind, LayoutConstraintStore, ReconciledBaseAdjustment,
    ReconciledField, ReconciledLayout, ReconciledParameterUsage, ReconciledReturnValueUse,
    ReconciledVtableSlot, Reconciler,
};
pub use verification::{
    CauseCategory, ComparisonCounts, ComparisonLevel, ComparisonPolicy, ComparisonResult,
    ExecutionDiagnostic, FailureAnalysisRequest, InitialState, NormalizationRule, Observation,
    ObservationBackend, ObservationError, ObservationSet, RegressionSet, RegressionTracker,
    RepairConfig, RepairGenerationRequest, RepairResult, Scenario as VerificationScenario,
    ScenarioExecutor, ScenarioInput, VerificationComparison, VerificationRepairDriver,
    Wave7ObservationBackend, bounded_diff_for_llm, compare, determine_cause,
    is_regression_edge_kind, is_regression_fingerprint_edge_kind,
};
pub use work_graph::{DependencyEdgeKind, WorkGraph, WorkGraphBuilder, WorkItemNode};
