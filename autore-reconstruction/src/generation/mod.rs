//! Deterministic project skeleton generation.
//!
//! This module implements the `ProjectSkeletonBuilder` that emits a
//! managed source tree with explicit stub files per canonical entity.
//! Every generated file is registered as an artifact through
//! `ApplicationCommand` variants — no direct storage access.
//!
//! # Design
//!
//! - Source paths are derived from [`EntityId`] UUIDs, never from
//!   `display_name` or content-derived names.
//! - Generation order follows spec §11.2: external declarations,
//!   constants/enums, globals, leaf functions, classes, vtables,
//!   static initializers, entrypoints.
//! - Every stub file contains a `reconstruction_status = "stubbed"`
//!   marker for explicit status tracking.
//!
//! [`EntityId`]: autore_schema::ids::EntityId

pub mod mapping;
pub mod skeleton;
pub mod stub;

#[cfg(test)]
mod tests;

pub use mapping::GeneratedSourceMappingIntent;
pub use skeleton::{FileRole, GeneratedFile, ProjectSkeletonBuilder, SkeletonManifest};
pub use stub::StubPolicy;
