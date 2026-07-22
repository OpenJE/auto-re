//! Work-item input fingerprinting and bounded downstream invalidation.
//!
//! The [`compute`] sub-module produces a deterministic BLAKE3 content hash
//! over every input that can affect a work item's output.  The
//! [`invalidate`] sub-module propagates invalidation through
//! `GeneratedDeclRequirement` and `BuildDependency` edges only.

pub mod compute;
pub mod invalidate;

#[cfg(test)]
mod tests;

pub use compute::{
    FingerprintComparison, FingerprintInput, compare_fingerprint, compute_fingerprint,
};
pub use invalidate::{FingerprintSnapshot, InMemorySnapshot, InvalidationPropagator};
