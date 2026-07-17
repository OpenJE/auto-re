//! Model provider trait and associated types.
//!
//! Defines the abstraction for LLM inference backends and the
//! descriptor / request / response types that flow through it.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Model classification
// ---------------------------------------------------------------------------

/// Classification of a model's primary capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModelClass {
    Analyzer,
    Verifier,
    Summarizer,
    Generalist,
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// Feature flags describing what a model can do.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelCapabilities {
    pub json_mode: bool,
    pub tool_use: bool,
    pub analysis: bool,
    pub verification: bool,
}

// ---------------------------------------------------------------------------
// Descriptor
// ---------------------------------------------------------------------------

/// Static metadata for a single model offered by a provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelDescriptor {
    pub id: String,
    pub name: String,
    pub class: ModelClass,
    pub capabilities: ModelCapabilities,
    pub max_context_tokens: u64,
}

// ---------------------------------------------------------------------------
// Request / Response
// ---------------------------------------------------------------------------

/// A completion request to a model provider.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelRequest {
    pub model_id: String,
    pub prompt: String,
    /// Optional JSON Schema the response must conform to.
    pub schema: Option<serde_json::Value>,
}

/// A completion response from a model provider.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ModelResponse {
    /// Response content — valid JSON when the request included a schema.
    pub content: String,
    pub tokens_used: u64,
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Async trait for LLM inference backends.
///
/// Implementations must be `Send + Sync` so they can be shared across
/// tasks in a multi-threaded tokio runtime.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Lists all models available from this provider.
    async fn list_models(&self) -> crate::Result<Vec<ModelDescriptor>>;

    /// Produces a completion for the given request.
    ///
    /// Implementations must cooperatively check `cancel` and return
    /// `Error::ModelProvider("cancelled")` promptly when it fires.
    async fn complete(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
    ) -> crate::Result<ModelResponse>;
}
