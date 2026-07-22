//! `InvestigationBundle` — the bounded, handle-only context packet sent to
//! an LLM analysis capability.
//!
//! Per spec §8.3 the bundle carries artifact *handles* (`ArtifactId`), never
//! raw bytes. A `byte_size_estimate` keeps the bundle bounded (≤ 64 KiB).

use autore_schema::ids::{
    ArtifactId, ConflictRecordId, EntityId, HypothesisId, VerificationComparisonId, WorkItemId,
};
use serde::{Deserialize, Serialize};

use crate::work_graph::DependencyEdgeKind;

/// Maximum bundle byte-size estimate (64 KiB).
pub const BUNDLE_MAX_BYTES: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Supporting value types
// ---------------------------------------------------------------------------

/// A caller or callee summarized for the bundle — work-item handle + a
/// short textual brief + the dependency edge kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallSiteSummary {
    pub work_item_id: WorkItemId,
    pub brief: String,
    pub edge_kind: DependencyEdgeKind,
}

/// A string literal or notable constant observed in the subject's scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StringSnippet {
    pub value: String,
    pub context: String,
}

/// A summarized build diagnostic (no raw compiler output).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildDiagnosticSummary {
    pub severity: String,
    pub source_file: Option<String>,
    pub line: Option<u32>,
    pub code: Option<String>,
    pub message: String,
}

// ---------------------------------------------------------------------------
// InvestigationBundle
// ---------------------------------------------------------------------------

/// The bounded investigation bundle (§8.3).
///
/// Every artifact reference is an `ArtifactId` handle — the bundle never
/// carries raw bytes. The bundle is serialized to JSON and sent as the
/// `ExecutionRequest` payload for an LLM analysis capability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestigationBundle {
    /// WorkItemId of the subject work item.
    pub subject_identity: WorkItemId,
    /// EntityId of the semantic entity under analysis, if any.
    pub subject_entity_id: Option<EntityId>,

    // -- static structural observations (artifact handles) --
    /// ArtifactId for the IDA function/type snapshot.
    pub static_structural_snapshot: Option<ArtifactId>,
    /// ArtifactId for the decompilation pseudocode.
    pub decompilation_artifact: Option<ArtifactId>,
    /// ArtifactId for the disassembly listing.
    pub disassembly_artifact: Option<ArtifactId>,
    /// ArtifactId for the control-flow graph summary.
    pub cfg_summary: Option<ArtifactId>,

    // -- graph neighborhood --
    /// Callers and callees relevant to the subject.
    pub callers_and_callees: Vec<CallSiteSummary>,
    /// EntityIds of types referenced by the subject.
    pub relevant_types: Vec<EntityId>,
    /// EntityIds of globals referenced by the subject.
    pub relevant_globals: Vec<EntityId>,
    /// String literals and notable constants in the subject's scope.
    pub strings_and_constants: Vec<StringSnippet>,

    // -- dynamic observations (empty in Wave 5) --
    /// ArtifactIds for dynamic trace observations.
    pub dynamic_observations: Vec<ArtifactId>,

    // -- prior knowledge --
    /// HypothesisIds accepted for this subject.
    pub accepted_hypotheses: Vec<HypothesisId>,
    /// ConflictRecordIds still open for this subject.
    pub unresolved_conflicts: Vec<ConflictRecordId>,
    /// ArtifactId of a prior generated candidate, if any.
    pub prior_generated_candidate: Option<ArtifactId>,

    // -- build/verification feedback (empty in Wave 5) --
    /// Summarized build diagnostics.
    pub compiler_diagnostics: Vec<BuildDiagnosticSummary>,
    /// VerificationComparisonIds of failures.
    pub verification_failures: Vec<VerificationComparisonId>,

    // -- output contract --
    /// The JSON Schema for the target capability's parsed result.
    pub requested_output_schema: serde_json::Value,
}

impl InvestigationBundle {
    /// Returns an estimate of the serialized byte size of this bundle.
    ///
    /// Uses `serde_json::to_vec` length as the estimate. Callers should
    /// assert this is ≤ [`BUNDLE_MAX_BYTES`].
    pub fn byte_size_estimate(&self) -> usize {
        serde_json::to_vec(self).map(|v| v.len()).unwrap_or(0)
    }
}
