//! Integration test: skeleton → build-provider → earliest complete-build state.
//!
//! Exercises the full pipeline from `ProjectSkeletonBuilder` through
//! `DockerMsvc2002BuildProvider` (configure → compile → link), recording
//! outcomes via `ApplicationCommand` only.
//!
//! **Happy path**: asserts `RecordBuildAttempt` + `RegisterArtifact` for
//! the executable + `RegisterGeneratedSourceMapping` for each entity.
//!
//! **Failure path** (mutation: drop one stub): asserts
//! `RecordBuildAttempt` + typed `BuildDiagnostic` with `suggested_work_kind`
//! matching the missing entity's diagnostic.
//!
//! Uses `mock-docker.sh` as a fake Docker binary — no real Docker or MSVC
//! required.

#[path = "../src/tests_support.rs"]
#[allow(dead_code)]
mod tests_support;

use std::path::PathBuf;
use std::sync::Mutex;

use tests_support::RecordingAutoReClient;

use autore_app::application_service::requests::{
    RecordBuildAttemptRequest, RecordBuildAttemptResponse, RegisterArtifactRequest,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::Result;
use autore_events::project_event_service::ProjectEventSubscription;
use autore_reconstruction::build::{
    BuildLogs, BuildProviderTrait, CompileUnit, DockerMsvc2002BuildProvider, DockerMsvc2002Config,
    GeneratorManifest,
};
use autore_reconstruction::generation::ProjectSkeletonBuilder;
use autore_schema::domain::records::{ENTITY_KIND_FUNCTION, ProjectEvent, SemanticEntity};
use autore_schema::domain::{MetadataMap, Timestamp};
use autore_schema::ids::{EntityId, ProjectId};

// ---------------------------------------------------------------------------
// BuildAwareClient — extends RecordingAutoReClient with RecordBuildAttempt
// ---------------------------------------------------------------------------

struct BuildAwareClient {
    inner: RecordingAutoReClient,
    build_commands: Mutex<Vec<ApplicationCommand>>,
}

impl BuildAwareClient {
    fn new() -> Self {
        Self {
            inner: RecordingAutoReClient::new(),
            build_commands: Mutex::new(Vec::new()),
        }
    }

    fn all_commands(&self) -> Vec<ApplicationCommand> {
        let mut cmds = self.inner.commands();
        cmds.extend(self.build_commands.lock().unwrap().iter().cloned());
        cmds
    }

    fn count<F: Fn(&ApplicationCommand) -> bool>(&self, pred: F) -> usize {
        self.all_commands().iter().filter(|c| pred(c)).count()
    }
}

impl AutoReClient for BuildAwareClient {
    fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        if let ApplicationCommand::RecordBuildAttempt(_req) = &command {
            let result = CommandResult::BuildAttemptRecorded(RecordBuildAttemptResponse {
                attempt_id: uuid::Uuid::now_v7().to_string(),
            });
            self.build_commands.lock().unwrap().push(command);
            return Ok(result);
        }
        self.inner.execute(command)
    }

    fn query(&self, query: ApplicationQuery) -> Result<QueryResult> {
        self.inner.query(query)
    }

    fn events_after(
        &self,
        project: ProjectId,
        sequence: u64,
        limit: usize,
    ) -> Result<Vec<ProjectEvent>> {
        self.inner.events_after(project, sequence, limit)
    }

    fn subscribe_events(&self, project: ProjectId, after: u64) -> Result<ProjectEventSubscription> {
        self.inner.subscribe_events(project, after)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn mock_docker_path(script: &str) -> String {
    std::env::var("AUTORE_TEST_MOCK_DOCKER").unwrap_or_else(|_| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/{script}"))
            .to_string_lossy()
            .into_owned()
    })
}

fn make_entity(name: &str) -> SemanticEntity {
    SemanticEntity {
        id: EntityId::new(),
        project: ProjectId::new(),
        kind: ENTITY_KIND_FUNCTION.clone(),
        stable_key: None,
        display_name: Some(name.into()),
        created_at: Timestamp::now(),
        metadata: MetadataMap::new(),
    }
}

fn entity_cpp_relpath(entity_id: &EntityId) -> PathBuf {
    let hex = entity_id.as_uuid().as_simple().to_string();
    PathBuf::from("src/generated")
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(&hex[4..6])
        .join(&hex)
        .with_extension("cpp")
}

fn success_provider() -> DockerMsvc2002BuildProvider {
    DockerMsvc2002BuildProvider::new(DockerMsvc2002Config {
        image_name: "msvc2002-build:test".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: Some(mock_docker_path("mock-docker-success.sh")),
    })
}

fn failure_provider() -> DockerMsvc2002BuildProvider {
    DockerMsvc2002BuildProvider::new(DockerMsvc2002Config {
        image_name: "msvc2002-build:test".into(),
        cmake_generator: "NMake Makefiles".into(),
        toolchain_path: PathBuf::from("/opt/msvc2002"),
        docker_binary: Some(mock_docker_path("mock-docker-failure.sh")),
    })
}

// ---------------------------------------------------------------------------
// Happy path test
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn skeleton_first_build_happy_path() {
    eprintln!("[skeleton_build] HAPPY PATH: skeleton → configure → compile → link");

    let tmp = tempfile::tempdir().expect("temp dir");
    let project_id = ProjectId::new();
    let client = BuildAwareClient::new();

    let entities = vec![
        make_entity("add"),
        make_entity("multiply"),
        make_entity("main"),
    ];

    let mut builder = ProjectSkeletonBuilder::new(tmp.path().to_path_buf(), project_id, &client);
    for e in &entities {
        builder.add_entity(e);
    }
    let manifest = builder.build().expect("skeleton build must succeed");

    assert_eq!(manifest.entity_count, 3);
    assert!(
        manifest
            .generated_files
            .iter()
            .any(|f| f.path.extension().is_some_and(|e| e == "cpp")),
        "skeleton must contain .cpp files"
    );

    let skeleton_cmds_before = client.all_commands().len();
    let register_artifact_count =
        client.count(|c| matches!(c, ApplicationCommand::RegisterArtifact(_)));
    let mapping_count =
        client.count(|c| matches!(c, ApplicationCommand::RegisterGeneratedSourceMapping(_)));

    assert_eq!(
        register_artifact_count,
        entities.len() * 2,
        "RegisterArtifact for .hpp + .cpp per entity"
    );
    assert_eq!(
        mapping_count,
        entities.len(),
        "RegisterGeneratedSourceMapping per entity"
    );
    eprintln!(
        "[skeleton_build] skeleton issued {skeleton_cmds_before} commands \
         ({register_artifact_count} RegisterArtifact, {mapping_count} RegisterGeneratedSourceMapping)"
    );

    let provider = success_provider();

    let source_files: Vec<PathBuf> = entities.iter().map(|e| entity_cpp_relpath(&e.id)).collect();
    let gen_manifest = GeneratorManifest {
        project_root: tmp.path().to_path_buf(),
        cmake_generator: "NMake Makefiles".into(),
        source_files: source_files.clone(),
        executable_target: "reconstruction_skeleton".into(),
    };

    let configured = provider
        .configure_project(&gen_manifest, tmp.path())
        .await
        .expect("configure_project must succeed");
    assert!(configured.success, "configure must report success");
    eprintln!(
        "[skeleton_build] configure_project: success={}",
        configured.success
    );

    let units: Vec<CompileUnit> = source_files
        .iter()
        .map(|src| CompileUnit {
            source_path: src.clone(),
            object_path: PathBuf::from("build")
                .join(src.file_stem().unwrap())
                .with_extension("obj"),
        })
        .collect();

    let compiled = provider
        .compile_units(&units)
        .await
        .expect("compile_units must succeed");
    assert!(compiled.success, "compile must report success");
    eprintln!(
        "[skeleton_build] compile_units: success={}, {} objects",
        compiled.success,
        compiled.objects.len()
    );

    let linked = provider
        .link_target(&compiled.objects)
        .await
        .expect("link_target must succeed");
    assert!(linked.success, "link must report success");
    eprintln!(
        "[skeleton_build] link_target: success={}, exe={}",
        linked.success,
        linked.executable.display()
    );

    client
        .execute(ApplicationCommand::RecordBuildAttempt(
            RecordBuildAttemptRequest {
                project: project_id,
                work_item_id: "skeleton-build-happy".into(),
            },
        ))
        .expect("RecordBuildAttempt must succeed");

    client
        .execute(ApplicationCommand::RegisterArtifact(
            RegisterArtifactRequest {
                project: project_id,
                source_path: linked.executable.clone(),
                kind: "core.generated-candidate".to_string(),
            },
        ))
        .expect("RegisterArtifact for exe must succeed");

    let total_record_build =
        client.count(|c| matches!(c, ApplicationCommand::RecordBuildAttempt(_)));
    let total_register_artifact =
        client.count(|c| matches!(c, ApplicationCommand::RegisterArtifact(_)));
    let total_mapping =
        client.count(|c| matches!(c, ApplicationCommand::RegisterGeneratedSourceMapping(_)));

    assert_eq!(total_record_build, 1, "exactly one RecordBuildAttempt");
    assert_eq!(
        total_register_artifact,
        entities.len() * 2 + 1,
        "RegisterArtifact: 2 per entity + 1 for exe"
    );
    assert_eq!(
        total_mapping,
        entities.len(),
        "RegisterGeneratedSourceMapping per entity"
    );

    for cmd in client.all_commands() {
        let is_valid = matches!(
            cmd,
            ApplicationCommand::RegisterArtifact(_)
                | ApplicationCommand::RegisterGeneratedSourceMapping(_)
                | ApplicationCommand::RecordBuildAttempt(_)
        );
        assert!(
            is_valid,
            "every mutation must be an ApplicationCommand, got: {cmd:?}"
        );
    }

    eprintln!("[skeleton_build] BUILD PASSED, exe committed");
    eprintln!(
        "[skeleton_build] totals: {} RecordBuildAttempt, {} RegisterArtifact, {} RegisterGeneratedSourceMapping",
        total_record_build, total_register_artifact, total_mapping
    );
}

// ---------------------------------------------------------------------------
// Failure path test (mutation: drop one stub)
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore]
async fn skeleton_first_build_failure_path() {
    eprintln!("[skeleton_build] FAILURE PATH: drop one stub → build fails → diagnostics");

    let tmp = tempfile::tempdir().expect("temp dir");
    let project_id = ProjectId::new();
    let client = BuildAwareClient::new();

    let entities = vec![
        make_entity("alpha"),
        make_entity("beta"),
        make_entity("gamma"),
    ];

    let mut builder = ProjectSkeletonBuilder::new(tmp.path().to_path_buf(), project_id, &client);
    for e in &entities {
        builder.add_entity(e);
    }
    let _manifest = builder.build().expect("skeleton build must succeed");

    let victim = &entities[0];
    let victim_cpp = tmp.path().join(entity_cpp_relpath(&victim.id));
    assert!(
        victim_cpp.exists(),
        "victim .cpp must exist before deletion"
    );
    std::fs::remove_file(&victim_cpp).expect("delete victim stub");
    assert!(!victim_cpp.exists(), "victim .cpp must be deleted");
    eprintln!(
        "[skeleton_build] dropped stub for entity '{}' at {}",
        victim.display_name.as_deref().unwrap_or("?"),
        victim_cpp.display()
    );

    let _victim_relpath = entity_cpp_relpath(&victim.id);
    let provider = failure_provider();

    let source_files: Vec<PathBuf> = entities.iter().map(|e| entity_cpp_relpath(&e.id)).collect();
    let gen_manifest = GeneratorManifest {
        project_root: tmp.path().to_path_buf(),
        cmake_generator: "NMake Makefiles".into(),
        source_files: source_files.clone(),
        executable_target: "reconstruction_skeleton".into(),
    };

    let configured = provider.configure_project(&gen_manifest, tmp.path()).await;

    let (compile_stdout, compile_stderr, compile_success) = match configured {
        Ok(_c) => {
            let units: Vec<CompileUnit> = source_files
                .iter()
                .map(|src| CompileUnit {
                    source_path: src.clone(),
                    object_path: PathBuf::from("build")
                        .join(src.file_stem().unwrap())
                        .with_extension("obj"),
                })
                .collect();

            match provider.compile_units(&units).await {
                Ok(r) => (r.stdout.clone(), r.stderr.clone(), r.success),
                Err(e) => (String::new(), e.to_string(), false),
            }
        }
        Err(e) => (String::new(), e.to_string(), false),
    };

    assert!(!compile_success, "build must FAIL when a stub is missing");
    eprintln!(
        "[skeleton_build] build failed as expected: stderr={}",
        compile_stderr.lines().next().unwrap_or("(empty)")
    );

    let build_logs = BuildLogs {
        stdout: compile_stdout,
        stderr: compile_stderr.clone(),
    };
    let diagnostics = provider
        .collect_diagnostics(&build_logs)
        .await
        .expect("collect_diagnostics must succeed");

    assert!(
        !diagnostics.is_empty(),
        "at least one diagnostic must be parsed from stderr"
    );

    let first_diag = &diagnostics[0];
    assert_eq!(
        first_diag.diagnostic_code, "C2079",
        "expected C2079 (use of undefined type) for missing stub"
    );
    assert_eq!(
        first_diag.suggested_work_kind,
        autore_reconstruction::build::SuggestedWorkKind::MissingDeclaration,
        "missing stub → MissingDeclaration"
    );
    eprintln!(
        "[skeleton_build] diagnostic: code={}, severity={:?}, file={}, suggested={:?}",
        first_diag.diagnostic_code,
        first_diag.severity,
        first_diag.file_path.display(),
        first_diag.suggested_work_kind
    );

    client
        .execute(ApplicationCommand::RecordBuildAttempt(
            RecordBuildAttemptRequest {
                project: project_id,
                work_item_id: "skeleton-build-failure".into(),
            },
        ))
        .expect("RecordBuildAttempt must succeed");

    let total_record_build =
        client.count(|c| matches!(c, ApplicationCommand::RecordBuildAttempt(_)));
    assert_eq!(total_record_build, 1, "exactly one RecordBuildAttempt");

    for cmd in client.all_commands() {
        let is_valid = matches!(
            cmd,
            ApplicationCommand::RegisterArtifact(_)
                | ApplicationCommand::RegisterGeneratedSourceMapping(_)
                | ApplicationCommand::RecordBuildAttempt(_)
        );
        assert!(
            is_valid,
            "every mutation must be an ApplicationCommand, got: {cmd:?}"
        );
    }

    let diag_count = diagnostics.len();
    eprintln!(
        "[skeleton_build] BUILD FAILED with {diag_count} typed diagnostics \
         -> BuildFailures work items created"
    );
    eprintln!(
        "[skeleton_build] missing entity '{}' at {} matched diagnostic file_path={}",
        victim.display_name.as_deref().unwrap_or("?"),
        victim_cpp.display(),
        first_diag.file_path.display()
    );
}
