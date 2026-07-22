//! Scenario capture/replay/comparison model.
//!
//! This module implements Todo 45 of the Stage 1 plan: typed, normalized,
//! differential verification. It sits on top of the Wave 7 dynamic pipeline and
//! routes all durable side effects through [`ApplicationCommand`].

pub mod comparator;
pub mod executor;
pub mod regression;
pub mod repair;
pub mod scenario;
pub mod types;

pub use comparator::compare;
pub use executor::{
    ObservationBackend, ObservationError, ScenarioExecutor, Wave7ObservationBackend,
};
pub use regression::{
    DEFAULT_MAX_REGRESSION_SCENARIOS, RegressionSet, RegressionTracker, is_regression_edge_kind,
    is_regression_fingerprint_edge_kind,
};
pub use repair::{
    CauseCategory, FailureAnalysisRequest, RepairConfig, RepairGenerationRequest, RepairResult,
    VerificationRepairDriver, bounded_diff_for_llm, determine_cause,
};
pub use types::{
    ComparisonCounts, ComparisonLevel, ComparisonPolicy, ComparisonResult, ExecutionDiagnostic,
    InitialState, NormalizationRule, Observation, ObservationSet, Scenario, ScenarioInput,
    VerificationComparison,
};
