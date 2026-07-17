//! Model provider abstraction — trait, types, routing, and mock implementation.
//!
//! Re-exports all public items so callers can use
//! `crate::model::{ModelProvider, ModelClass, ModelRouter, MockModelProvider, ...}`.

mod mock;
mod provider;
pub mod router;

pub use mock::MockModelProvider;
pub use provider::{
    ModelCapabilities, ModelClass, ModelDescriptor, ModelProvider, ModelRequest, ModelResponse,
};
pub use router::ModelRouter;
