//! Whole-program reconstruction: canonical entity identity and observation
//! import for the auto-re Stage 1 pipeline.
//!
//! The [`identity`] module owns the canonical key that pins an entity to a
//! specific location in a binary revision, independent of any single
//! provider's row ids. The importer consumes provider observations and
//! routes every canonical mutation through [`ApplicationCommand`] so the
//! Stage 0 event store remains the single source of truth.

pub mod fingerprint;
pub mod identity;
pub mod work_graph;

#[cfg(test)]
mod tests_support;

pub use fingerprint::{
    FingerprintComparison, FingerprintInput, FingerprintSnapshot, InMemorySnapshot,
    InvalidationPropagator, compare_fingerprint, compute_fingerprint,
};
pub use identity::{
    CanonicalEntityKey, ImportSummary, ObservationImporter, entity_kind_for_observation_kind,
    entity_kind_from_observation, work_item_kind_for_entity,
};
pub use work_graph::{DependencyEdgeKind, WorkGraph, WorkGraphBuilder, WorkItemNode};
