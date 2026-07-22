//! Work-graph construction for whole-program reconstruction.
//!
//! This module builds a dependency graph of [`ReconstructionWorkItem`]s from
//! semantic entities and their inter-entity dependency edges. Recursive
//! call cycles among `Function` nodes are collapsed into
//! `FunctionCluster` nodes via Kosaraju's SCC algorithm.
//!
//! All mutations route through [`AutoReClient`] commands — no direct
//! storage access.
//!
//! [`ReconstructionWorkItem`]: autore_schema::domain::records::ReconstructionWorkItem

pub mod builder;
pub mod graph;
pub mod kind;

#[cfg(test)]
mod tests;

pub use builder::WorkGraphBuilder;
pub use graph::{WorkGraph, WorkItemNode};
pub use kind::{
    DependencyEdgeKind, ENTITY_KIND_CLASS, ENTITY_KIND_ENTRYPOINT, ENTITY_KIND_ENUM,
    ENTITY_KIND_STATIC_INITIALIZER, ENTITY_KIND_VTABLE, work_item_kind_for_entity_kind,
};
