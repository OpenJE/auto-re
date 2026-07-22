//! OpenAI-compatible LLM provider gRPC service implementation.
//!
//! Exposes 7 analysis capabilities (spec §8.2) and 6 generation
//! capabilities (spec §11.4). On `Execute`, parses the bounded request
//! payload, validates it against the capability's JSON Schema, renders a
//! capability-specific prompt template, submits to the configured
//! OpenAI-compatible endpoint with `response_format: json_schema`, validates
//! the response via `jsonschema`, and emits raw + parsed observations. For
//! generation capabilities the candidate source bytes are staged via
//! `ArtifactTransport` and an `ArtifactProduced` event is emitted. On schema
//! validation failure, retries exactly once with a schema-repair prompt
//! (spec §8.7), then fails.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use tokio::sync::Notify;
use tonic::{Request, Response, Status};

use autore_provider_protocol::v1::provider_server::Provider;
use autore_provider_protocol::v1::{
    ArtifactDescriptor, ArtifactProduced, CapabilityDescriptor, DiscoverRequest, ExecutionEvent,
    ExecutionRequest, HealthRequest, HealthResponse, NegotiateRequest, NegotiateResponse,
    ObservationProduced, ShutdownRequest, ShutdownResponse, completed, diagnostic, execution_event,
    health_response,
};
use autore_provider_runtime::artifact::{ArtifactTransport, LocalStagingTransport};
use autore_schema::{ContentHash, ProviderInstanceId};

use crate::llm::{LlmError, OpenAiClient, validate_against_schema};
use crate::prompts::PromptRegistry;
use crate::schemas;

type BoxStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type Event = execution_event::Event;

/// Names of the 13 capabilities: 7 analysis + 6 generation.
pub const CAPABILITIES: &[&str] = &[
    "llm.analysis.function",
    "llm.analysis.type",
    "llm.analysis.class",
    "llm.analysis.subsystem",
    "llm.analysis.conflict",
    "llm.analysis.failure",
    "llm.experiment.design",
    "llm.generation.declaration",
    "llm.generation.type",
    "llm.generation.function",
    "llm.generation.cluster",
    "llm.generation.test",
    "llm.generation.repair",
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

/// OpenAI-compatible LLM provider exposing 7 analysis and 6 generation
/// capabilities.
pub struct OpenAiCompatibleProvider {
    instance_id: String,
    shutdown_signal: Arc<Notify>,
    #[allow(dead_code)]
    config: ProviderConfig,
    prompts: PromptRegistry,
    client: OpenAiClient,
    /// Root directory for staging generated candidate artifacts.
    staging_root: PathBuf,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        instance_id: String,
        shutdown_signal: Arc<Notify>,
        config: ProviderConfig,
        prompts: PromptRegistry,
        client: OpenAiClient,
        staging_root: PathBuf,
    ) -> Self {
        Self {
            instance_id,
            shutdown_signal,
            config,
            prompts,
            client,
            staging_root,
        }
    }

    /// Build capability descriptors with JSON Schema bytes (spec §8.2).
    fn capabilities() -> Vec<CapabilityDescriptor> {
        CAPABILITIES
            .iter()
            .map(|id| schemas::descriptor_for(id))
            .collect()
    }

    /// True if the capability is one of the six generation capabilities.
    fn is_generation(capability_id: &str) -> bool {
        schemas::GENERATION_CAPABILITIES.contains(&capability_id)
    }

    /// Convert the provider instance id string to a typed ID. If the string
    /// is not a valid UUID (e.g. in tests), mint a fresh UUIDv7 ID.
    fn provider_instance_id(&self) -> ProviderInstanceId {
        uuid::Uuid::parse_str(&self.instance_id)
            .map(ProviderInstanceId::from_uuid)
            .unwrap_or_else(|_| ProviderInstanceId::new())
    }

    /// Dispatch execute to the LLM handler. The capability must be one of
    /// the 13 declared in `CAPABILITIES`.
    async fn execute_llm_capability(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let request_schema = match schemas::request_schema_for(&req.capability_id) {
            Some(s) => s,
            None => {
                return self.fail_events(
                    req,
                    "unknown-capability",
                    &format!("unknown capability: {}", req.capability_id),
                );
            }
        };

        let payload_str = match std::str::from_utf8(&req.payload) {
            Ok(s) => s,
            Err(_) => {
                return self.fail_events(
                    req,
                    "invalid-request-payload",
                    "request payload is not valid UTF-8 JSON",
                );
            }
        };

        let payload_value: serde_json::Value = match serde_json::from_str(payload_str) {
            Ok(v) => v,
            Err(e) => {
                return self.fail_events(
                    req,
                    "invalid-request-payload",
                    &format!("request payload is not valid JSON: {e}"),
                );
            }
        };

        if let Err(e) = validate_against_schema(&payload_value, &request_schema) {
            return self.fail_events(
                req,
                "invalid-request-payload",
                &format!("request payload schema validation failed: {e}"),
            );
        }

        let prompt = if Self::is_generation(&req.capability_id) {
            let bundle_json = payload_value
                .get("bundle")
                .and_then(|v| serde_json::to_string(v).ok())
                .unwrap_or_else(|| "{}".to_string());
            let generation_context_json = payload_value
                .get("generation_context")
                .and_then(|v| serde_json::to_string(v).ok())
                .unwrap_or_else(|| "{}".to_string());
            match self.prompts.render_generation(
                &req.capability_id,
                &bundle_json,
                &generation_context_json,
            ) {
                Ok(p) => p,
                Err(e) => {
                    return self.fail_events(
                        req,
                        "prompt-render-failed",
                        &format!("prompt render failed: {e}"),
                    );
                }
            }
        } else {
            match self.prompts.render(&req.capability_id, payload_str) {
                Ok(p) => p,
                Err(e) => {
                    return self.fail_events(
                        req,
                        "prompt-render-failed",
                        &format!("prompt render failed: {e}"),
                    );
                }
            }
        };

        let response_schema = match schemas::response_schema_for(&req.capability_id) {
            Some(s) => s,
            None => {
                return self.fail_events(
                    req,
                    "unknown-capability",
                    &format!("no response schema for capability: {}", req.capability_id),
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
                    payload_str,
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

        if Self::is_generation(&req.capability_id) {
            match self.stage_generation_artifact(req, &parsed).await {
                Ok(descriptor) => {
                    events.push(self.artifact_produced(req, seq, descriptor));
                    seq += 1;
                }
                Err(e) => {
                    events.push(self.diagnostic(
                        req,
                        seq,
                        "artifact-stage-failed",
                        &format!("failed to stage generated candidate: {e}"),
                    ));
                    seq += 1;
                    events.push(self.completed_failed(
                        req,
                        seq,
                        start,
                        1,
                        0,
                        "artifact staging failed",
                    ));
                    return events;
                }
            }
        }

        events.push(self.completed_succeeded(req, seq, start, "LLM analysis completed"));
        events
    }

    /// Stage the candidate source bytes from a generation response and
    /// return an `ArtifactDescriptor` for the staged artifact.
    async fn stage_generation_artifact(
        &self,
        req: &ExecutionRequest,
        parsed: &serde_json::Value,
    ) -> Result<ArtifactDescriptor, String> {
        let field = candidate_source_field_for(&req.capability_id);
        let b64 = parsed
            .get(field)
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("missing {field}"))?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("invalid base64 in {field}: {e}"))?;
        let hash = ContentHash::blake3(&bytes);
        let size = bytes.len() as u64;

        let instance_id = self.provider_instance_id();
        let transport = LocalStagingTransport::new(
            self.staging_root.clone(),
            instance_id,
            req.request_id.clone(),
        );
        let handle = transport
            .stage_inbound(Bytes::from(bytes))
            .await
            .map_err(|e| format!("stage_inbound failed: {e}"))?;

        let relative_path = handle
            .staging_path()
            .join("data")
            .to_string_lossy()
            .into_owned();

        Ok(ArtifactDescriptor {
            package_id: "openai-compatible".into(),
            version: "1.0.0".into(),
            content_hash: hash.digest.clone(),
            relative_path,
            size,
        })
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

    fn artifact_produced(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        artifact: ArtifactDescriptor,
    ) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::ArtifactProduced(ArtifactProduced {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                artifact: Some(artifact),
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

fn candidate_source_field_for(capability_id: &str) -> &'static str {
    match capability_id {
        "llm.generation.repair" => "new_candidate_source_bytes",
        _ => "candidate_source_bytes",
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
            max_concurrency: br#"{"llm.analysis.function":2,"llm.analysis.type":2,"llm.analysis.class":2,"llm.analysis.subsystem":2,"llm.analysis.conflict":2,"llm.analysis.failure":2,"llm.experiment.design":2,"llm.generation.declaration":2,"llm.generation.type":2,"llm.generation.function":2,"llm.generation.cluster":2,"llm.generation.test":2,"llm.generation.repair":2}"#.to_vec(),
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
        let events = self.execute_llm_capability(&req).await;
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
        staging_root: PathBuf,
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
            staging_root,
        )
    }
}

#[cfg(test)]
mod generation {
    use super::*;
    use autore_provider_protocol::v1::provider_server::Provider;

    fn temp_staging_root() -> PathBuf {
        tempfile::TempDir::new().unwrap().path().to_path_buf()
    }

    #[test]
    fn provider_advertises_six_generation_capabilities() {
        let tmp = tempfile::TempDir::new().unwrap();
        let prompts = PromptRegistry::load(tmp.path());
        let client = OpenAiClient::with_mock(crate::llm::MockResponder(|| async move {
            Err::<crate::llm::HttpResponse, crate::llm::LlmError>(crate::llm::LlmError::Timeout)
        }));
        let provider = test_support::provider_for_tests(
            "01900000-0000-7000-8000-000000000001",
            prompts,
            client,
            temp_staging_root(),
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt
            .block_on(provider.negotiate(Request::new(NegotiateRequest {
                min_supported: 1,
                max_supported: 1,
                coordinator_id: "coord".into(),
            })))
            .expect("negotiate ok")
            .into_inner();

        let cap_ids: Vec<&str> = resp
            .capabilities
            .iter()
            .map(|c| c.capability_id.as_str())
            .collect();

        for id in schemas::GENERATION_CAPABILITIES {
            assert!(
                cap_ids.contains(id),
                "generation capability {id} must be advertised"
            );
        }

        for id in schemas::GENERATION_CAPABILITIES {
            let cap = resp
                .capabilities
                .iter()
                .find(|c| c.capability_id == *id)
                .unwrap();
            assert!(
                !cap.request_schema.is_empty(),
                "{id} request_schema must not be empty"
            );
            assert!(
                !cap.response_schema.is_empty(),
                "{id} response_schema must not be empty"
            );
        }
    }

    #[test]
    fn generation_function_schema_rejects_missing_entity_target_id() {
        let schema = schemas::generation_response_schema_for("llm.generation.function")
            .expect("generation.function schema defined");
        let validator = jsonschema::validator_for(&schema).expect("schema valid");
        let bad = serde_json::json!({
            "candidate_source_bytes": "Y29uc29sZS5sb2coJ2hpJyk=",
            "declarations_required": [],
            "assumptions": [],
            "dependencies": [],
            "unsupported_behavior": [],
            "proposed_tests": [],
            "source_evidence_references": []
        });
        let result = validator.validate(&bad);
        assert!(
            result.is_err(),
            "missing entity_target_id must fail validation"
        );
    }

    #[test]
    fn generation_test_schema_rejects_unsupported_test_kind() {
        let schema = schemas::generation_response_schema_for("llm.generation.test")
            .expect("generation.test schema defined");
        let validator = jsonschema::validator_for(&schema).expect("schema valid");
        let bad = serde_json::json!({
            "target_unit": "unit-1",
            "test_kind": "fuzz",
            "naive_expected_observations": ["obs1"],
            "candidate_source_bytes": "Y29uc29sZS5sb2coJ2hpJyk="
        });
        let result = validator.validate(&bad);
        assert!(
            result.is_err(),
            "unsupported test_kind must fail validation"
        );
    }

    #[test]
    fn generation_function_stages_candidate_artifact_with_mock_llm() {
        let source = "void generated_fn() {}";
        let b64 = base64::engine::general_purpose::STANDARD.encode(source);
        let generation_response = serde_json::json!({
            "entity_target_id": "019abcde-0000-7000-8000-000000000001",
            "candidate_source_bytes": b64,
            "declarations_required": [],
            "assumptions": ["assumes 32-bit int"],
            "dependencies": [],
            "unsupported_behavior": [],
            "proposed_tests": ["test-1"],
            "source_evidence_references": ["ev-1"]
        });
        let openai_body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": generation_response.to_string()
                }
            }]
        })
        .to_string();

        let mut mock = OpenAiClient::with_mock(crate::llm::MockResponder(move || {
            let body = openai_body.clone();
            async move {
                Ok::<crate::llm::HttpResponse, crate::llm::LlmError>(crate::llm::HttpResponse {
                    status: 200,
                    body,
                })
            }
        }));
        mock.set_api_key_ref("test-key".into());

        let staging_tmp = tempfile::TempDir::new().unwrap();
        let staging_root = staging_tmp.path().to_path_buf();
        let tmp = tempfile::TempDir::new().unwrap();
        let prompts = PromptRegistry::load(tmp.path());
        let provider = test_support::provider_for_tests(
            "01900000-0000-7000-8000-000000000001",
            prompts,
            mock,
            staging_root.clone(),
        );

        let payload = serde_json::json!({
            "bundle": {
                "subject_identity": "019abcde-0000-7000-8000-000000000001",
                "subject_entity_id": "019abcde-0000-7000-8000-000000000001",
                "callers_and_callees": [],
                "relevant_types": [],
                "relevant_globals": [],
                "strings_and_constants": [],
                "dynamic_observations": [],
                "accepted_hypotheses": [],
                "unresolved_conflicts": [],
                "compiler_diagnostics": [],
                "verification_failures": [],
                "requested_output_schema": {}
            },
            "generation_context": {
                "accepted_types": [],
                "accepted_specs": [],
                "generated_stubs": {}
            }
        });
        let req = ExecutionRequest {
            request_id: "req-generation-1".into(),
            operation_id: "op-generation-1".into(),
            capability_id: "llm.generation.function".into(),
            capability_version: "1.0.0".into(),
            payload: payload.to_string().into_bytes(),
            deadline: None,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let events: Vec<_> = rt.block_on(async {
            let stream = provider
                .execute(Request::new(req))
                .await
                .unwrap()
                .into_inner();
            tokio_stream::StreamExt::collect(stream).await
        });

        let mut saw_raw = false;
        let mut saw_parsed = false;
        let mut saw_artifact = false;
        let mut completed_ok = false;
        for ev in &events {
            let ev = ev.as_ref().unwrap();
            match &ev.event {
                Some(Event::ObservationProduced(o)) if o.observation_kind == "llm.raw-response" => {
                    saw_raw = true;
                }
                Some(Event::ObservationProduced(o))
                    if o.observation_kind == "llm.parsed-result" =>
                {
                    saw_parsed = true;
                }
                Some(Event::ArtifactProduced(_)) => {
                    saw_artifact = true;
                }
                Some(Event::Completed(c)) if c.status == completed::Status::Succeeded as i32 => {
                    completed_ok = true;
                }
                _ => {}
            }
        }
        assert!(saw_raw, "must emit llm.raw-response");
        assert!(saw_parsed, "must emit llm.parsed-result");
        assert!(saw_artifact, "must emit ArtifactProduced for generation");
        assert!(completed_ok, "must complete successfully");

        let instance_dir = staging_root.join("01900000-0000-7000-8000-000000000001");
        let request_dir = instance_dir.join("req-generation-1");
        assert!(request_dir.exists(), "staging request dir must exist");
        let entries: Vec<_> = std::fs::read_dir(&request_dir).unwrap().collect();
        assert!(
            !entries.is_empty(),
            "staging dir must contain artifact subdir"
        );
    }
}
