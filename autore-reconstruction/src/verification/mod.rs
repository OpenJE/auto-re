//! Scenario capture/replay/comparison model.
//!
//! This module implements Todo 45 of the Stage 1 plan: typed, normalized,
//! differential verification. It sits on top of the Wave 7 dynamic pipeline and
//! routes all durable side effects through [`ApplicationCommand`].

pub mod comparator;
pub mod executor;
pub mod scenario;
pub mod types;

pub use comparator::compare;
pub use executor::{
    ObservationBackend, ObservationError, ScenarioExecutor, Wave7ObservationBackend,
};
pub use types::{
    ComparisonCounts, ComparisonLevel, ComparisonPolicy, ComparisonResult, ExecutionDiagnostic,
    InitialState, NormalizationRule, Observation, ObservationSet, Scenario, ScenarioInput,
    VerificationComparison,
};
