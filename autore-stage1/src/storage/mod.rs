//! Stage 1 storage module.
//!
//! Re-exports the Stage 0 SQLite `Database` and `Transaction` types that
//! stage1 builds on top of, and exposes the M1 repository traits and
//! SQLite implementations that live in this crate.

pub use autore_store::storage::{Database, Transaction};

pub mod repositories;
