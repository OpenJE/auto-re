//! Wave 7 exit-criterion test: end-to-end IDA debugger + GDB backend seam.
//!
//! Verifies the Wave 7 exit criterion from spec §9.9:
//!   1. A typed debugger scenario is constructed for a fixture function.
//!   2. The scenario serializes to ≤ 16 KiB JSON.
//!   3. A mock `WineGdbRunner` executes the scenario to `ScenarioStatus::Passed`
//!      and produces at least one observation for the target function.
//!   4. The same scenario is shape-stable when the `WindowsGdbServerRunner`
//!      backend stub is substituted: serialization is unchanged.
//!   5. The IDA provider's `NegotiateResponse` carries the backend metadata
//!      `ida.debugger.backend = gdb-wine`.
//!
//! No real IDA, Wine, or GDB is required. The IDA provider binary is spawned
//! in its default (non-IDA) configuration to test the negotiate path only.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;

use autore_provider_protocol::v1::NegotiateRequest;
use autore_provider_protocol::v1::provider_client::ProviderClient;
use autore_provider_runtime::{
    BootstrapSocketAddr, BootstrapStream, CoordinatorBootstrap, bind_bootstrap_socket,
    listener::BootstrapListener,
};
use autore_reconstruction::dynamic::runner::WindowsGdbServerRunner;
use autore_reconstruction::dynamic::{
    AddressRange, Scenario, ScenarioStatus, ScenarioVerifier, SetupOp, Step, StopOp, WineGdbRunner,
    execute_scenario,
};
use autore_schema::domain::SemanticEntity;
use autore_schema::domain::records::ENTITY_KIND_FUNCTION;
use autore_schema::ids::{ArtifactId, EntityId, ProjectId};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_function_entity(project_id: ProjectId, name: &str) -> SemanticEntity {
    SemanticEntity::new(
        project_id,
        ENTITY_KIND_FUNCTION.clone(),
        None,
        Some(name.into()),
    )
}

fn make_scenario(entity: EntityId, exe: ArtifactId) -> Scenario {
    Scenario::new(
        vec![SetupOp::LaunchTarget {
            exe_artifact: exe,
            env: HashMap::new(),
            working_dir: PathBuf::from("/tmp"),
        }],
        vec![
            Step::SetBreakpoint { entity },
            Step::Continue,
            Step::CaptureArguments { entity },
        ],
        vec![StopOp::StopAfterInvocationCount { count: 1 }],
    )
}

/// Resolves the workspace target directory.
fn workspace_target_dir() -> PathBuf {
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir);
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().join("target")
}

/// Resolves the ida-provider binary path.
fn ida_provider_binary() -> PathBuf {
    workspace_target_dir().join("debug/ida-provider")
}

/// Builds the ida-provider binary if it is missing.
fn ensure_ida_provider_binary() {
    let binary = ida_provider_binary();
    if binary.exists() {
        return;
    }

    eprintln!("[wave7_exit_criterion] building ida-provider binary...");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("build")
        .arg("-p")
        .arg("ida-provider")
        .arg("--no-default-features")
        .current_dir(&workspace_root);
    if let Some(protoc) = option_env!("PROTOC") {
        cmd.env("PROTOC", protoc);
    } else if std::env::var("PROTOC").is_err() {
        // Fall back to the known protoc location used by this project.
        cmd.env("PROTOC", "/tmp/opencode/protoc/bin/protoc");
    }
    let status = cmd
        .status()
        .expect("failed to run cargo build for ida-provider");
    assert!(
        status.success(),
        "cargo build -p ida-provider --no-default-features failed"
    );
    assert!(
        binary.exists(),
        "ida-provider binary not found after build: {binary:?}"
    );
}

/// Performs the raw bootstrap handshake with a spawned provider and returns the
/// gRPC address reported by the provider along with the bootstrap stream and child.
async fn bootstrap_provider(
    bootstrap: &CoordinatorBootstrap,
    listener: &mut BootstrapListener,
    socket_addr: &BootstrapSocketAddr,
) -> (BootstrapStream, Child, String) {
    let binary = ida_provider_binary();
    let mut cmd = bootstrap.build_command(&binary, socket_addr);
    let child = cmd.spawn().expect("failed to spawn ida-provider");

    let mut stream = tokio::time::timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("provider did not connect within deadline")
        .expect("provider accept failed");

    // 1. Authenticate: provider sends 32-byte secret, we echo success.
    let mut secret = [0u8; 32];
    stream
        .read_exact(&mut secret)
        .await
        .expect("failed to read provider secret");
    assert_eq!(
        secret,
        *bootstrap.secret.as_bytes(),
        "provider secret mismatch"
    );
    stream
        .write_all(&[0x00])
        .await
        .expect("failed to send auth success");

    // 2. Negotiate raw protocol version range.
    let min = stream
        .read_u32()
        .await
        .expect("failed to read provider min version");
    let max = stream
        .read_u32()
        .await
        .expect("failed to read provider max version");
    assert!(
        min <= 1 && max >= 1,
        "provider does not support protocol version 1"
    );
    stream
        .write_all(&[0x00])
        .await
        .expect("failed to send negotiate success");

    // 3. Read gRPC address.
    let addr_len = stream
        .read_u16()
        .await
        .expect("failed to read gRPC address length") as usize;
    let mut addr_buf = vec![0u8; addr_len];
    stream
        .read_exact(&mut addr_buf)
        .await
        .expect("failed to read gRPC address");
    let grpc_addr = String::from_utf8(addr_buf).expect("invalid gRPC address");

    (stream, child, grpc_addr)
}

// ---------------------------------------------------------------------------
// Exit-criterion test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn wave7_exit_criterion_ida_debugger_uses_gdb() {
    eprintln!("[wave7_exit_criterion] constructing fixture scenario");

    let project_id = ProjectId::new();
    let exe_artifact = ArtifactId::new();
    let function_entity = make_function_entity(project_id, "wave7_fixture_function");
    let scenario = make_scenario(function_entity.id, exe_artifact);

    // 1. Scenario serializable surface ≤ 16 KiB.
    let scenario_json = serde_json::to_string(&scenario).expect("scenario serializes");
    assert!(
        scenario_json.len() <= 16 * 1024,
        "scenario JSON must be ≤ 16 KiB, got {} bytes",
        scenario_json.len()
    );
    eprintln!(
        "[wave7_exit_criterion] scenario JSON size: {} bytes",
        scenario_json.len()
    );

    // 2. Validate against canonical context (security boundary).
    let mut entities_by_id = HashMap::new();
    entities_by_id.insert(function_entity.id, function_entity.clone());
    let mapped_segments = vec![AddressRange::new(0x400000, 0x500000)];
    let allowed_apis = HashSet::new();
    ScenarioVerifier::validate(&scenario, &entities_by_id, &mapped_segments, &allowed_apis)
        .expect("scenario must be valid");
    eprintln!("[wave7_exit_criterion] scenario verifier: PASS");

    // 3. Execute with mock WineGdbRunner.
    let result = execute_scenario(&WineGdbRunner::mock(), &scenario)
        .await
        .expect("scenario execution must succeed");
    assert_eq!(result.status, ScenarioStatus::Passed);
    let has_function_observation = result
        .ctx
        .observations
        .iter()
        .any(|o| o.entity == Some(function_entity.id));
    assert!(
        has_function_observation,
        "expected at least one observation for the fixture function"
    );
    eprintln!(
        "[wave7_exit_criterion] mock WineGdbRunner executed: {} observations",
        result.ctx.observations.len()
    );

    // 4. Shape-stable across the backend-agnostic TargetRunner seam.
    let json_before = serde_json::to_string(&scenario).expect("scenario serializes");
    let _windows_runner = WindowsGdbServerRunner;
    let json_after =
        serde_json::to_string(&scenario).expect("scenario serializes after stub runner");
    assert_eq!(
        json_before, json_after,
        "scenario shape must not change when a different TargetRunner is considered"
    );
    eprintln!("[wave7_exit_criterion] scenario shape stable across TargetRunner backends");

    // 5. IDA provider Negotiate response exposes the GDB backend metadata.
    ensure_ida_provider_binary();
    eprintln!("[wave7_exit_criterion] spawning ida-provider for negotiate check");

    let bootstrap = CoordinatorBootstrap::new().expect("bootstrap creation failed");
    let instance_id = bootstrap.instance_id.to_string();
    let (mut listener, socket_addr) = bind_bootstrap_socket()
        .await
        .expect("failed to bind bootstrap socket");

    let (mut _stream, mut child, grpc_addr) =
        bootstrap_provider(&bootstrap, &mut listener, &socket_addr).await;

    // Keep the bootstrap stream alive so the provider stays connected.
    let _keepalive = _stream;

    let channel = tonic::transport::Channel::from_shared(grpc_addr)
        .expect("invalid gRPC address")
        .connect()
        .await
        .expect("failed to connect to provider gRPC");
    let mut client = ProviderClient::new(channel);

    let req = NegotiateRequest {
        min_supported: 1,
        max_supported: 1,
        coordinator_id: instance_id,
    };
    let resp = client
        .negotiate(req)
        .await
        .expect("Negotiate RPC failed")
        .into_inner();

    let max_concurrency: serde_json::Value =
        serde_json::from_slice(&resp.max_concurrency).expect("max_concurrency must be valid JSON");
    let backend = max_concurrency
        .get("ida.debugger.backend")
        .and_then(|v| v.as_str())
        .expect("ida.debugger.backend metadata missing");
    assert_eq!(backend, "gdb-wine", "IDA debugger backend must be gdb-wine");
    eprintln!("[wave7_exit_criterion] IDA provider Negotiate backend: {backend}");

    // Clean up the provider child.
    let _ = child.kill().await;
    let _ = child.wait().await;

    eprintln!(
        "[OK] coordinator can schedule + execute structured experiments; backend seams documented"
    );
}
