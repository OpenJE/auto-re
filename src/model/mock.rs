//! Deterministic mock model provider for testing.
//!
//! Returns two descriptors (Analyzer + Verifier) and produces the same
//! `FunctionAnalysisOutput`-shaped JSON on every call.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::provider::{
    ModelCapabilities, ModelClass, ModelDescriptor, ModelProvider, ModelRequest, ModelResponse,
};

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

/// A deterministic mock provider that returns schema-valid JSON.
#[derive(Debug, Clone, Default)]
pub struct MockModelProvider;

impl MockModelProvider {
    pub fn new() -> Self {
        Self
    }
}

// ---------------------------------------------------------------------------
// Deterministic fixtures
// ---------------------------------------------------------------------------

/// Deterministic JSON response shaped like a `FunctionAnalysisOutput`.
fn mock_analysis_json() -> serde_json::Value {
    serde_json::json!({
        "function_name": "sub_1000",
        "address": "0x1000",
        "summary": "Initializes the configuration subsystem",
        "confidence": 0.85,
        "classification": "initialization",
        "inputs": [
            {
                "name": "config_ptr",
                "type": "void*",
                "description": "Pointer to configuration buffer"
            }
        ],
        "outputs": [
            {
                "name": "status",
                "type": "int",
                "description": "0 on success, -1 on failure"
            }
        ],
        "side_effects": ["writes to global config table"],
        "calls": ["memcpy", "strlen"],
        "tags": ["init", "config"]
    })
}

fn analyzer_descriptor() -> ModelDescriptor {
    ModelDescriptor {
        id: "mock-analyzer-v1".into(),
        name: "Mock Analyzer".into(),
        class: ModelClass::Analyzer,
        capabilities: ModelCapabilities {
            json_mode: true,
            tool_use: false,
            analysis: true,
            verification: false,
        },
        max_context_tokens: 8192,
    }
}

fn verifier_descriptor() -> ModelDescriptor {
    ModelDescriptor {
        id: "mock-verifier-v1".into(),
        name: "Mock Verifier".into(),
        class: ModelClass::Verifier,
        capabilities: ModelCapabilities {
            json_mode: true,
            tool_use: true,
            analysis: false,
            verification: true,
        },
        max_context_tokens: 4096,
    }
}

// ---------------------------------------------------------------------------
// Trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl ModelProvider for MockModelProvider {
    async fn list_models(&self) -> crate::Result<Vec<ModelDescriptor>> {
        Ok(vec![analyzer_descriptor(), verifier_descriptor()])
    }

    async fn complete(
        &self,
        request: ModelRequest,
        cancel: CancellationToken,
    ) -> crate::Result<ModelResponse> {
        // Cooperative cancellation check before any work.
        if cancel.is_cancelled() {
            return Err(crate::Error::ModelProvider("cancelled".into()));
        }

        // Validate the requested model_id against known models.
        let models = self.list_models().await?;
        let _model = models
            .iter()
            .find(|m| m.id == request.model_id)
            .ok_or_else(|| {
                crate::Error::ModelProvider(format!("unknown model: {}", request.model_id))
            })?;

        // Yield to allow concurrent cancellation to propagate.
        tokio::task::yield_now().await;

        // Re-check cancellation after yield.
        if cancel.is_cancelled() {
            return Err(crate::Error::ModelProvider("cancelled".into()));
        }

        Ok(ModelResponse {
            content: mock_analysis_json().to_string(),
            tokens_used: 42,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_provider_lists_models() {
        let provider = MockModelProvider::new();
        let models = provider.list_models().await.expect("list_models failed");

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].class, ModelClass::Analyzer);
        assert_eq!(models[1].class, ModelClass::Verifier);
        assert_eq!(models[0].id, "mock-analyzer-v1");
        assert_eq!(models[1].id, "mock-verifier-v1");
    }

    #[tokio::test]
    async fn mock_provider_complete_returns_valid_json() {
        let provider = MockModelProvider::new();
        let cancel = CancellationToken::new();
        let request = ModelRequest {
            model_id: "mock-analyzer-v1".into(),
            prompt: "Analyze this function".into(),
            schema: Some(serde_json::json!({"type": "object"})),
        };

        let response = provider
            .complete(request, cancel)
            .await
            .expect("complete failed");

        // Content must parse as valid JSON.
        let parsed: serde_json::Value =
            serde_json::from_str(&response.content).expect("response is not valid JSON");

        // Verify FunctionAnalysisOutput shape.
        assert_eq!(parsed["function_name"], "sub_1000");
        assert_eq!(parsed["address"], "0x1000");
        assert!(parsed["summary"].is_string());
        assert!(parsed["confidence"].is_f64());
        assert!(parsed["inputs"].is_array());
        assert!(parsed["outputs"].is_array());
        assert!(parsed["side_effects"].is_array());
        assert!(parsed["calls"].is_array());
        assert!(parsed["tags"].is_array());
        assert_eq!(response.tokens_used, 42);
    }

    #[tokio::test]
    async fn mock_provider_cancels_on_token() {
        let provider = MockModelProvider::new();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let request = ModelRequest {
            model_id: "mock-analyzer-v1".into(),
            prompt: "Analyze this function".into(),
            schema: None,
        };

        let result = provider.complete(request, cancel).await;
        let err = result.expect_err("expected cancellation error");
        assert_eq!(err.to_string(), "model provider error: cancelled");
    }

    #[tokio::test]
    async fn mock_provider_descriptor_capabilities() {
        let provider = MockModelProvider::new();
        let models = provider.list_models().await.expect("list_models failed");

        let analyzer = &models[0];
        assert!(analyzer.capabilities.json_mode);
        assert!(analyzer.capabilities.analysis);
        assert!(!analyzer.capabilities.verification);
        assert!(!analyzer.capabilities.tool_use);
        assert_eq!(analyzer.max_context_tokens, 8192);

        let verifier = &models[1];
        assert!(verifier.capabilities.json_mode);
        assert!(verifier.capabilities.verification);
        assert!(verifier.capabilities.tool_use);
        assert!(!verifier.capabilities.analysis);
        assert_eq!(verifier.max_context_tokens, 4096);
    }
}
