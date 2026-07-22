//! Typed verification observations, normalization rules, and comparison results.
//!
//! These types are intentionally separate from the Wave 7 dynamic scenario
//! language: they model the *semantic* outcome of a scenario (what was observed)
//! and the *policy* for comparing two outcomes, rather than the debugger steps
//! used to produce them.

use std::collections::HashMap;
use std::path::PathBuf;

use autore_schema::domain::{EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, EntityId, ProviderRunId, VerificationComparisonId};

use crate::dynamic::scenario::Step;

// ---------------------------------------------------------------------------
// Scenario description
// ---------------------------------------------------------------------------

/// Initial state for a verification scenario.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InitialState {
    /// Environment variables for the target process.
    pub env: HashMap<String, String>,
    /// Command-line arguments.
    pub argv: Vec<String>,
    /// Working directory for the target process.
    pub working_dir: PathBuf,
    /// Deterministic seed, if the scenario is seeded.
    pub seed: Option<u64>,
}

impl InitialState {
    /// Creates a new initial state.
    pub fn new(
        env: HashMap<String, String>,
        argv: Vec<String>,
        working_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            env,
            argv,
            working_dir: working_dir.into(),
            seed: None,
        }
    }

    /// Sets the deterministic seed.
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }
}

/// A single input fed to the scenario (stdin, network payload, etc.).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScenarioInput {
    /// Namespaced input kind, e.g. `verify.input.stdin`.
    pub kind: NamespacedId,
    /// Structured input payload.
    pub data: serde_json::Value,
}

impl ScenarioInput {
    /// Creates a new scenario input.
    pub fn new(kind: NamespacedId, data: serde_json::Value) -> Self {
        Self { kind, data }
    }
}

/// Level at which the comparison is performed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComparisonLevel {
    /// Compare a single function.
    Function,
    /// Compare a function cluster or subsystem.
    Cluster,
    /// Compare the whole program smoke test.
    WholeProgram,
}

/// Policy controlling how strict the comparison is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComparisonPolicy {
    /// Values must match exactly.
    Strict,
    /// Values may match after applying normalization rules.
    #[default]
    Normalized,
    /// Diagnostics are ignored during comparison.
    IgnoreDiagnostics,
}

/// A normalization rule applied before comparing observations.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum NormalizationRule {
    /// Addresses are relative to the image base of each binary.
    RelocatedAddress {
        /// Image base used for the original executable.
        original_base_address: u128,
        /// Image base used for the generated candidate.
        candidate_base_address: u128,
    },
    /// Timestamps are replaced by the given placeholder.
    Timestamp {
        /// Value used in place of the real timestamp.
        placeholder: u64,
    },
    /// Random seeds are replaced by the given placeholder.
    RandomSeed {
        /// Value used in place of the real seed.
        placeholder: u64,
    },
    /// Environment-specific handles (file descriptors, window handles) are replaced.
    EnvSpecificHandle {
        /// Value used in place of the real handle.
        placeholder: String,
    },
}

// ---------------------------------------------------------------------------
// Observations
// ---------------------------------------------------------------------------

/// A single typed observation extracted from a scenario execution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Observation {
    /// Namespaced observation kind, e.g. `debug.registers`.
    pub kind: NamespacedId,
    /// Optional stable key distinguishing multiple observations of the same kind.
    pub key: Option<String>,
    /// Entity associated with the observation, if any.
    pub entity_id: Option<EntityId>,
    /// Address associated with the observation, if any.
    pub address: Option<u128>,
    /// Wall-clock timestamp, if the observation carries one.
    pub timestamp: Option<u64>,
    /// Structured observation payload.
    pub data: serde_json::Value,
}

impl Observation {
    /// Creates an observation.
    pub fn new(kind: NamespacedId, data: serde_json::Value) -> Self {
        Self {
            kind,
            key: None,
            entity_id: None,
            address: None,
            timestamp: None,
            data,
        }
    }

    /// Sets the stable key.
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Sets the entity.
    pub fn with_entity(mut self, entity_id: EntityId) -> Self {
        self.entity_id = Some(entity_id);
        self
    }

    /// Sets the address.
    pub fn with_address(mut self, address: u128) -> Self {
        self.address = Some(address);
        self
    }

    /// Sets the timestamp.
    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }
}

/// Diagnostic emitted during scenario execution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExecutionDiagnostic {
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl ExecutionDiagnostic {
    /// Creates a diagnostic.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// The complete set of observations captured for one scenario execution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ObservationSet {
    /// Identifier of the scenario that produced this set.
    pub scenario_id: String,
    /// Artifact that was executed to produce this set.
    pub target_artifact_id: ArtifactId,
    /// Image base of the executed binary, if known.
    pub image_base: Option<u128>,
    /// Typed observations captured during execution.
    pub observations: Vec<Observation>,
    /// Captured stdout.
    pub stdout: Option<String>,
    /// Captured stderr.
    pub stderr: Option<String>,
    /// Process exit code.
    pub exit_code: Option<i32>,
    /// Non-fatal diagnostics.
    pub diagnostics: Vec<ExecutionDiagnostic>,
    /// Whether the execution itself failed (segfault, timeout, etc.).
    pub execution_failed: bool,
    /// Diagnostic describing the execution failure, if any.
    pub execution_failure_diagnostic: Option<ExecutionDiagnostic>,
}

impl ObservationSet {
    /// Creates an empty observation set.
    pub fn new(scenario_id: impl Into<String>, target_artifact_id: ArtifactId) -> Self {
        Self {
            scenario_id: scenario_id.into(),
            target_artifact_id,
            image_base: None,
            observations: Vec::new(),
            stdout: None,
            stderr: None,
            exit_code: None,
            diagnostics: Vec::new(),
            execution_failed: false,
            execution_failure_diagnostic: None,
        }
    }

    /// Sets the image base.
    pub fn with_image_base(mut self, base: u128) -> Self {
        self.image_base = Some(base);
        self
    }

    /// Adds an observation.
    pub fn add_observation(mut self, observation: Observation) -> Self {
        self.observations.push(observation);
        self
    }

    /// Records a fatal execution failure.
    pub fn with_execution_failure(mut self, diagnostic: ExecutionDiagnostic) -> Self {
        self.execution_failed = true;
        self.execution_failure_diagnostic = Some(diagnostic);
        self
    }

    /// Records the process exit code.
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Records captured stdout.
    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = Some(stdout.into());
        self
    }

    /// Records captured stderr.
    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = Some(stderr.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Comparison results
// ---------------------------------------------------------------------------

/// Result of comparing a single observation or a whole scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ComparisonResult {
    /// Values are identical without normalization.
    Equal,
    /// Values are identical after applying normalization rules.
    EquivalentUnderNormalization,
    /// Values differ even after normalization.
    Different,
    /// Observations are not comparable (e.g. mismatched kinds).
    Inconclusive,
    /// Expected observation was missing from one side.
    NotObserved,
    /// Execution failed before a meaningful comparison could be made.
    ExecutionFailed,
}

/// Counts of each comparison result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ComparisonCounts {
    pub equal_count: u64,
    pub equivalent_count: u64,
    pub different_count: u64,
    pub inconclusive_count: u64,
    pub not_observed_count: u64,
    pub execution_failed_count: u64,
}

impl ComparisonCounts {
    /// Creates zeroed counts.
    pub fn zero() -> Self {
        Self {
            equal_count: 0,
            equivalent_count: 0,
            different_count: 0,
            inconclusive_count: 0,
            not_observed_count: 0,
            execution_failed_count: 0,
        }
    }
}

/// The outcome of comparing an original observation set with a candidate set.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VerificationComparison {
    pub id: VerificationComparisonId,
    pub scenario_id: String,
    pub provider_run_id: ProviderRunId,
    pub original_output: EvidenceValue,
    pub candidate_output: EvidenceValue,
    pub per_observation_results: Vec<ComparisonResult>,
    pub counts: ComparisonCounts,
    pub overall: ComparisonResult,
    pub matches: bool,
    /// Whether the difference should trigger a repair cycle.
    pub requires_repair: bool,
    pub compared_at: Timestamp,
}

impl VerificationComparison {
    /// Creates a new comparison record.
    pub fn new(
        scenario_id: impl Into<String>,
        original_output: EvidenceValue,
        candidate_output: EvidenceValue,
        per_observation_results: Vec<ComparisonResult>,
        counts: ComparisonCounts,
        overall: ComparisonResult,
    ) -> Self {
        let matches = matches!(
            overall,
            ComparisonResult::Equal | ComparisonResult::EquivalentUnderNormalization
        );
        let requires_repair = matches!(
            overall,
            ComparisonResult::Different
                | ComparisonResult::Inconclusive
                | ComparisonResult::ExecutionFailed
        );
        Self {
            id: VerificationComparisonId::new(),
            scenario_id: scenario_id.into(),
            provider_run_id: ProviderRunId::new(),
            original_output,
            candidate_output,
            per_observation_results,
            counts,
            overall,
            matches,
            requires_repair,
            compared_at: Timestamp::now(),
        }
    }
}

// ---------------------------------------------------------------------------
// Scenario
// ---------------------------------------------------------------------------

/// A verification scenario describing what to run, how to compare, and what to
/// ignore.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scenario {
    /// Stable scenario identifier.
    pub id: String,
    /// Work item that owns this scenario.
    pub work_item_id: String,
    /// Canonical entity the scenario exercises.
    pub subject_entity: EntityId,
    /// Initial state for the target process.
    pub initial_state: InitialState,
    /// Inputs fed to the scenario.
    pub inputs: Vec<ScenarioInput>,
    /// Original binary artifact to execute.
    pub executable_artifact_id: ArtifactId,
    /// Generated candidate binary artifact to execute.
    pub candidate_artifact_id: ArtifactId,
    /// Typed debugger steps used to drive the scenario.
    pub execution_steps: Vec<Step>,
    /// Comparison policy.
    pub comparison_policy: ComparisonPolicy,
    /// Normalization rules applied before comparison.
    pub normalization_rules: Vec<NormalizationRule>,
    /// Level at which the comparison is performed.
    pub comparison_level: ComparisonLevel,
}

impl Scenario {
    /// Creates a new verification scenario.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        work_item_id: impl Into<String>,
        subject_entity: EntityId,
        initial_state: InitialState,
        executable_artifact_id: ArtifactId,
        candidate_artifact_id: ArtifactId,
        execution_steps: Vec<Step>,
        comparison_level: ComparisonLevel,
    ) -> Self {
        Self {
            id: id.into(),
            work_item_id: work_item_id.into(),
            subject_entity,
            initial_state,
            inputs: Vec::new(),
            executable_artifact_id,
            candidate_artifact_id,
            execution_steps,
            comparison_policy: ComparisonPolicy::default(),
            normalization_rules: Vec::new(),
            comparison_level,
        }
    }

    /// Adds a normalization rule.
    pub fn add_normalization_rule(mut self, rule: NormalizationRule) -> Self {
        self.normalization_rules.push(rule);
        self
    }

    /// Adds an input.
    pub fn add_input(mut self, input: ScenarioInput) -> Self {
        self.inputs.push(input);
        self
    }
}
