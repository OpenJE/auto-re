//! Provider trait implementation with 5 fixture capabilities.

use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Notify;
use tonic::{Request, Response, Status};

use autore_provider_protocol::v1::provider_server::Provider;
use autore_provider_protocol::v1::{
    ArtifactDescriptor, ArtifactProduced, CapabilityDescriptor, Completed, Diagnostic,
    DiscoverRequest, ExecutionEvent, ExecutionRequest, HealthRequest, HealthResponse,
    NegotiateRequest, NegotiateResponse, ObservationProduced, Progress, ShutdownRequest,
    ShutdownResponse,
};
use autore_provider_protocol::v1::{completed, diagnostic, execution_event, health_response};

type BoxStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type Event = execution_event::Event;

/// Fixture provider exposing 5 test capabilities.
pub struct FixtureProvider {
    instance_id: String,
    shutdown_signal: Arc<Notify>,
}

impl FixtureProvider {
    pub fn new(instance_id: String, shutdown_signal: Arc<Notify>) -> Self {
        Self {
            instance_id,
            shutdown_signal,
        }
    }

    fn cap(id: &str, name: &str) -> CapabilityDescriptor {
        CapabilityDescriptor {
            capability_id: id.into(),
            version: "1.0.0".into(),
            name: name.into(),
            request_schema: Vec::new(),
            response_schema: Vec::new(),
        }
    }

    fn capabilities() -> Vec<CapabilityDescriptor> {
        vec![
            Self::cap("fixture.echo", "Echo Fixture"),
            Self::cap("fixture.delay", "Delay Fixture"),
            Self::cap("fixture.fail", "Fail Fixture"),
            Self::cap("fixture.artifact", "Artifact Fixture"),
            Self::cap("fixture.large-stream", "Large Stream Fixture"),
        ]
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

    fn done(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        status: completed::Status,
        summary: &str,
    ) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::Completed(Completed {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                status: status as i32,
                summary: summary.into(),
            })),
        }
    }

    fn progress(&self, req: &ExecutionRequest, seq: u64, msg: &str, frac: f64) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::Progress(Progress {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                message: msg.into(),
                progress: frac,
            })),
        }
    }

    fn execute_echo(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let obs = ExecutionEvent {
            event: Some(Event::ObservationProduced(ObservationProduced {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: 1,
                observation_kind: "fixture.echo.observation".into(),
                payload: req.payload.clone(),
                artifacts: vec![],
            })),
        };
        vec![
            self.accepted(req, 0),
            obs,
            self.done(req, 2, completed::Status::Succeeded, "echo completed"),
        ]
    }

    async fn execute_delay(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let delay_ms = req
            .deadline
            .as_ref()
            .and_then(|d| d.relative_budget.as_ref())
            .map(|d| (d.seconds * 1000 + d.nanos as i64 / 1_000_000) as u64)
            .unwrap_or(100);
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        vec![
            self.accepted(req, 0),
            self.progress(req, 1, "delay elapsed", 1.0),
            self.done(req, 2, completed::Status::Succeeded, "delay completed"),
        ]
    }

    fn execute_fail(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let diag = ExecutionEvent {
            event: Some(Event::Diagnostic(Diagnostic {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: 1,
                severity: diagnostic::Severity::Error as i32,
                code: "fixture.fail.intentional".into(),
                message: "intentional failure".into(),
            })),
        };
        vec![
            self.accepted(req, 0),
            diag,
            self.done(req, 2, completed::Status::Failed, "fail completed"),
        ]
    }

    async fn execute_artifact(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let blob: Vec<u8> = (0..65536u32).map(|i| (i % 256) as u8).collect();
        let hash = blake3::hash(&blob);
        let temp_dir = std::env::temp_dir().join(format!("fixture-artifact-{}", req.request_id));
        let _ = tokio::fs::create_dir_all(&temp_dir).await;
        let artifact_path = temp_dir.join("data");
        let _ = tokio::fs::write(&artifact_path, &blob).await;
        let ap = ExecutionEvent {
            event: Some(Event::ArtifactProduced(ArtifactProduced {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: 1,
                artifact: Some(ArtifactDescriptor {
                    package_id: "fixture.artifact".into(),
                    version: "1.0.0".into(),
                    content_hash: hash.as_bytes().to_vec(),
                    relative_path: artifact_path.to_string_lossy().into(),
                    size: 65536,
                }),
            })),
        };
        vec![
            self.accepted(req, 0),
            ap,
            self.done(req, 2, completed::Status::Succeeded, "artifact completed"),
        ]
    }

    fn execute_large_stream(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let mut events = Vec::with_capacity(1026);
        events.push(self.accepted(req, 0));
        for i in 0..1024u64 {
            events.push(self.progress(
                req,
                i + 1,
                &format!("progress {i}/1024"),
                i as f64 / 1023.0,
            ));
        }
        events.push(self.done(
            req,
            1025,
            completed::Status::Succeeded,
            "large-stream completed",
        ));
        events
    }
}

#[tonic::async_trait]
impl Provider for FixtureProvider {
    async fn negotiate(
        &self,
        request: Request<NegotiateRequest>,
    ) -> Result<Response<NegotiateResponse>, Status> {
        let req = request.into_inner();
        let accepted = if req.min_supported <= 1 && req.max_supported >= 1 {
            1
        } else {
            return Err(Status::invalid_argument("unsupported protocol version"));
        };
        Ok(Response::new(NegotiateResponse {
            accepted_version: accepted, package_id: "fixture.echo".into(), package_version: "0.1.0".into(),
            capabilities: Self::capabilities(),
            max_concurrency: br#"{"fixture.echo":4,"fixture.delay":4,"fixture.fail":4,"fixture.artifact":4,"fixture.large-stream":4}"#.to_vec(),
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
        let events = match req.capability_id.as_str() {
            "fixture.echo" => self.execute_echo(&req),
            "fixture.delay" => self.execute_delay(&req).await,
            "fixture.fail" => self.execute_fail(&req),
            "fixture.artifact" => self.execute_artifact(&req).await,
            "fixture.large-stream" => self.execute_large_stream(&req),
            _ => {
                return Err(Status::not_found(format!(
                    "unknown capability: {}",
                    req.capability_id
                )));
            }
        };
        Ok(Response::new(Box::pin(tokio_stream::iter(
            events.into_iter().map(Ok),
        ))))
    }

    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: health_response::Status::Healthy as i32,
            message: "fixture provider healthy".into(),
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
