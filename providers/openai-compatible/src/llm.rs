//! OpenAI-compatible HTTP client abstraction with bounded-1 retry.
//!
//! The primary path submits a prompt with `response_format: json_schema`.
//! On parse or schema-validation failure the caller retries exactly once
//! with a schema-repair prompt (spec §8.7). A `Responder` trait allows
//! deterministic mock HTTP in tests. The plaintext API key is resolved
//! from `api_key_ref` at call time and never persisted or echoed.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Response returned by the `Responder` abstraction.
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// Abstract HTTP transport for the LLM call. Tests inject a mock responder
/// to avoid real network IO; production uses `RealResponder`.
pub trait Responder: Send + Sync {
    fn call<'a>(
        &'a self,
        url: &'a str,
        api_key: &'a str,
        body: &'a Value,
    ) -> BoxFuture<'a, Result<HttpResponse, LlmError>>;
}

/// Errors from the LLM client.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("http non-2xx: status {0} body={1}")]
    NonSuccess(u16, String),
    #[error("response parse error: {0}")]
    Parse(String),
    #[error("schema validation failed: {0}")]
    Validation(String),
    #[error("invalid output (recoverable): {source}")]
    InvalidOutput {
        raw_body: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("request timed out")]
    Timeout,
}

/// Successful submission: raw body plus parsed + validated JSON.
pub struct SubmitResult {
    pub raw_body: String,
    pub parsed: Value,
}

/// Production HTTP responder using `reqwest`.
pub struct RealResponder {
    pub client: reqwest::Client,
    pub timeout: std::time::Duration,
}

impl Responder for RealResponder {
    fn call<'a>(
        &'a self,
        url: &'a str,
        api_key: &'a str,
        body: &'a Value,
    ) -> BoxFuture<'a, Result<HttpResponse, LlmError>> {
        Box::pin(async move {
            let response = self
                .client
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .header(AUTHORIZATION, format!("Bearer {api_key}"))
                .timeout(self.timeout)
                .json(body)
                .send()
                .await?;
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            Ok(HttpResponse { status, body: text })
        })
    }
}

/// Mock responder for deterministic tests. The closure is invoked once per
/// call and must return a future. Wrap an `AtomicUsize` inside to vary the
/// response by call index when testing retry paths.
pub struct MockResponder<F>(pub F);

impl<F, Fut> Responder for MockResponder<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = Result<HttpResponse, LlmError>> + Send + 'static,
{
    fn call<'a>(
        &'a self,
        _url: &'a str,
        _api_key: &'a str,
        _body: &'a Value,
    ) -> BoxFuture<'a, Result<HttpResponse, LlmError>> {
        let fut = (self.0)();
        Box::pin(fut)
    }
}

/// OpenAI-compatible client. Holds the endpoint URL, a key reference (NOT
/// the plaintext secret), model, sampling, and the responder implementation.
pub struct OpenAiClient {
    endpoint_url: String,
    api_key_ref: String,
    model_name: String,
    temperature: f64,
    max_tokens: u32,
    responder: Arc<dyn Responder>,
}

impl OpenAiClient {
    /// Construct a client using real HTTP with `reqwest`.
    pub fn new(
        endpoint_url: String,
        api_key_ref: String,
        model_name: String,
        temperature: f64,
        max_tokens: u32,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self {
            endpoint_url,
            api_key_ref,
            model_name,
            temperature,
            max_tokens,
            responder: Arc::new(RealResponder {
                client,
                timeout: std::time::Duration::from_secs(120),
            }),
        }
    }

    /// Construct a client backed by a mock responder (used by tests).
    pub fn with_mock(responder: impl Responder + 'static) -> Self {
        Self {
            endpoint_url: "http://127.0.0.1:0/v1/chat/completions".into(),
            api_key_ref: "env:TEST_KEY_REF".into(),
            model_name: "mock".into(),
            temperature: 0.0,
            max_tokens: 1024,
            responder: Arc::new(responder),
        }
    }

    /// Override the key reference (used by tests to inject a known plaintext
    /// key so assertions can confirm it never leaks into artifacts).
    pub fn set_api_key_ref(&mut self, key_ref: String) {
        self.api_key_ref = key_ref;
    }

    /// Resolve the plaintext API key from the configured reference.
    fn resolve_key(&self) -> Result<String, LlmError> {
        if let Some(var) = self.api_key_ref.strip_prefix("env:") {
            std::env::var(var)
                .map_err(|e| LlmError::Parse(format!("failed to read key from env {var}: {e}")))
        } else {
            Ok(self.api_key_ref.clone())
        }
    }

    /// Submit a prompt and validate the response against `response_schema`.
    /// Returns `InvalidOutput` on parse/validation failure (caller retries).
    pub async fn submit(
        &self,
        prompt: &str,
        response_schema: Value,
        capability_id: &str,
    ) -> Result<SubmitResult, LlmError> {
        let api_key = self.resolve_key()?;
        let schema_name = capability_id.replace('.', "_");

        let request_body = serde_json::json!({
            "model": self.model_name,
            "temperature": self.temperature,
            "max_tokens": self.max_tokens,
            "messages": [{"role": "user", "content": prompt}],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "strict": true,
                    "schema": response_schema,
                }
            }
        });

        let response = self
            .responder
            .call(&self.endpoint_url, &api_key, &request_body)
            .await?;

        if response.status < 200 || response.status >= 300 {
            return Err(LlmError::NonSuccess(response.status, response.body));
        }

        let root: Value =
            serde_json::from_str(&response.body).map_err(|e| LlmError::InvalidOutput {
                raw_body: response.body.clone(),
                source: Box::new(e),
            })?;

        let content = root
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| LlmError::InvalidOutput {
                raw_body: response.body.clone(),
                source: String::from("missing choices[0].message.content").into(),
            })?;

        let parsed: Value = serde_json::from_str(content).map_err(|e| LlmError::InvalidOutput {
            raw_body: response.body.clone(),
            source: Box::new(e),
        })?;

        validate_against_schema(&parsed, &response_schema)?;

        Ok(SubmitResult {
            raw_body: response.body,
            parsed,
        })
    }
}

/// Validate a parsed value against a JSON Schema using `jsonschema`.
pub fn validate_against_schema(value: &Value, schema: &Value) -> Result<(), LlmError> {
    let validator = jsonschema::validator_for(schema)
        .map_err(|e| LlmError::Validation(format!("schema compile failed: {e}")))?;
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| format!("{} at {}", e, e.instance_path))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LlmError::Validation(errors.join("; ")))
    }
}
