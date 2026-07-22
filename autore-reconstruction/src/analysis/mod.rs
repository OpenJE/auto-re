//! `autore-reconstruction::analysis` — bounded investigation bundle and
//! JSON-Schema contracts for the 7 LLM analysis capabilities.
//!
//! The [`InvestigationBundle`] is a handle-only context packet (≤ 64 KiB)
//! assembled by [`BundleBuilder`] from a [`WorkGraph`] and a [`BundleStore`].
//! [`request_payload_for`] serializes the bundle for an `ExecutionRequest`.
//!
//! [`WorkGraph`]: crate::work_graph::WorkGraph

pub mod builder;
pub mod bundle;
pub mod import;
pub mod schemas;

#[cfg(test)]
mod tests;

pub use builder::{BundleBuilder, BundleStore, StaticArtifactSet};
pub use bundle::{
    BUNDLE_MAX_BYTES, BuildDiagnosticSummary, CallSiteSummary, InvestigationBundle, StringSnippet,
};
pub use import::{LlmImportError, LlmImportResult, LlmImporter};
pub use schemas::{
    CAPABILITIES, request_payload_for, request_schema, response_schema_for,
    validate_request_payload, validate_response_payload,
};
