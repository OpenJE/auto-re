//! Verification scenario definition.
//!
//! Re-exports [`super::types::Scenario`] and related helpers so callers can
//! build scenarios without importing the entire `types` module.

pub use super::types::{
    ComparisonLevel, ComparisonPolicy, InitialState, NormalizationRule, Scenario, ScenarioInput,
};
