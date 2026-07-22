//! gRPC `Provider` service implementation for the build provider.
//!
//! Routes 6 capabilities to the [`DockerMsvc2002BuildProvider`]:
//! `build.configure`, `build.compile`, `build.link`, `build.run-test`,
//! `build.collect-diagnostics`, `build.abort`.
// allow: SIZE_OK — single gRPC service with 6 capability handlers and
// shared event-construction helpers; splitting fragments the routing.

use std::path::PathBuf;
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
use autore_reconstruction::build::{
    BuildLogs, BuildProviderTrait, CompileUnit, DockerMsvc2002BuildProvider, GeneratorManifest,
};

type BoxStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type Event = execution_event::Event;

/// gRPC service wrappingping the build provider.
pub struct BuildProvider {
    instance_id: String,
    shutdown_signal: Arc<Notify>,
    inner: DockerMsvc2002BuildProvider,
}

impl BuildProvider {
    pub fn new(
        instance_id: String,
        shutdown_signal: Arc<Notify>,
        inner: DockerMsvc2002BuildProvider,
    ) -> Self {
        Self {
            instance_id,
            shutdown_signal,
            inner,
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
            Self::cap("build.configure", "Configure Build"),
            Self::cap("build.compile", "Compile Units"),
            Self::cap("build.link", "Link Target"),
            Self::cap("build.run-test", "Run Test"),
            Self::cap("build.collect-diagnostics", "Collect Diagnostics"),
            Self::cap("build.abort", "Abort Build"),
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

    fn artifact_event(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        path: &str,
        data: &[u8],
    ) -> ExecutionEvent {
        let hash = blake3::hash(data);
        ExecutionEvent {
            event: Some(Event::ArtifactProduced(ArtifactProduced {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                artifact: Some(ArtifactDescriptor {
                    package_id: "build.msvc2002".into(),
                    version: "0.1.0".into(),
                    content_hash: hash.as_bytes().to_vec(),
                    relative_path: path.into(),
                    size: data.len() as u64,
                }),
            })),
        }
    }

    fn observation_event(
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

    fn diagnostic_event(
        &self,
        req: &ExecutionRequest,
        seq: u64,
        code: &str,
        message: &str,
    ) -> ExecutionEvent {
        ExecutionEvent {
            event: Some(Event::Diagnostic(Diagnostic {
                provider_instance_id: self.instance_id.clone(),
                request_id: req.request_id.clone(),
                operation_id: req.operation_id.clone(),
                capability_id: req.capability_id.clone(),
                capability_version: req.capability_version.clone(),
                sequence: seq,
                severity: diagnostic::Severity::Error as i32,
                code: code.into(),
                message: message.into(),
            })),
        }
    }

    async fn execute_configure(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let payload: serde_json::Value = serde_json::from_slice(&req.payload).unwrap_or_default();
        let project_root = payload
            .get("project_root")
            .and_then(|v| v.as_str())
            .unwrap_or("/workspace");
        let cmake_generator = payload
            .get("cmake_generator")
            .and_then(|v| v.as_str())
            .unwrap_or("NMake Makefiles");
        let exe_target = payload
            .get("executable_target")
            .and_then(|v| v.as_str())
            .unwrap_or("output");

        let manifest = GeneratorManifest {
            project_root: PathBuf::from(project_root),
            cmake_generator: cmake_generator.into(),
            source_files: vec![],
            executable_target: exe_target.into(),
        };

        let mut events = vec![self.accepted(req, 0)];
        match self
            .inner
            .configure_project(&manifest, std::path::Path::new(project_root))
            .await
        {
            Ok(configured) => {
                events.push(self.artifact_event(
                    req,
                    1,
                    "build-stdout.log",
                    configured.stdout.as_bytes(),
                ));
                events.push(self.artifact_event(
                    req,
                    2,
                    "build-stderr.log",
                    configured.stderr.as_bytes(),
                ));
                let status = if configured.success {
                    completed::Status::Succeeded
                } else {
                    completed::Status::Failed
                };
                events.push(self.done(req, 3, status, "configure completed"));
            }
            Err(e) => {
                events.push(self.diagnostic_event(req, 1, "build.configure.error", &e.to_string()));
                events.push(self.done(req, 2, completed::Status::Failed, &e.to_string()));
            }
        }
        events
    }

    async fn execute_compile(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let payload: serde_json::Value = serde_json::from_slice(&req.payload).unwrap_or_default();
        let units: Vec<CompileUnit> = payload
            .get("units")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let mut events = vec![self.accepted(req, 0)];
        match self.inner.compile_units(&units).await {
            Ok(result) => {
                events.push(self.progress(req, 1, "compilation done", 1.0));
                let status = if result.success {
                    completed::Status::Succeeded
                } else {
                    completed::Status::Failed
                };
                events.push(self.done(req, 2, status, "compile completed"));
            }
            Err(e) => {
                events.push(self.diagnostic_event(req, 1, "build.compile.error", &e.to_string()));
                events.push(self.done(req, 2, completed::Status::Failed, &e.to_string()));
            }
        }
        events
    }

    async fn execute_link(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let payload: serde_json::Value = serde_json::from_slice(&req.payload).unwrap_or_default();
        let artifacts: Vec<PathBuf> = payload
            .get("target_artifacts")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let mut events = vec![self.accepted(req, 0)];
        match self.inner.link_target(&artifacts).await {
            Ok(result) => {
                let status = if result.success {
                    completed::Status::Succeeded
                } else {
                    completed::Status::Failed
                };
                events.push(self.done(req, 1, status, "link completed"));
            }
            Err(e) => {
                events.push(self.diagnostic_event(req, 1, "build.link.error", &e.to_string()));
                events.push(self.done(req, 2, completed::Status::Failed, &e.to_string()));
            }
        }
        events
    }

    async fn execute_run_test(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let payload: serde_json::Value = serde_json::from_slice(&req.payload).unwrap_or_default();
        let test_target = payload
            .get("test_target")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut events = vec![self.accepted(req, 0)];
        match self.inner.run_test(test_target).await {
            Ok(result) => {
                events.push(self.artifact_event(
                    req,
                    1,
                    "test-stdout.log",
                    result.stdout.as_bytes(),
                ));
                let status = if result.exit_code == 0 {
                    completed::Status::Succeeded
                } else {
                    completed::Status::Failed
                };
                events.push(self.done(req, 2, status, "test completed"));
            }
            Err(e) => {
                events.push(self.diagnostic_event(req, 1, "build.run-test.error", &e.to_string()));
                events.push(self.done(req, 2, completed::Status::Failed, &e.to_string()));
            }
        }
        events
    }

    async fn execute_collect_diagnostics(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        let payload: serde_json::Value = serde_json::from_slice(&req.payload).unwrap_or_default();
        let stderr = payload.get("stderr").and_then(|v| v.as_str()).unwrap_or("");
        let stdout = payload.get("stdout").and_then(|v| v.as_str()).unwrap_or("");

        let logs = BuildLogs {
            stdout: stdout.into(),
            stderr: stderr.into(),
        };

        let mut events = vec![self.accepted(req, 0)];
        match self.inner.collect_diagnostics(&logs).await {
            Ok(diags) => {
                let payload_json = serde_json::to_vec(&diags).unwrap_or_default();
                events.push(self.observation_event(req, 1, "build.diagnostics", payload_json));
                events.push(self.done(
                    req,
                    2,
                    completed::Status::Succeeded,
                    &format!("{} diagnostics collected", diags.len()),
                ));
            }
            Err(e) => {
                events.push(self.diagnostic_event(
                    req,
                    1,
                    "build.diagnostics.error",
                    &e.to_string(),
                ));
                events.push(self.done(req, 2, completed::Status::Failed, &e.to_string()));
            }
        }
        events
    }

    fn execute_abort(&self, req: &ExecutionRequest) -> Vec<ExecutionEvent> {
        vec![
            self.accepted(req, 0),
            self.done(req, 1, completed::Status::Succeeded, "abort acknowledged"),
        ]
    }
}

#[tonic::async_trait]
impl Provider for BuildProvider {
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
            accepted_version: accepted,
            package_id: "build.msvc2002".into(),
            package_version: "0.1.0".into(),
            capabilities: Self::capabilities(),
            max_concurrency: br#"{"build.configure":1,"build.compile":4,"build.link":1,"build.run-test":4,"build.collect-diagnostics":4,"build.abort":4}"#.to_vec(),
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
            "build.configure" => self.execute_configure(&req).await,
            "build.compile" => self.execute_compile(&req).await,
            "build.link" => self.execute_link(&req).await,
            "build.run-test" => self.execute_run_test(&req).await,
            "build.collect-diagnostics" => self.execute_collect_diagnostics(&req).await,
            "build.abort" => self.execute_abort(&req),
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
            message: "build provider healthy".into(),
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
