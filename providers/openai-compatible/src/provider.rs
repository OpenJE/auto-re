//! OpenAI-compatible LLM provider gRPC service implementation.
//!
//! Exposes 7 analysis capabilities (spec §8.2). On `Execute`, parses the
//! bounded investigation bundle, renders a capability-specific prompt template,
//! submits to the configured OpenAI-compatible endpoint with
//! `response_format: json_schema`, validates the response via `jsonschema`,
//! and emits raw + parsed observations. On schema validation failure, retries
//! exactly once with a schema-repair prompt (spec §8.7), then fails.

use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Notify;
use tonic::{Request, Response, Status};

use autore_provider_protocol::v1::provider_server::Provider;
use autore_provider_protocol::v1::{
    CapabilityDescriptor, DiscoverRequest, ExecutionEvent, ExecutionRequest, HealthRequest,
    HealthResponse, NegotiateRequest, NegotiateResponse, ObservationProduced, ShutdownRequest,
    ShutdownResponse, completed, diagnostic, execution_event, health_response,
};

use crate::llm::{LlmError, OpenAiClient};
use crate::prompts::PromptRegistry;
use crate::schemas;

type BoxStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type Event = execution_event::Event;

/// Names of the 7 analysis capabilities declared in spec §8.2.
pub const CAPABILITIES: &[&str] = &[
    "llm.analysis.function",
    "llm.analysis.type",
    "llm.analysis.class",
    "llm.analysis.subsystem",
    "llm.analysis.conflict",
    "llm.analysis.failure",
    "llm.experiment.design",
];

/// Resolved provider configuration. The plaintext API key is NEVER stored
/// here; only a reference (env var name or file path) from which the key is
/// fetched live for each HTTP request (spec §8.8).
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub endpoint_url: String,
    pub api_key_ref: String,
    pub model_name: String,
    pub temperature: f64,
    pub max_tokens: u32,
}

impl ProviderConfig {
    /// Read configuration from environment variables with sensible defaults.
    pub fn from_env() -> Self {
        Self {
            endpoint_url: std::env::var("AUTORE_LLM_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into()),
            api_key_ref: std::env::var("AUTORE_LLM_API_KEY_REF")
                .unwrap_or_else(|_| "env:AUTORE_LLM_API_KEY".into()),
            model_name: std::env::var("AUTORE_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into()),
            temperature: std::env::var("AUTORE_LLM_TEMPERATURE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.2),
            max_tokens: std::env::var("AUTORE_LLM_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(4096),
        }
    }

    /// Construct directly (used by tests).
    pub fn new_for_tests(endpoint_url: String, api_key_ref: String, model_name: String) -> Self {
        Self {
            endpoint_url,
            api_key_ref,
            model_name,
            temperature: 0.0,
            max_tokens: 1024,
        }
    }
}

/// OpenAI-compatible LLM provider exposing 7 analysis capabilities.
pub struct OpenAiCompatibleProvider {
    instance_id: String,
    shutdown_signal: Arc<Notify>,
    #[allow(dead_code)]
    config: ProviderConfig,
    prompts: PromptRegistry,
    client: OpenAiClient,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        instance_id: String,
        shutdown_signal: Arc<Notify>,
        config: ProviderConfig,
        prompts: PromptRegistry,
        client: OpenAiClient,
    ) -> Self {
        Self {
            instance_id,
            shutdown_signal,
            config,
            prompts,
            client,
        }
    }

    /// Build capability descriptors with JSON Schema bytes (spec §8.2).
    fn capabilities() -> Vec<CapabilityDescriptor> {
        CAPABILITIES
            .iter()
            .map(|id| schemas::descriptor_for(id))
            .collect()
    }

    /// Dispatch execute to the LLM analysis handler. The capability must be
    /// one of the 7 declared in `CAPABILITIES`.
    async fn execute_llm_analysis(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let response_schema = match schemas::response_schema_for(&req.capability_id) {
            Some(s) => s,
            None => {
                return self.fail_events(
                    req,
                    "unknown-capability",
                    &format!("unknown capability: {}", req.capability_id),
                );
            }
        };

        let bundle_str = match std::str::from_utf8(&req.payload) {
            Ok(s) => s,
            Err(_) => {
                return self.fail_events(
                    req,
                    "invalid-request-payload",
                    "investigation bundle is not valid UTF-8 JSON",
                );
            }
        };

        let prompt = match self.prompts.render(&req.capability_id, bundle_str) {
            Ok(p) => p,
            Err(e) => {
                return self.fail_events(
                    req,
                    "prompt-render-failed",
                    &format!("prompt render failed: {e}"),
                );
            }
        };

        let mut events = vec![self.accepted(req, 0)];
        let mut seq: u64 = 1;

        events.push(self.progress(req, seq, "submitting primary LLM request", 0.1));
        seq += 1;

        let start = tokio::time::Instant::now();
        let primary = self
            .client
            .submit(&prompt, response_schema.clone(), &req.capability_id)
            .await;

        let (raw_body, parsed) = match primary {
            Ok(r) => (r.raw_body, Some(r.parsed)),
            Err(LlmError::InvalidOutput { raw_body, source }) => {
                events.push(self.diagnostic(
                    req,
                    seq,
                    "invalid-llm-output",
                    &format!("primary output invalid: {source}"),
                ));
                seq += 1;
                events.push(self.progress(req, seq, "attempting schema-repair retry", 0.5));
                seq += 1;

                let repair_prompt = match self.prompts.render_schema_repair(
                    bundle_str,
                    &raw_body,
                    &source.to_string(),
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        events.push(self.diagnostic(
                            req,
                            seq,
                            "prompt-render-failed",
                            &format!("repair prompt render failed: {e}"),
                        ));
                        seq += 1;
                        events.push(self.completed_failed(
                            req,
                            seq,
                            start,
                            1,
                            0,
                            "repair prompt render failed",
                        ));
                        return events;
                    }
                };

                match self
                    .client
                    .submit(&repair_prompt, response_schema, &req.capability_id)
                    .await
                {
                    Ok(r) => (r.raw_body, Some(r.parsed)),
                    Err(e) => {
                        events.push(self.diagnostic(
                            req,
                            seq,
                            "invalid-llm-output",
                            &format!("retry failed: {e}"),
                        ));
                        seq += 1;
                        events.push(self.completed_failed(
                            req,
                            seq,
                            start,
                            2,
                            0,
                            "retry produced invalid output",
                        ));
                        return events;
                    }
                }
            }
            Err(e) => {
                events.push(self.diagnostic(
                    req,
                    seq,
                    "llm-request-failed",
                    &format!("LLM request failed: {e}"),
                ));
                seq += 1;
                events.push(self.completed_failed(req, seq, start, 1, 0, "LLM request failed"));
                return events;
            }
        };

        let parsed = parsed.expect("parsed is Some on success path");

        events.push(self.observation(req, seq, "llm.raw-response", raw_body.as_bytes().to_vec()));
        seq += 1;
        events.push(self.observation(
            req,
            seq,
            "llm.parsed-result",
            serde_json::to_vec(&parsed).unwrap_or_default(),
        ));
        seq += 1;
        events.push(self.completed_succeeded(req, seq, start, "LLM analysis completed"));
        events
    }

    fn accepted(&self, req: &ExecutionRequest, seq: u64) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::Accepted(autore_provider_protocol::v1::Accepted {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
            })),
        }
    }

    fn progress(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        message: &str,
        progress: f64,
    ) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::Progress(autore_provider_protocol::v1::Progress {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                message: message.into(),
                progress,
            })),
        }
    }

    fn diagnostic(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        code: &str,
        message: &str,
    ) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::Diagnostic(
                autore_provider_protocol::v1::Diagnostic {
                    provider_instance_id: self.instance_id.clone(),
                    request_id: req.request_id.clone(),
                    operation_id: req.operation_id.clone(),
                    capability_id: req.capability_id.clone(),
                    capability_version: req.capability_version.clone(),
                    sequence: seq,
                    severity: diagnostic::Severity::Error as i32,
                    code: code.into(),
                    message: message.into(),
                },
            )),
        }
    }

    fn observation(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        kind: &str,
        payload: Vec<u8>,
    ) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::ObservationProduced(ObservationProduced {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                observation_kind: kind.into(),
                payload,
                artifacts: vec![],
            })),
        }
    }

    fn completed_succeeded(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        start: tokio::time::Instant,
        summary: &str,
    ) -> ExecutionEvent {
        let duration_ms = start.elapsed().as_millis() as u64;
        let meta = serde_json::json!({
            "token_usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0},
            "duration_ms": duration_ms,
        });
        ExecutionEvent {
            event: Some(Event::Completed(autore_provider_protocol::v1::Completed {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                status: completed::Status::Succeeded as i32,
                summary: format!("{summary} | {meta}"),
            })),
        }
    }

    fn completed_failed(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        start: tokio::time::Instant,
        request_count: u32,
        retry_count: u32,
        summary: &str,
    ) -> ExecutionEvent {
        let duration_ms = start.elapsed().as_millis() as u64;
        let meta = serde_json::json!({
            "request_count": request_count,
            "retry_count": retry_count,
            "duration_ms": duration_ms,
        });
        ExecutionEvent {
            event: Some(Event::Completed(autore_provider_protocol::v1::Completed {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                status: completed::Status::Failed as i32,
                summary: format!("{summary} | {meta}"),
            })),
        }
    }

    fn fail_events(
        &self,
        req: &ExecutionRequest,
        code: &str,
        message: &str,
    ) -> Vec<ExecutionEvent> {
        let start = tokio::time::Instant::now();
        vec![
            self.accepted(req, 0),
            self.diagnostic(req, 1, code, message),
            self.completed_failed(req, 2, start, 0, 0, message),
        ]
    }
}

#[tonic::async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn negotiate(
        &self,
        request: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        let req = request.into_inner();
        if req.min_supported > 1 || req.max_supported < 1 {
            return Err(Status::invalid_argument("unsupported protocol version"));
        }
        Ok(Response::new(NegotiateResponse {
            accepted_version: 1,
            package_id: "openai-compatible".into(),
            package_version: "0.1.0".into(),
            capabilities: Self::capabilities(),
            max_concurrency: br#"{"llm.analysis.function":2,"llm.analysis.type":2,"llm.analysis.class":2,"llm.analysis.subsystem":2,"llm.analysis.conflict":2,"llm.analysis.failure":2,"llm.experiment.design":2}"#.to_vec(),
        }))
    }

    type DiscoverCapabilitiesStream = BoxStream<CapabilityDescriptor>;
    async fn discover_capabilities(
        &self,
        _: Request<DiscoverRequest>,
    ) -> Result<Response<Self::DiscoverCapabilitiesStream>, Status> {
        let caps: Vec<Result<CapabilityDescriptor, Status>> =
            Self::capabilities().into_iter().map(Ok).collect();
        Ok(Response::new(Box::pin(tokio_stream::iter(caps))))
    }

    type ExecuteStream = BoxStream<ExecutionEvent>;
    async fn execute(
        &self,
        request: Request<ExecutionRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let req = request.into_inner();
        if !CAPABILITIES.contains(&req.capability_id.as_str()) {
            return Err(Status::not_found(format!(
                "unknown capability: {}",
                req.capability_id
            )));
        }
        let events = self.execute_llm_analysis(&req).await;
        Ok(Response::new(Box::pin(tokio_stream::iter(
            events.into_iter().map(Ok),
        ))))
    }

    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: health_response::Status::Healthy as i32,
            message: "openai-compatible provider healthy".into(),
            active_operations: 0,
        }))
    }

    async fn graceful_shutdown(
        &self,
        _: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        self.shutdown_signal.notify_one();
        Ok(Response::new(ShutdownResponse {
            acknowledged: true,
            pending_operations: 0,
        }))
    }
}

/// Test-only helpers. Exposed unconditionally so integration tests in
/// `tests/` can construct a provider without spinning up a gRPC server.
pub mod test_support {
    use super::*;
    use crate::llm::OpenAiClient;

    /// Build a provider wired to the given client and prompt registry,
    /// suitable for direct invocation in unit tests (no gRPC server).
    pub fn provider_for_tests(
        instance_id: &str,
        prompts: PromptRegistry,
        client: OpenAiClient,
    ) -> OpenAiCompatibleProvider {
        let config = ProviderConfig::new_for_tests(
            "http://127.0.0.1:0/v1".into(),
            "env:TEST_KEY_REF".into(),
            "test-model".into(),
        );
        OpenAiCompatibleProvider::new(
            instance_id.into(),
            Arc::new(Notify::new()),
            config,
            prompts,
            client,
        )
    }
}
