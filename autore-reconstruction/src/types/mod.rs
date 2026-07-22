//! Deterministic layout constraint model and reconciliation for type
//! reconstruction.
//!
//! The [`constraint`] sub-module defines the 11 layout constraint kinds and
//! the in-memory store used to canonicalise them as evidence. The
//! [`reconciler`] sub-module performs the deterministic pre-LLM merge step
//! and emits either a layout hypothesis or a conflict-resolution work item.
//! The [`verification`] sub-module tracks per-field verification of canonical
//! type hypotheses per spec §10.4. The [`conflict`] sub-module arbitrates
//! unresolved layout conflicts via the `llm.analysis.conflict` capability.

pub mod conflict;
pub mod constraint;
#[path = "declaration.rs"]
pub mod declaration_gen;
pub mod reconciler;
#[path = "verification.rs"]
pub mod verification_split;

pub use constraint::{
    EVIDENCE_PREDICATE_LAYOUT_CONSTRAINT, LayoutConstraint, LayoutConstraintKind,
    LayoutConstraintStore, OPERATION_LAYOUT_RECONCILIATION,
};
pub use declaration_gen::{
    BUILD_FAILURE_PREFIX, DeclarationGenerator, DeclarationOutput, entity_to_source_path,
    render_struct_decl, render_vtable_decl,
};
pub use reconciler::{
    CONFLICT_RESOLUTION_PREFIX, LAYOUT_HYPOTHESIS_PREDICATE, ReconciledBaseAdjustment,
    ReconciledField, ReconciledLayout, ReconciledParameterUsage, ReconciledReturnValueUse,
    ReconciledVtableSlot, Reconciler,
};
pub use verification_split::{
    CanonicalTypeStore, VerificationField, applicable_verification_fields, compute_confidence,
    is_fully_verified,
};
