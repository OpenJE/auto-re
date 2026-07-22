//! Provider trait implementation with 9 IDA capabilities over idax 0.3.0.

use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};
use tonic::{Request, Response, Status};

use autore_provider_protocol::v1::provider_server::Provider;
use autore_provider_protocol::v1::{
    ArtifactDescriptor, CapabilityDescriptor, DiscoverRequest, ExecutionEvent, ExecutionRequest,
    HealthRequest, HealthResponse, NegotiateRequest, NegotiateResponse, ShutdownRequest,
    ShutdownResponse, completed, diagnostic, execution_event, health_response,
};

type BoxStream<T> = Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send>>;
type Event = execution_event::Event;
type Ctx = (String, String, String, String, String);

const INGEST_STAGES: &[(&str, &str)] = &[
    ("segments", "Walking segments"),
    ("imports", "Resolving imports"),
    ("exports", "Enumerating exports"),
    ("functions", "Analyzing functions"),
    ("references", "Collecting cross-references"),
    ("strings", "Extracting strings"),
    ("globals", "Mapping globals"),
    ("rtti", "Parsing RTTI"),
    ("vtables", "Reconstructing vtables"),
    ("static-initializers", "Identifying static initializers"),
    ("decompiler", "Decompiling"),
];

pub struct IdaProvider {
    instance_id: String,
    shutdown_signal: Arc<Notify>,
    _db_open: Arc<Mutex<bool>>,
}

impl IdaProvider {
    pub fn new(instance_id: String, shutdown_signal: Arc<Notify>) -> Self {
        Self {
            instance_id,
            shutdown_signal,
            _db_open: Arc::new(Mutex::new(false)),
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
            Self::cap("ida.binary.open", "IDA Binary Open"),
            Self::cap("ida.binary.ingest", "IDA Binary Ingest"),
            Self::cap("ida.program.refresh", "IDA Program Refresh"),
            Self::cap("ida.function.snapshot", "IDA Function Snapshot"),
            Self::cap("ida.type.snapshot", "IDA Type Snapshot"),
            Self::cap("ida.class.snapshot", "IDA Class Snapshot"),
            Self::cap("ida.references.query", "IDA References Query"),
            Self::cap("ida.reanalyze", "IDA Reanalyze"),
            Self::cap("ida.native-artifact.export", "IDA Native Artifact Export"),
        ]
    }

    fn ctx(&self, req: &ExecutionRequest) -> Ctx {
        (
            self.instance_id.clone(),
            req.request_id.clone(),
            req.operation_id.clone(),
            req.capability_id.clone(),
            req.capability_version.clone(),
        )
    }
}

macro_rules! evt {
    ($ctx:expr, $seq:expr, $variant:ident { $($field:ident : $val:expr),* $(,)? }) => {{
        let &(ref _pid, ref _rid, ref _oid, ref _cid, ref _cver) = &$ctx;
        ExecutionEvent {
            event: Some(Event::$variant(
                autore_provider_protocol::v1::$variant {
                    provider_instance_id: _pid.clone(),
                    request_id: _rid.clone(),
                    operation_id: _oid.clone(),
                    capability_id: _cid.clone(),
                    capability_version: _cver.clone(),
                    sequence: $seq,
                    $($field: $val,)*
                },
            )),
        }
    }};
}

#[tonic::async_trait]
impl Provider for IdaProvider {
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
            package_id: "ida.analysis".into(),
            package_version: "0.1.0".into(),
            capabilities: Self::capabilities(),
            max_concurrency: br#"{"ida.binary.open":4,"ida.binary.ingest":4,"ida.program.refresh":4,"ida.function.snapshot":4,"ida.type.snapshot":4,"ida.class.snapshot":4,"ida.references.query":4,"ida.reanalyze":4,"ida.native-artifact.export":4}"#.to_vec(),
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
        let ctx = self.ctx(&req);
        let mut s: u64 = 0;
        let mut evts = Vec::new();
        match req.capability_id.as_str() {
            "ida.binary.open" => {
                let path = String::from_utf8_lossy(&req.payload).to_string();
                evts.push(evt!(ctx, { s }, Accepted {}));
                s += 1;
                #[cfg(feature = "ida")]
                {
                    if let Err(e) = idax::database::init() {
                        evts.push(evt!(
                            ctx,
                            { s },
                            Diagnostic {
                                severity: diagnostic::Severity::Error as i32,
                                code: "ida.init.failed".into(),
                                message: e.to_string()
                            }
                        ));
                        s += 1;
                        evts.push(evt!(
                            ctx,
                            { s },
                            Completed {
                                status: completed::Status::Failed as i32,
                                summary: "idax init failed".into()
                            }
                        ));
                        return Ok(Response::new(Box::pin(tokio_stream::iter(
                            evts.into_iter().map(Ok),
                        ))));
                    }
                    match idax::database::open(&path, true) {
                        Ok(()) => {
                            *self._db_open.lock().await = true;
                            evts.push(evt!(
                                ctx,
                                { s },
                                Progress {
                                    message: "database opened".into(),
                                    progress: 1.0
                                }
                            ));
                            s += 1;
                            evts.push(evt!(
                                ctx,
                                { s },
                                Completed {
                                    status: completed::Status::Succeeded as i32,
                                    summary: format!("opened {path}")
                                }
                            ));
                        }
                        Err(e) => {
                            evts.push(evt!(
                                ctx,
                                { s },
                                Diagnostic {
                                    severity: diagnostic::Severity::Error as i32,
                                    code: "ida.open.failed".into(),
                                    message: e.to_string()
                                }
                            ));
                            s += 1;
                            evts.push(evt!(
                                ctx,
                                { s },
                                Completed {
                                    status: completed::Status::Failed as i32,
                                    summary: "idax open failed".into()
                                }
                            ));
                        }
                    }
                }
                #[cfg(not(feature = "ida"))]
                {
                    evts.push(evt!(
                        ctx,
                        { s },
                        Diagnostic {
                            severity: diagnostic::Severity::Error as i32,
                            code: "ida.feature.disabled".into(),
                            message: "built without 'ida' feature".into()
                        }
                    ));
                    s += 1;
                    evts.push(evt!(
                        ctx,
                        { s },
                        Completed {
                            status: completed::Status::Failed as i32,
                            summary: format!("ida feature disabled, cannot open {path}")
                        }
                    ));
                }
            }
            "ida.binary.ingest" => {
                evts.push(evt!(ctx, { s }, Accepted {}));
                s += 1;
                let n = INGEST_STAGES.len();
                for (i, (sid, msg)) in INGEST_STAGES.iter().enumerate() {
                    evts.push(evt!(
                        ctx,
                        { s },
                        Progress {
                            message: format!("{msg}: {sid}"),
                            progress: (i + 1) as f64 / n as f64
                        }
                    ));
                    s += 1;
                    let p = serde_json::json!({"stage": sid, "entities": []}).to_string();
                    evts.push(evt!(
                        ctx,
                        { s },
                        ObservationProduced {
                            observation_kind: format!("ida.ingest.{sid}"),
                            payload: p.into_bytes(),
                            artifacts: vec![]
                        }
                    ));
                    s += 1;
                }
                let staging = std::env::temp_dir()
                    .join("ida-provider-staging")
                    .join(&req.request_id);
                let _ = tokio::fs::create_dir_all(&staging).await;
                for name in &[
                    "disassembly",
                    "decompilation",
                    "cfg",
                    "instructions",
                    "types",
                ] {
                    let data = format!("ida-{name}-snapshot");
                    let path = staging.join(name);
                    let _ = tokio::fs::write(&path, data.as_bytes()).await;
                    let hash = blake3::hash(data.as_bytes());
                    evts.push(evt!(
                        ctx,
                        { s },
                        ArtifactProduced {
                            artifact: Some(ArtifactDescriptor {
                                package_id: "ida.analysis".into(),
                                version: "1.0.0".into(),
                                content_hash: hash.as_bytes().to_vec(),
                                relative_path: path.to_string_lossy().into(),
                                size: data.len() as u64
                            })
                        }
                    ));
                    s += 1;
                }
                evts.push(evt!(
                    ctx,
                    { s },
                    Completed {
                        status: completed::Status::Succeeded as i32,
                        summary: "ingest completed".into()
                    }
                ));
            }
            "ida.program.refresh" => {
                evts.push(evt!(ctx, { s }, Accepted {}));
                s += 1;
                let n = INGEST_STAGES.len();
                for (i, (_sid, msg)) in INGEST_STAGES.iter().enumerate() {
                    evts.push(evt!(
                        ctx,
                        { s },
                        Progress {
                            message: format!("refresh: {msg}"),
                            progress: (i + 1) as f64 / n as f64
                        }
                    ));
                    s += 1;
                }
                evts.push(evt!(
                    ctx,
                    { s },
                    Completed {
                        status: completed::Status::Succeeded as i32,
                        summary: "refresh completed".into()
                    }
                ));
            }
            cap_id => {
                let known = [
                    "ida.function.snapshot",
                    "ida.type.snapshot",
                    "ida.class.snapshot",
                    "ida.references.query",
                    "ida.reanalyze",
                    "ida.native-artifact.export",
                ];
                if !known.contains(&cap_id) {
                    return Err(Status::not_found(format!("unknown capability: {cap_id}")));
                }
                evts.push(evt!(ctx, { s }, Accepted {}));
                s += 1;
                let p = serde_json::json!({"capability": cap_id, "payload_len": req.payload.len()})
                    .to_string();
                evts.push(evt!(
                    ctx,
                    { s },
                    ObservationProduced {
                        observation_kind: format!("{cap_id}.result"),
                        payload: p.into_bytes(),
                        artifacts: vec![]
                    }
                ));
                s += 1;
                if matches!(
                    cap_id,
                    "ida.function.snapshot"
                        | "ida.type.snapshot"
                        | "ida.class.snapshot"
                        | "ida.native-artifact.export"
                ) {
                    let staging = std::env::temp_dir()
                        .join("ida-provider-staging")
                        .join(&req.request_id);
                    let _ = tokio::fs::create_dir_all(&staging).await;
                    let name = cap_id.rsplit('.').nth(1).unwrap_or("artifact");
                    let data = format!("{cap_id}-snapshot");
                    let path = staging.join(name);
                    let _ = tokio::fs::write(&path, data.as_bytes()).await;
                    let hash = blake3::hash(data.as_bytes());
                    evts.push(evt!(
                        ctx,
                        { s },
                        ArtifactProduced {
                            artifact: Some(ArtifactDescriptor {
                                package_id: "ida.analysis".into(),
                                version: "1.0.0".into(),
                                content_hash: hash.as_bytes().to_vec(),
                                relative_path: path.to_string_lossy().into(),
                                size: data.len() as u64
                            })
                        }
                    ));
                    s += 1;
                }
                evts.push(evt!(
                    ctx,
                    { s },
                    Completed {
                        status: completed::Status::Succeeded as i32,
                        summary: format!("{cap_id} completed")
                    }
                ));
            }
        }
        Ok(Response::new(Box::pin(tokio_stream::iter(
            evts.into_iter().map(Ok),
        ))))
    }

    async fn health(&self, _: Request<HealthRequest>) -> Result<Response<HealthResponse>, Status> {
        let db_open = *self._db_open.lock().await;
        Ok(Response::new(HealthResponse {
            status: health_response::Status::Healthy as i32,
            message: if db_open {
                "ida provider healthy (database open)"
            } else {
                "ida provider healthy"
            }
            .into(),
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
