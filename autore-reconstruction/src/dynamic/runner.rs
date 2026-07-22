//! Pluggable target runner abstraction for dynamic debugging.
//!
//! The [`TargetRunner`] trait abstracts launching/attaching, executing scenario
//! steps, capturing observations, and stopping a target process. The first
//! concrete implementation is [`WineGdbRunner`] (Wine + gdbserver on a
//! Linux/IDA host). A compile-time stub [`WindowsGdbServerRunner`] proves the
//! backend-agnostic trait seam for future Windows-native backends.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use autore_provider_protocol::v1::ArtifactDescriptor;
use autore_schema::ids::{ArtifactId, EntityId};

use super::scenario::Step;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur while driving a [`TargetRunner`].
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum RunnerError {
    /// The backend is not supported in this build (e.g. Windows-only stub).
    #[error("unsupported backend")]
    Unsupported,
    /// No target process is currently launched.
    #[error("target not launched")]
    NotLaunched,
    /// A target is already launched; `launch` was called twice.
    #[error("target already launched")]
    AlreadyLaunched,
    /// The request payload or step is invalid.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The runner failed to execute a step or capture an observation.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    /// Execution was cancelled before completion.
    #[error("cancelled")]
    Cancelled,
}

// ---------------------------------------------------------------------------
// Observations
// ---------------------------------------------------------------------------

/// A single dynamic observation captured from the target process.
///
/// Observations are accumulated in a [`CaptureContext`] and eventually emitted
/// by the provider as `ObservationProduced` events with
/// `observation_kind = "debug.observation"`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DebugObservation {
    /// Namespaced observation sub-kind, e.g. "registers", "arguments",
    /// "memory-region", "call-target".
    pub kind: String,
    /// Entity associated with the observation, if any.
    pub entity: Option<EntityId>,
    /// Address associated with the observation, if any.
    pub address: Option<u128>,
    /// Size in bytes, for memory observations.
    pub size: Option<usize>,
    /// Wall-clock timestamp in milliseconds since the UNIX epoch.
    pub timestamp_ms: u64,
    /// Structured observation payload.
    pub data: serde_json::Value,
}

/// Accumulates observations and staged artifacts produced by a debug session.
#[derive(Debug, Clone, Default)]
pub struct CaptureContext {
    /// Observations captured so far.
    pub observations: Vec<DebugObservation>,
    /// Artifact descriptors staged for the current session.
    pub artifacts: Vec<ArtifactDescriptor>,
}

impl CaptureContext {
    /// Creates an empty capture context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a debug observation.
    pub fn record_observation(
        &mut self,
        kind: &str,
        entity: Option<EntityId>,
        address: Option<u128>,
        size: Option<usize>,
        data: serde_json::Value,
    ) {
        let timestamp_ms = now_ms();
        self.observations.push(DebugObservation {
            kind: kind.into(),
            entity,
            address,
            size,
            timestamp_ms,
            data,
        });
    }

    /// Records a diagnostic observation (used by runners to surface warnings).
    pub fn record_diagnostic(&mut self, severity: &str, code: &str, message: &str) {
        self.record_observation(
            "diagnostic",
            None,
            None,
            None,
            serde_json::json!({
                "severity": severity,
                "code": code,
                "message": message,
            }),
        );
    }

    /// Stages an artifact descriptor in the context.
    pub fn stage_artifact(&mut self, artifact: ArtifactDescriptor) {
        self.artifacts.push(artifact);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// TargetRunner trait
// ---------------------------------------------------------------------------

/// Backend-agnostic driver for launching and controlling a debug target.
///
/// Implementations are responsible for the low-level debugger backend (Wine +
/// gdbserver, a native Windows gdbserver, etc.). The coordinator/provider layer
/// builds typed [`Scenario`](super::scenario::Scenario) ASTs and validates them
/// with [`ScenarioVerifier`](super::verifier::ScenarioVerifier) before calling
/// into a runner.
#[async_trait]
pub trait TargetRunner: Send + Sync {
    /// Launch a target process from the given binary artifact.
    async fn launch(
        &self,
        exe: ArtifactId,
        env: HashMap<String, String>,
        cwd: PathBuf,
    ) -> Result<(), RunnerError>;

    /// Attach to an already-running process.
    async fn attach(&self, pid: u32) -> Result<(), RunnerError>;

    /// Stop the target process and detach.
    async fn stop(&self) -> Result<(), RunnerError>;

    /// Execute a single scenario step, accumulating observations in `ctx`.
    async fn execute_step(&self, step: &Step, ctx: &mut CaptureContext) -> Result<(), RunnerError>;

    /// Convenience sub-flow: launch, break, run, capture arguments/return/registers.
    async fn capture_function(
        &self,
        entity: EntityId,
        run_count: u32,
    ) -> Result<CaptureContext, RunnerError>;

    /// Convenience sub-flow: trace a function to a given depth.
    async fn trace_function(
        &self,
        entity: EntityId,
        depth: u32,
    ) -> Result<CaptureContext, RunnerError>;

    /// Capture a contiguous memory region.
    async fn capture_memory(&self, addr: u128, size: usize) -> Result<CaptureContext, RunnerError>;

    /// Capture the call graph around a function entity.
    async fn capture_calls(&self, entity: EntityId) -> Result<CaptureContext, RunnerError>;
}

// ---------------------------------------------------------------------------
// WineGdbRunner
// ---------------------------------------------------------------------------

/// Internal launch state for the Wine + gdbserver runner.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LaunchState {
    exe: ArtifactId,
    env: HashMap<String, String>,
    cwd: PathBuf,
}

/// First concrete [`TargetRunner`] implementation using Wine + gdbserver.
///
/// In production, this runner would shell out to `wine` and `gdbserver`. For
/// tests and CI environments, set `AUTORE_TEST_MOCK_RUNNER=1` (or call
/// [`WineGdbRunner::mock`]) to produce deterministic observations without
/// spawning subprocesses.
#[derive(Debug)]
#[allow(dead_code)]
pub struct WineGdbRunner {
    wine_path: String,
    gdbserver_path: String,
    mock: bool,
    cancel: CancellationToken,
    launched: Mutex<Option<LaunchState>>,
}

impl WineGdbRunner {
    /// Creates a runner from environment variables.
    ///
    /// Reads `AUTORE_WINE_PATH` and `AUTORE_GDBSERVER_PATH` with sensible
    /// defaults. If `AUTORE_TEST_MOCK_RUNNER=1` is set, the runner operates in
    /// mock mode and skips real subprocesses.
    pub fn from_env() -> Self {
        let mock = std::env::var("AUTORE_TEST_MOCK_RUNNER")
            .map(|v| v == "1")
            .unwrap_or(false);
        let wine_path = std::env::var("AUTORE_WINE_PATH").unwrap_or_else(|_| "wine".into());
        let gdbserver_path =
            std::env::var("AUTORE_GDBSERVER_PATH").unwrap_or_else(|_| "gdbserver".into());
        Self::new(wine_path, gdbserver_path, mock)
    }

    /// Creates a new runner with explicit binary paths.
    pub fn new(wine_path: String, gdbserver_path: String, mock: bool) -> Self {
        Self {
            wine_path,
            gdbserver_path,
            mock,
            cancel: CancellationToken::new(),
            launched: Mutex::new(None),
        }
    }

    /// Creates a deterministic mock runner for tests.
    pub fn mock() -> Self {
        Self::new("wine".into(), "gdbserver".into(), true)
    }

    /// Triggers cancellation for any in-flight execution.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Resets the cancellation token so the runner can be reused in tests.
    pub fn reset_cancel(&self) {
        // CancellationToken does not support reset; callers should construct a
        // new runner for a fresh token. This method documents that limitation.
    }

    fn check_cancelled(&self) -> Result<(), RunnerError> {
        if self.cancel.is_cancelled() {
            return Err(RunnerError::Cancelled);
        }
        Ok(())
    }

    async fn ensure_launched(&self) -> Result<(), RunnerError> {
        let mut launched = self.launched.lock().await;
        if launched.is_some() {
            return Ok(());
        }
        if !self.mock {
            return Err(RunnerError::NotLaunched);
        }
        *launched = Some(LaunchState {
            exe: ArtifactId::new(),
            env: HashMap::new(),
            cwd: PathBuf::from("/"),
        });
        Ok(())
    }

    fn make_artifact_descriptor() -> ArtifactDescriptor {
        let data = b"mock-staged-debug-artifact";
        ArtifactDescriptor {
            package_id: "ida.analysis".into(),
            version: "1.0.0".into(),
            content_hash: blake3::hash(data).as_bytes().to_vec(),
            relative_path: "staging/mock-debug-artifact.bin".into(),
            size: data.len() as u64,
        }
    }
}

#[async_trait]
impl TargetRunner for WineGdbRunner {
    async fn launch(
        &self,
        exe: ArtifactId,
        env: HashMap<String, String>,
        cwd: PathBuf,
    ) -> Result<(), RunnerError> {
        self.check_cancelled()?;
        let mut launched = self.launched.lock().await;
        if launched.is_some() {
            return Err(RunnerError::AlreadyLaunched);
        }
        if !self.mock {
            // Real subprocess path: verify the binaries are available, then
            // spawn `wine <exe>` under a separate gdbserver. This build does not
            // implement the full Wine/GDB process plumbing; operators should use
            // the mock runner or the `tools/wine-launch-vanburen-gdb.sh` helper.
            return Err(RunnerError::ExecutionFailed(
                "real Wine/GDB backend not implemented in this build; \
                 set AUTORE_TEST_MOCK_RUNNER=1 for tests"
                    .into(),
            ));
        }
        *launched = Some(LaunchState { exe, env, cwd });
        Ok(())
    }

    async fn attach(&self, _pid: u32) -> Result<(), RunnerError> {
        self.check_cancelled()?;
        let mut launched = self.launched.lock().await;
        if launched.is_some() {
            return Err(RunnerError::AlreadyLaunched);
        }
        if !self.mock {
            return Err(RunnerError::ExecutionFailed(
                "real Wine/GDB attach not implemented in this build".into(),
            ));
        }
        *launched = Some(LaunchState {
            exe: ArtifactId::new(),
            env: HashMap::new(),
            cwd: PathBuf::from("/"),
        });
        Ok(())
    }

    async fn stop(&self) -> Result<(), RunnerError> {
        self.check_cancelled()?;
        let mut launched = self.launched.lock().await;
        if launched.is_none() {
            return Err(RunnerError::NotLaunched);
        }
        *launched = None;
        Ok(())
    }

    async fn execute_step(&self, step: &Step, ctx: &mut CaptureContext) -> Result<(), RunnerError> {
        if self.cancel.is_cancelled() {
            ctx.record_diagnostic("warning", "cancellation", "execution cancelled before step");
            return Err(RunnerError::Cancelled);
        }
        self.ensure_launched().await?;

        match step {
            Step::SetBreakpoint { entity } => {
                ctx.record_observation(
                    "breakpoint-set",
                    Some(*entity),
                    None,
                    None,
                    serde_json::json!({"action": "set"}),
                );
            }
            Step::RemoveBreakpoint { entity } => {
                ctx.record_observation(
                    "breakpoint-removed",
                    Some(*entity),
                    None,
                    None,
                    serde_json::json!({"action": "remove"}),
                );
            }
            Step::Continue => {
                ctx.record_observation(
                    "continue",
                    None,
                    None,
                    None,
                    serde_json::json!({"action": "continue"}),
                );
            }
            Step::Step => {
                ctx.record_observation(
                    "single-step",
                    None,
                    None,
                    None,
                    serde_json::json!({"action": "step"}),
                );
            }
            Step::Finish => {
                ctx.record_observation(
                    "finish",
                    None,
                    None,
                    None,
                    serde_json::json!({"action": "finish"}),
                );
            }
            Step::CaptureRegisters => {
                ctx.record_observation(
                    "registers",
                    None,
                    None,
                    None,
                    serde_json::json!({
                        "rax": 0,
                        "rbx": 0,
                        "rcx": 0,
                        "rdx": 0,
                        "rip": 0x401000,
                    }),
                );
            }
            Step::CaptureArguments { entity } => {
                ctx.record_observation(
                    "arguments",
                    Some(*entity),
                    None,
                    None,
                    serde_json::json!({
                        "captured_arguments": [
                            {"idx": 0, "value": "rcx=0xdeadbeef", "type": "u64"},
                            {"idx": 1, "value": "rdx=0", "type": "u64"},
                        ],
                    }),
                );
            }
            Step::CaptureReturnValue { entity } => {
                ctx.record_observation(
                    "return-value",
                    Some(*entity),
                    None,
                    None,
                    serde_json::json!({"value": "rax=0", "type": "u64"}),
                );
            }
            Step::CaptureMemoryRegion { addr, size } => {
                ctx.record_observation(
                    "memory-region",
                    None,
                    Some(*addr),
                    Some(*size),
                    serde_json::json!({"bytes": "mock"}),
                );
            }
            Step::CaptureMemoryDelta { addr, size } => {
                ctx.record_observation(
                    "memory-delta",
                    None,
                    Some(*addr),
                    Some(*size),
                    serde_json::json!({"before": "mock", "after": "mock"}),
                );
            }
            Step::CaptureGlobalValue { entity } => {
                ctx.record_observation(
                    "global-value",
                    Some(*entity),
                    None,
                    None,
                    serde_json::json!({"value": "mock"}),
                );
            }
            Step::CaptureCallTarget => {
                ctx.record_observation(
                    "call-target",
                    None,
                    None,
                    None,
                    serde_json::json!({"target": "mock"}),
                );
            }
            Step::CaptureExternalCall { api } => {
                ctx.record_observation(
                    "external-call",
                    None,
                    None,
                    None,
                    serde_json::json!({"api": api.to_string()}),
                );
            }
            Step::CaptureException => {
                ctx.record_observation(
                    "exception",
                    None,
                    None,
                    None,
                    serde_json::json!({"code": "mock"}),
                );
            }
        }
        Ok(())
    }

    async fn capture_function(
        &self,
        entity: EntityId,
        run_count: u32,
    ) -> Result<CaptureContext, RunnerError> {
        self.check_cancelled()?;
        let mut ctx = CaptureContext::new();
        // Sub-flow: set breakpoint, continue, capture arguments + return + registers.
        self.execute_step(&Step::SetBreakpoint { entity }, &mut ctx)
            .await?;
        self.execute_step(&Step::Continue, &mut ctx).await?;
        self.execute_step(&Step::CaptureArguments { entity }, &mut ctx)
            .await?;
        self.execute_step(&Step::CaptureReturnValue { entity }, &mut ctx)
            .await?;
        self.execute_step(&Step::CaptureRegisters, &mut ctx).await?;
        ctx.stage_artifact(Self::make_artifact_descriptor());
        ctx.record_observation(
            "function-capture-summary",
            Some(entity),
            None,
            None,
            serde_json::json!({"run_count": run_count}),
        );
        Ok(ctx)
    }

    async fn trace_function(
        &self,
        entity: EntityId,
        depth: u32,
    ) -> Result<CaptureContext, RunnerError> {
        self.check_cancelled()?;
        let mut ctx = CaptureContext::new();
        self.execute_step(&Step::SetBreakpoint { entity }, &mut ctx)
            .await?;
        for _ in 0..depth.max(1) {
            self.execute_step(&Step::Step, &mut ctx).await?;
        }
        ctx.stage_artifact(Self::make_artifact_descriptor());
        ctx.record_observation(
            "function-trace-summary",
            Some(entity),
            None,
            None,
            serde_json::json!({"depth": depth}),
        );
        Ok(ctx)
    }

    async fn capture_memory(&self, addr: u128, size: usize) -> Result<CaptureContext, RunnerError> {
        self.check_cancelled()?;
        let mut ctx = CaptureContext::new();
        self.execute_step(&Step::CaptureMemoryRegion { addr, size }, &mut ctx)
            .await?;
        ctx.stage_artifact(Self::make_artifact_descriptor());
        Ok(ctx)
    }

    async fn capture_calls(&self, entity: EntityId) -> Result<CaptureContext, RunnerError> {
        self.check_cancelled()?;
        let mut ctx = CaptureContext::new();
        self.execute_step(&Step::SetBreakpoint { entity }, &mut ctx)
            .await?;
        self.execute_step(&Step::CaptureCallTarget, &mut ctx)
            .await?;
        ctx.stage_artifact(Self::make_artifact_descriptor());
        ctx.record_observation(
            "calls-capture-summary",
            Some(entity),
            None,
            None,
            serde_json::json!({"callee_count": 1}),
        );
        Ok(ctx)
    }
}

// ---------------------------------------------------------------------------
// WindowsGdbServerRunner (compile-time stub)
// ---------------------------------------------------------------------------

/// Compile-time stub proving the [`TargetRunner`] seam is backend-agnostic.
///
/// This runner returns [`RunnerError::Unsupported`] for every operation. It
/// exists so that future Windows-native backends (e.g. x64dbg direct) can be
/// added without changing the coordinator/provider code.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsGdbServerRunner;

#[async_trait]
impl TargetRunner for WindowsGdbServerRunner {
    async fn launch(
        &self,
        _exe: ArtifactId,
        _env: HashMap<String, String>,
        _cwd: PathBuf,
    ) -> Result<(), RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn attach(&self, _pid: u32) -> Result<(), RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn stop(&self) -> Result<(), RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn execute_step(
        &self,
        _step: &Step,
        _ctx: &mut CaptureContext,
    ) -> Result<(), RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn capture_function(
        &self,
        _entity: EntityId,
        _run_count: u32,
    ) -> Result<CaptureContext, RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn trace_function(
        &self,
        _entity: EntityId,
        _depth: u32,
    ) -> Result<CaptureContext, RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn capture_memory(
        &self,
        _addr: u128,
        _size: usize,
    ) -> Result<CaptureContext, RunnerError> {
        Err(RunnerError::Unsupported)
    }

    async fn capture_calls(&self, _entity: EntityId) -> Result<CaptureContext, RunnerError> {
        Err(RunnerError::Unsupported)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use autore_schema::ids::{ArtifactId, EntityId};

    #[tokio::test]
    async fn mock_runner_launch_and_stop_lifecycle() {
        let runner = WineGdbRunner::mock();
        let exe = ArtifactId::new();
        runner
            .launch(exe, HashMap::new(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        runner.stop().await.unwrap();
    }

    #[tokio::test]
    async fn mock_runner_records_observation_for_step() {
        let runner = WineGdbRunner::mock();
        let exe = ArtifactId::new();
        runner
            .launch(exe, HashMap::new(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        let mut ctx = CaptureContext::new();
        let entity = EntityId::new();
        runner
            .execute_step(&Step::CaptureArguments { entity }, &mut ctx)
            .await
            .unwrap();
        assert!(
            ctx.observations
                .iter()
                .any(|o| o.kind == "arguments" && o.entity == Some(entity))
        );
    }

    #[tokio::test]
    async fn cancellation_emits_diagnostic() {
        let runner = WineGdbRunner::mock();
        runner
            .launch(ArtifactId::new(), HashMap::new(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        runner.cancel();
        let mut ctx = CaptureContext::new();
        let result = runner.execute_step(&Step::Continue, &mut ctx).await;
        assert!(matches!(result, Err(RunnerError::Cancelled)));
        assert!(ctx.observations.iter().any(|o| {
            o.kind == "diagnostic"
                && o.data.get("code").and_then(|v| v.as_str()) == Some("cancellation")
                && o.data.get("severity").and_then(|v| v.as_str()) == Some("warning")
        }));
    }

    #[tokio::test]
    async fn windows_stub_returns_unsupported() {
        let runner = WindowsGdbServerRunner;
        let result = runner
            .launch(ArtifactId::new(), HashMap::new(), PathBuf::from("/tmp"))
            .await;
        assert!(matches!(result, Err(RunnerError::Unsupported)));
    }
}
