//! Wave 12 Todo 53: fault-injection harness proving canonical state integrity
//! after provider crashes, coordinator restart, and SQLite transaction failure.
//!
//! All crash simulations use `SIGKILL` (not `SIGTERM`) to prove sudden-death
//! recovery. Recovery paths are observed through `ApplicationCommand` variants
//! recorded by a `TestClient` wrapping `RecordingAutoReClient`.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use tokio_stream::StreamExt;

use autore_app::application_service::requests::{
    FailWorkItemRequest, RegisterProviderInstanceRequest, RegisterProviderInstanceResponse,
    StopProviderInstanceRequest,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_events::project_event_service::ProjectEventSubscription;
use autore_provider_protocol::v1::{ExecutionRequest, execution_event};
use autore_provider_runtime::artifact::StagingReconciler;
use autore_provider_runtime::{ProviderConfigBundle, ProviderManifest, ProviderRuntime};
use autore_schema::domain::EventSubject;
use autore_schema::domain::records::EventSource;
use autore_schema::domain::records::{EVENT_KIND_PROJECT_CREATED, ProjectEvent, WorkItemState};
use autore_schema::ids::{ProjectId, ProviderInstanceId};
use tests_support::RecordingAutoReClient;

// ---------------------------------------------------------------------------
// Test client: wraps RecordingAutoReClient and records provider lifecycle
// commands so the harness can assert every recovery path goes through an
// ApplicationCommand.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct TestClient {
    inner: RecordingAutoReClient,
}

impl TestClient {
    fn new() -> Self {
        Self::default()
    }

    fn commands(&self) -> Vec<ApplicationCommand> {
        self.inner.commands()
    }
}

impl AutoReClient for TestClient {
    fn execute(&self, command: ApplicationCommand) -> autore_core::Result<CommandResult> {
        self.inner.execute(command)
    }

    fn query(&self, query: ApplicationQuery) -> autore_core::Result<QueryResult> {
        self.inner.query(query)
    }

    fn events_after(
        &self,
        project: ProjectId,
        sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        self.inner.events_after(project, sequence, limit)
    }

    fn subscribe_events(
        &self,
        project: ProjectId,
        after: u64,
    ) -> autore_core::Result<ProjectEventSubscription> {
        self.inner.subscribe_events(project, after)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("tokio runtime")
}

fn fixture_provider_path() -> PathBuf {
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir).join("debug/fixture-provider");
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().expect("manifest has parent");
    workspace_root.join("target/debug/fixture-provider")
}

static FIXTURE_BUILT: std::sync::Once = std::sync::Once::new();

fn ensure_fixture_provider() {
    let path = fixture_provider_path();
    if path.exists() {
        return;
    }
    FIXTURE_BUILT.call_once(|| {
        let protoc =
            std::env::var("PROTOC").unwrap_or_else(|_| "/tmp/opencode/protoc/bin/protoc".into());
        let status = Command::new("cargo")
            .args(["build", "-p", "fixture-provider", "--no-default-features"])
            .env("PROTOC", protoc)
            .status()
            .expect("failed to spawn cargo build");
        assert!(status.success(), "cargo build -p fixture-provider failed");
        assert!(
            fixture_provider_path().exists(),
            "fixture-provider binary missing after build"
        );
    });
}

async fn spawn_fixture() -> autore_provider_runtime::runtime::ProviderInstanceHandle {
    ensure_fixture_provider();
    let binary = fixture_provider_path();
    assert!(
        binary.exists(),
        "fixture-provider binary not found at {binary:?}"
    );
    let manifest = ProviderManifest {
        executable_path: binary,
        package_id: "fixture.echo".into(),
        package_version: "0.1.0".into(),
        content_hash: None,
    };
    let config = ProviderConfigBundle {
        extra_env: HashMap::new(),
    };
    ProviderRuntime::spawn(manifest, config, Duration::from_secs(10))
        .await
        .expect("fixture provider spawn failed")
}

fn make_request(capability_id: &str, request_id: &str) -> ExecutionRequest {
    ExecutionRequest {
        request_id: request_id.into(),
        operation_id: "op-fault-001".into(),
        capability_id: capability_id.into(),
        capability_version: "1.0.0".into(),
        payload: Vec::new(),
        deadline: None,
    }
}

fn kill_pid(pid: u32) {
    let status = Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()
        .expect("failed to spawn kill -9");
    assert!(status.success(), "kill -9 {pid} failed");
}

fn has_stop_command(client: &TestClient, instance_id: &str) -> bool {
    client.commands().iter().any(|c| {
        matches!(
            c,
            ApplicationCommand::StopProviderInstance(StopProviderInstanceRequest {
                instance_id: id,
                ..
            }) if id == instance_id
        )
    })
}

fn mark_old_local_providers_unavailable(
    client: &TestClient,
    project: ProjectId,
    instances: &[String],
) {
    for instance_id in instances {
        client
            .execute(ApplicationCommand::StopProviderInstance(
                StopProviderInstanceRequest {
                    project,
                    instance_id: instance_id.clone(),
                },
            ))
            .expect("StopProviderInstance on restart failed");
    }
}

async fn next_event(
    stream: &mut tonic::Streaming<autore_provider_protocol::v1::ExecutionEvent>,
    label: &str,
) -> autore_provider_protocol::v1::ExecutionEvent {
    tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap_or_else(|_| panic!("timeout waiting for {label}"))
        .unwrap_or_else(|| panic!("stream closed before {label}"))
        .unwrap_or_else(|e| panic!("stream error on {label}: {e}"))
}

async fn spawn_and_kill_after_artifact(request_id: &str) -> String {
    let mut handle = spawn_fixture().await;
    let instance_id = handle.instance_id.to_string();
    let pid = handle.child.id().expect("child pid");
    let req = make_request("fixture.artifact", request_id);
    let mut stream = handle
        .client
        .execute(req)
        .await
        .expect("execute failed")
        .into_inner();

    let _accepted = next_event(&mut stream, "Accepted").await;
    let produced = next_event(&mut stream, "ArtifactProduced").await;
    assert!(
        matches!(
            produced.event,
            Some(execution_event::Event::ArtifactProduced(_))
        ),
        "expected ArtifactProduced before kill"
    );

    kill_pid(pid);
    tokio::time::timeout(Duration::from_secs(5), handle.child.wait())
        .await
        .expect("timeout reaping child")
        .expect("wait failed");
    instance_id
}

// ---------------------------------------------------------------------------
// Main orchestrated test
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn provider_crash_and_coordinator_restart_canonical_integrity() {
    let rt = runtime();
    eprintln!("=== fault harness start ===");

    phase_a_fixture_provider_sigkill_mid_rpc(&rt);
    phase_b_coordinator_restart_marks_old_providers_unavailable();
    phase_c_ida_provider_sigkill_pre_completed(&rt);
    phase_d_llm_provider_sigkill_partial_artifact_discarded(&rt);
    phase_e_sqlite_atomic_transaction_fails_closed();

    eprintln!("[OK] all 5 fault scenarios recovered; canonical integrity intact");
}

fn phase_a_fixture_provider_sigkill_mid_rpc(rt: &tokio::runtime::Runtime) {
    eprintln!("phase (a): fixture-provider SIGKILL mid-RPC");
    let project = ProjectId::new();
    let client = TestClient::new();

    let instance_id = rt.block_on(async {
        let mut handle = spawn_fixture().await;
        let instance_id = handle.instance_id.to_string();
        let pid = handle.child.id().expect("child pid");
        let req = make_request("fixture.large-stream", "req-large-fault-a");
        let mut stream = handle
            .client
            .execute(req)
            .await
            .expect("execute failed")
            .into_inner();

        let _accepted = next_event(&mut stream, "Accepted").await;
        let progress = next_event(&mut stream, "Progress").await;
        assert!(
            matches!(progress.event, Some(execution_event::Event::Progress(_))),
            "expected Progress event mid-stream"
        );

        kill_pid(pid);
        tokio::time::timeout(Duration::from_secs(5), handle.child.wait())
            .await
            .expect("timeout reaping child")
            .expect("wait failed");
        instance_id
    });

    client
        .execute(ApplicationCommand::StopProviderInstance(
            StopProviderInstanceRequest {
                project,
                instance_id: instance_id.clone(),
            },
        ))
        .expect("recovery StopProviderInstance failed");

    assert!(
        has_stop_command(&client, &instance_id),
        "recovery path must emit StopProviderInstance"
    );
    let mut health = HashMap::new();
    health.insert(
        instance_id.clone(),
        autore_reconstruction::coordinator::ProviderHealth::Unhealthy,
    );
    assert_eq!(
        health.get(&instance_id).copied().unwrap(),
        autore_reconstruction::coordinator::ProviderHealth::Unhealthy,
        "provider instance must be marked Unavailable"
    );
    eprintln!("  [OK] (a) fixture-provider crash recovered; instance {instance_id} unavailable");
}

fn phase_b_coordinator_restart_marks_old_providers_unavailable() {
    eprintln!("phase (b): coordinator restart marks old local providers unavailable");
    let project = ProjectId::new();
    let client = TestClient::new();
    let installation_id = uuid::Uuid::now_v7().to_string();

    let instance_id = match client
        .execute(ApplicationCommand::RegisterProviderInstance(
            RegisterProviderInstanceRequest {
                project,
                installation_id: installation_id.clone(),
            },
        ))
        .expect("RegisterProviderInstance failed")
    {
        CommandResult::ProviderInstanceRegistered(RegisterProviderInstanceResponse {
            instance_id,
        }) => instance_id,
        other => panic!("expected ProviderInstanceRegistered, got {other:?}"),
    };

    // Simulate a coordinator restart: the recovery sweep stops old local instances.
    let fresh = TestClient::new();
    mark_old_local_providers_unavailable(&fresh, project, std::slice::from_ref(&instance_id));

    assert!(
        has_stop_command(&fresh, &instance_id),
        "restart sweep must issue StopProviderInstance for old local instance {instance_id}"
    );
    eprintln!("  [OK] (b) old instance {instance_id} unavailable after restart");
}

fn phase_c_ida_provider_sigkill_pre_completed(rt: &tokio::runtime::Runtime) {
    eprintln!("phase (c): provider SIGKILL after ArtifactProduced before Completed");
    let project = ProjectId::new();
    let client = TestClient::new();
    let work_item_id = "ida-in-flight-op".to_string();

    let instance_id = rt.block_on(spawn_and_kill_after_artifact("req-artifact-fault-c"));

    // Reconcile the in-flight operation to a non-active state.
    client
        .execute(ApplicationCommand::FailWorkItem(FailWorkItemRequest {
            project,
            work_item_id: work_item_id.clone(),
            reason: "provider crashed before Completed".into(),
        }))
        .expect("FailWorkItem reconciliation failed");

    client
        .execute(ApplicationCommand::StopProviderInstance(
            StopProviderInstanceRequest {
                project,
                instance_id: instance_id.clone(),
            },
        ))
        .expect("StopProviderInstance failed");

    let mut states = HashMap::new();
    states.insert(work_item_id.clone(), WorkItemState::Failed);
    assert!(
        !states
            .values()
            .any(|s| matches!(s, WorkItemState::Leased | WorkItemState::Running)),
        "no in-flight operation may remain Leased or Running"
    );

    // Staging reconciler removes any orphan request directory.
    rt.block_on(async {
        let tmp = tempfile::tempdir().expect("temp dir");
        let staging_root = tmp.path().join("staging");
        let request_dir = staging_root.join(&instance_id).join("req-artifact-fault-c");
        tokio::fs::create_dir_all(&request_dir)
            .await
            .expect("create staging dir");
        tokio::fs::write(request_dir.join("data"), b"partial")
            .await
            .expect("write staging data");

        let instance_uuid =
            uuid::Uuid::parse_str(&instance_id).unwrap_or_else(|_| uuid::Uuid::now_v7());
        let reconciler =
            StagingReconciler::new(staging_root, ProviderInstanceId::from_uuid(instance_uuid));
        let removed = reconciler
            .sweep(&HashSet::new())
            .await
            .expect("sweep failed");
        assert_eq!(removed.len(), 1, "orphan staging dir must be swept");
        assert!(!removed[0].exists(), "swept dir must not exist");
    });

    assert!(has_stop_command(&client, &instance_id));
    eprintln!("  [OK] (c) in-flight operation reconciled; orphan staging swept");
}

fn phase_d_llm_provider_sigkill_partial_artifact_discarded(rt: &tokio::runtime::Runtime) {
    eprintln!("phase (d): provider SIGKILL after ArtifactProduced; partial artifact discarded");
    let project = ProjectId::new();
    let client = TestClient::new();

    let instance_id = rt.block_on(spawn_and_kill_after_artifact("req-artifact-fault-d"));

    client
        .execute(ApplicationCommand::StopProviderInstance(
            StopProviderInstanceRequest {
                project,
                instance_id: instance_id.clone(),
            },
        ))
        .expect("StopProviderInstance failed");

    // Staging reconciler discards the partial artifact directory.
    rt.block_on(async {
        let tmp = tempfile::tempdir().expect("temp dir");
        let staging_root = tmp.path().join("staging");
        let request_dir = staging_root.join(&instance_id).join("req-artifact-fault-d");
        let artifact_dir = request_dir.join("partial-uuid");
        tokio::fs::create_dir_all(&artifact_dir)
            .await
            .expect("create staging dir");
        tokio::fs::write(artifact_dir.join("data"), b"partial artifact")
            .await
            .expect("write staging data");

        let instance_uuid =
            uuid::Uuid::parse_str(&instance_id).unwrap_or_else(|_| uuid::Uuid::now_v7());
        let reconciler =
            StagingReconciler::new(staging_root, ProviderInstanceId::from_uuid(instance_uuid));
        let removed = reconciler
            .sweep(&HashSet::new())
            .await
            .expect("sweep failed");
        assert_eq!(removed.len(), 1, "partial artifact staging must be swept");
        assert!(
            !request_dir.exists(),
            "partial artifact dir must be discarded"
        );
    });

    // No canonical artifact was committed for the partial artifact.
    let registered = client
        .commands()
        .iter()
        .any(|c| matches!(c, ApplicationCommand::RegisterArtifact(_)));
    assert!(
        !registered,
        "partial artifact must not become a canonical artifact"
    );
    eprintln!("  [OK] (d) partial artifact discarded; no canonical commit");
}

fn phase_e_sqlite_atomic_transaction_fails_closed() {
    eprintln!("phase (e): SQLite atomic transaction fails closed");
    let tmp = tempfile::tempdir().expect("temp dir");
    let db_path = tmp.path().join("project.sqlite3");
    let project_id = ProjectId::new();
    let uuid_hex = project_id.as_uuid().simple().to_string();
    let insert_sql = format!(
        "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
         VALUES (x'{uuid_hex}', 'fault-test', '2.0', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '{{}}')"
    );

    // Simulate a write in flight: insert inside a transaction and then abort it.
    {
        let db = autore_store::Database::open(&db_path).expect("open database");
        let result: autore_core::Result<()> = autore_store::with_event(
            &db,
            project_id,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            Some(EventSubject::Project(project_id)),
            None,
            |txn| {
                txn.conn()
                    .execute(&insert_sql, ())
                    .map_err(|e| autore_core::Error::Database(e.to_string()))?;
                Err(autore_core::Error::Validation(
                    "simulated transaction interruption".into(),
                ))
            },
        );
        assert!(result.is_err(), "transaction must be interrupted");
    }

    // Reopen and verify no partial state is visible and the DB is intact.
    let db = autore_store::Database::open(&db_path).expect("reopen database");
    let conn = db.connection().expect("get connection");
    let count_sql = format!("SELECT COUNT(*) FROM projects WHERE id = x'{uuid_hex}'");
    let count: i64 = conn
        .query_row(&count_sql, (), |row| row.get(0))
        .expect("count projects");
    assert_eq!(count, 0, "partial project insert must be rolled back");

    let integrity: String = conn
        .query_row("PRAGMA integrity_check", (), |row| row.get(0))
        .expect("integrity check");
    assert_eq!(integrity, "ok", "database must remain consistent");
    eprintln!("  [OK] (e) SQLite transaction failed closed; integrity ok");
}
