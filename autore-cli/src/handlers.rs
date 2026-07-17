//! Command handlers for the auto-re CLI.
//!
//! All handlers route through `LocalAutoReClient` — no direct storage access.
//! The CLI never imports store types or SQL primitives.

use std::path::Path;
use std::sync::Arc;

use autore_app::autore_events::project_event_service::{EventBroadcaster, LocalProjectEventService};
use autore_app::domain::records::HypothesisStatus;
use autore_app::domain::{EvidenceRecord, EvidenceValue, StableEntityKey};
use autore_app::ids::{
    ArtifactId, ContradictionId, EntityId, HypothesisId, OperationId, ProjectId,
    VerificationRecordId,
};
use autore_app::storage::Database;
use autore_app::{
    ApplicationCommand, ApplicationQuery, ApplicationService, AutoReClient, CommandResult,
    CreateProjectRequest, LocalAutoReClient, QueryResult,
};
use autore_schema::manifest::ProjectManifest;

use crate::cli::*;

const PROJECT_DIR_NAME: &str = "project.auto-re";

// ---------------------------------------------------------------------------
// Client builder
// ---------------------------------------------------------------------------

/// Opens an existing project and constructs a `LocalAutoReClient` for it.
///
/// The project directory must contain a `project.auto-re/` subdirectory with
/// `project.toml` and `project.sqlite3`.
fn build_client(project_dir: &Path) -> Result<(LocalAutoReClient, ProjectId), String> {
    let auto_re_dir = project_dir.join(PROJECT_DIR_NAME);
    let manifest_path = auto_re_dir.join("project.toml");

    let manifest =
        ProjectManifest::load(&manifest_path).map_err(|e| format!("failed to load manifest: {e}"))?;
    let project_id = manifest.project.id;

    let database_path = auto_re_dir.join("project.sqlite3");
    let db = Arc::new(
        Database::open(&database_path).map_err(|e| format!("failed to open database: {e}"))?,
    );

    let broadcaster = Arc::new(EventBroadcaster::new());
    let events: Arc<dyn autore_app::autore_events::project_event_service::ProjectEventService + Send + Sync> =
        Arc::new(LocalProjectEventService::new(Arc::clone(&db), broadcaster));

    let service = ApplicationService::new(db, events, project_dir);
    let client = LocalAutoReClient::new(Arc::new(service));
    Ok((client, project_id))
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

fn print_json_with_schema(schema: &str, value: &serde_json::Value) {
    let mut map = serde_json::Map::new();
    map.insert(
        "$schema".to_owned(),
        serde_json::Value::String(format!("auto-re/schema/{schema}/v2.0")),
    );
    if let serde_json::Value::Object(inner) = value {
        for (k, v) in inner {
            map.insert(k.clone(), v.clone());
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default()
    );
}

fn print_list_json_with_schema(schema: &str, key: &str, items: &serde_json::Value) {
    let mut map = serde_json::Map::new();
    map.insert(
        "$schema".to_owned(),
        serde_json::Value::String(format!("auto-re/schema/{schema}/v2.0")),
    );
    map.insert(key.to_owned(), items.clone());
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default()
    );
}

// ---------------------------------------------------------------------------
// Top-level dispatch
// ---------------------------------------------------------------------------

pub fn run(cli: AutoReCli) -> Result<(), String> {
    let project_dir = cli.project_dir;
    match cli.command {
        None => {
            println!("auto-re — reverse-engineering project manager.");
            println!("Use --help for usage information, or run a subcommand.");
            Ok(())
        }
        Some(Commands::Project(args)) => handle_project(&project_dir, args),
        Some(Commands::Artifact(args)) => handle_artifact(&project_dir, args),
        Some(Commands::Entity(args)) => handle_entity(&project_dir, args),
        Some(Commands::Evidence(args)) => handle_evidence(&project_dir, args),
        Some(Commands::Hypothesis(args)) => handle_hypothesis(&project_dir, args),
        Some(Commands::Contradiction(args)) => handle_contradiction(&project_dir, args),
        Some(Commands::Verification(args)) => handle_verification(&project_dir, args),
        Some(Commands::Operation(args)) => handle_operation(&project_dir, args),
        Some(Commands::Events(args)) => handle_events(&project_dir, args),
    }
}

// ---------------------------------------------------------------------------
// Project handlers
// ---------------------------------------------------------------------------

fn handle_project(project_dir: &Path, args: ProjectArgs) -> Result<(), String> {
    match args.command {
        ProjectCommand::Create { name } => {
            if name.is_empty() {
                return Err("project name must not be empty".to_owned());
            }
            let lifecycle_project = autore_app::create_project(project_dir, &name)
                .map_err(|e| format!("failed to create project directory: {e}"))?;
            let (client, _) = build_client(project_dir)?;
            let result = client
                .execute(ApplicationCommand::CreateProject(CreateProjectRequest {
                    name: name.clone(),
                }))
                .map_err(|e| format!("failed to register project in database: {e}"))?;
            if let CommandResult::ProjectCreated(resp) = &result {
                let auto_re_dir = project_dir.join(PROJECT_DIR_NAME);
                let manifest_path = auto_re_dir.join("project.toml");
                let manifest = autore_schema::ProjectManifest::new(
                    resp.project.clone(),
                    manifest_path.clone(),
                );
                let _ = manifest.save(&manifest_path);
            }
            let app_project_id = match &result {
                CommandResult::ProjectCreated(resp) => resp.project.id,
                _ => lifecycle_project.id,
            };
            println!("Project created: {} ({})", name, app_project_id);
            Ok(())
        }
        ProjectCommand::Info { output } => {
            let (client, project_id) = build_client(project_dir)?;
            let result = client
                .query(ApplicationQuery::GetProjectSummary(
                    autore_app::GetProjectSummaryQuery { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::ProjectSummary(resp) => match output {
                    OutputFormat::Json => {
                        let val = serde_json::to_value(&resp.project).map_err(|e| e.to_string())?;
                        print_json_with_schema("project-summary", &val);
                    }
                    OutputFormat::Human => {
                        println!("Project: {}", resp.project.name);
                        println!("  ID:             {}", resp.project.id);
                        println!("  Schema version: {}", resp.project.schema_version);
                        println!("  Created at:     {}", resp.project.created_at);
                        println!("  Updated at:     {}", resp.project.updated_at);
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        ProjectCommand::Validate => {
            let (client, project_id) = build_client(project_dir)?;
            let result = client
                .execute(ApplicationCommand::ValidateProject(
                    autore_app::ValidateProjectRequest { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("project-validated", &result);
            Ok(())
        }
        ProjectCommand::Migrate => {
            let (client, project_id) = build_client(project_dir)?;
            let result = client
                .execute(ApplicationCommand::MigrateProject(
                    autore_app::MigrateProjectRequest { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("project-migrated", &result);
            Ok(())
        }
        ProjectCommand::RebuildIndexes => {
            let (client, project_id) = build_client(project_dir)?;
            let result = client
                .execute(ApplicationCommand::RebuildIndexes(
                    autore_app::RebuildIndexesRequest { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("indexes-rebuilt", &result);
            Ok(())
        }
        ProjectCommand::CheckArtifacts => {
            let (client, project_id) = build_client(project_dir)?;
            let result = client
                .query(ApplicationQuery::ListArtifacts(autore_app::ListArtifactsQuery {
                    project: project_id,
                    offset: 0,
                    limit: 1000,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Artifacts(resp) => {
                    println!(
                        "Artifact integrity check: {} artifact(s) registered.",
                        resp.artifacts.len()
                    );
                    println!("(Full verification not yet implemented — scaffold only.)");
                }
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Artifact handlers
// ---------------------------------------------------------------------------

fn handle_artifact(project_dir: &Path, args: ArtifactArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        ArtifactCommand::Add { file, kind } => {
            let result = client
                .execute(ApplicationCommand::RegisterArtifact(
                    autore_app::RegisterArtifactRequest {
                        project: project_id,
                        source_path: file,
                        kind,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("artifact-registered", &result);
            Ok(())
        }
        ArtifactCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListArtifacts(autore_app::ListArtifactsQuery {
                    project: project_id,
                    offset: 0,
                    limit: 1000,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Artifacts(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.artifacts).map_err(|e| e.to_string())?;
                        print_list_json_with_schema("artifacts", "artifacts", &val);
                    }
                    OutputFormat::Human => {
                        if resp.artifacts.is_empty() {
                            println!("No artifacts registered.");
                        } else {
                            println!("{:<38} {:<30} Size", "ID", "Kind");
                            println!("{}", "-".repeat(80));
                            for a in &resp.artifacts {
                                println!("{:<38} {:<30} {}", a.id, a.kind, a.size);
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        ArtifactCommand::Show { id, output } => {
            let artifact_id: ArtifactId = ArtifactId::from_uuid(
                uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid artifact ID: {e}"))?,
            );
            let result = client
                .query(ApplicationQuery::GetArtifact(autore_app::GetArtifactQuery {
                    id: artifact_id,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Artifact(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.artifact).map_err(|e| e.to_string())?;
                        print_json_with_schema("artifact", &val);
                    }
                    OutputFormat::Human => {
                        println!("Artifact: {}", resp.artifact.id);
                        println!("  Kind:       {}", resp.artifact.kind);
                        println!("  Size:       {} bytes", resp.artifact.size);
                        println!("  Hash:       {}", resp.artifact.content_hash);
                        println!("  Created at: {}", resp.artifact.created_at);
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Entity handlers
// ---------------------------------------------------------------------------

fn handle_entity(project_dir: &Path, args: EntityArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        EntityCommand::Add {
            kind,
            display_name,
            stable_key,
        } => {
            let stable_key: Option<StableEntityKey> = stable_key
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|e| format!("invalid stable key JSON: {e}"))?;
            let result = client
                .execute(ApplicationCommand::RegisterEntity(
                    autore_app::RegisterEntityRequest {
                        project: project_id,
                        kind,
                        stable_key,
                        display_name,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("entity-registered", &result);
            Ok(())
        }
        EntityCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListEntities(autore_app::ListEntitiesQuery {
                    project: project_id,
                    offset: 0,
                    limit: 1000,
                    kind_filter: None,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Entities(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.entities).map_err(|e| e.to_string())?;
                        print_list_json_with_schema("entities", "entities", &val);
                    }
                    OutputFormat::Human => {
                        if resp.entities.is_empty() {
                            println!("No entities registered.");
                        } else {
                            println!("{:<38} {:<30} Display Name", "ID", "Kind");
                            println!("{}", "-".repeat(80));
                            for e in &resp.entities {
                                println!(
                                    "{:<38} {:<30} {}",
                                    e.id,
                                    e.kind,
                                    e.display_name.as_deref().unwrap_or("-")
                                );
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        EntityCommand::Show { id, output } => {
            let entity_id: EntityId = EntityId::from_uuid(
                uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid entity ID: {e}"))?,
            );
            let result = client
                .query(ApplicationQuery::GetEntity(autore_app::GetEntityQuery {
                    id: entity_id,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Entity(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.entity).map_err(|e| e.to_string())?;
                        print_json_with_schema("entity", &val);
                    }
                    OutputFormat::Human => {
                        println!("Entity: {}", resp.entity.id);
                        println!("  Kind:         {}", resp.entity.kind);
                        println!(
                            "  Display name: {}",
                            resp.entity.display_name.as_deref().unwrap_or("-")
                        );
                        println!("  Created at:   {}", resp.entity.created_at);
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Evidence handlers
// ---------------------------------------------------------------------------

fn handle_evidence(project_dir: &Path, args: EvidenceArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        EvidenceCommand::Add { record } => {
            let json_str = std::fs::read_to_string(&record)
                .map_err(|e| format!("failed to read evidence record file: {e}"))?;
            let evidence_record: EvidenceRecord = serde_json::from_str(&json_str)
                .map_err(|e| format!("invalid evidence record JSON: {e}"))?;
            let result = client
                .execute(ApplicationCommand::AddEvidence(
                    autore_app::AddEvidenceRequest {
                        project: project_id,
                        record: evidence_record,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("evidence-added", &result);
            Ok(())
        }
        EvidenceCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListEvidence(autore_app::ListEvidenceQuery {
                    project: project_id,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::EvidenceList(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.records).map_err(|e| e.to_string())?;
                        print_list_json_with_schema("evidence-list", "records", &val);
                    }
                    OutputFormat::Human => {
                        if resp.records.is_empty() {
                            println!("No evidence records.");
                        } else {
                            println!("{:<38} {:<30} Subject", "ID", "Predicate");
                            println!("{}", "-".repeat(80));
                            for r in &resp.records {
                                println!("{:<38} {:<30} {}", r.id, r.predicate, r.subject);
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Hypothesis handlers
// ---------------------------------------------------------------------------

fn handle_hypothesis(project_dir: &Path, args: HypothesisArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        HypothesisCommand::Add {
            subject,
            predicate,
            candidate,
            confidence,
        } => {
            let subject_id = EntityId::from_uuid(
                uuid::Uuid::parse_str(&subject)
                    .map_err(|e| format!("invalid subject entity ID: {e}"))?,
            );
            let candidate_value: EvidenceValue = serde_json::from_str(&candidate)
                .map_err(|e| format!("invalid candidate EvidenceValue JSON: {e}"))?;
            let result = client
                .execute(ApplicationCommand::AddHypothesis(
                    autore_app::AddHypothesisRequest {
                        project: project_id,
                        subject: subject_id,
                        predicate,
                        candidate: candidate_value,
                        confidence_score: confidence,
                        confidence_rationale: None,
                        supporting_evidence: vec![],
                        contradicting_evidence: vec![],
                        derived_from: vec![],
                        status: HypothesisStatus::Proposed,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("hypothesis-added", &result);
            Ok(())
        }
        HypothesisCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListHypotheses(
                    autore_app::ListHypothesesQuery { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Hypotheses(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.hypotheses).map_err(|e| e.to_string())?;
                        print_list_json_with_schema("hypotheses", "hypotheses", &val);
                    }
                    OutputFormat::Human => {
                        if resp.hypotheses.is_empty() {
                            println!("No hypotheses.");
                        } else {
                            println!(
                                "{:<38} {:<30} {:<15} Confidence",
                                "ID", "Predicate", "Status"
                            );
                            println!("{}", "-".repeat(95));
                            for h in &resp.hypotheses {
                                println!(
                                    "{:<38} {:<30} {:<15} {:.2}",
                                    h.id, h.predicate, h.status, h.confidence.score()
                                );
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        HypothesisCommand::Accept { id } => {
            let hypothesis_id = HypothesisId::from_uuid(
                uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid hypothesis ID: {e}"))?,
            );
            let result = client
                .execute(ApplicationCommand::ChangeHypothesisStatus(
                    autore_app::ChangeHypothesisStatusRequest {
                        project: project_id,
                        id: hypothesis_id,
                        status: HypothesisStatus::Accepted,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("hypothesis-accepted", &result);
            Ok(())
        }
        HypothesisCommand::Reject { id } => {
            let hypothesis_id = HypothesisId::from_uuid(
                uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid hypothesis ID: {e}"))?,
            );
            let result = client
                .execute(ApplicationCommand::ChangeHypothesisStatus(
                    autore_app::ChangeHypothesisStatusRequest {
                        project: project_id,
                        id: hypothesis_id,
                        status: HypothesisStatus::Rejected,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("hypothesis-rejected", &result);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Contradiction handlers
// ---------------------------------------------------------------------------

fn handle_contradiction(project_dir: &Path, args: ContradictionArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        ContradictionCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListContradictions(
                    autore_app::ListContradictionsQuery { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Contradictions(resp) => match output {
                    OutputFormat::Json => {
                        let val = serde_json::to_value(&resp.contradictions)
                            .map_err(|e| e.to_string())?;
                        print_list_json_with_schema("contradictions", "contradictions", &val);
                    }
                    OutputFormat::Human => {
                        if resp.contradictions.is_empty() {
                            println!("No contradictions.");
                        } else {
                            println!("{:<38} {:<20} Subject", "ID", "Status");
                            println!("{}", "-".repeat(80));
                            for c in &resp.contradictions {
                                println!(
                                    "{:<38} {:<20} {}",
                                    c.id,
                                    c.status,
                                    c.subject,
                                );
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        ContradictionCommand::Show { id, output } => {
            let cid = ContradictionId::from_uuid(
                uuid::Uuid::parse_str(&id)
                    .map_err(|e| format!("invalid contradiction ID: {e}"))?,
            );
            let result = client
                .query(ApplicationQuery::GetContradiction(
                    autore_app::GetContradictionQuery { id: cid },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Contradiction(resp) => match output {
                    OutputFormat::Json => {
                        let val = serde_json::to_value(&resp.contradiction)
                            .map_err(|e| e.to_string())?;
                        print_json_with_schema("contradiction", &val);
                    }
                    OutputFormat::Human => {
                        println!("Contradiction: {}", resp.contradiction.id);
                        println!("  Status:      {}", resp.contradiction.status);
                        println!("  Subject:     {}", resp.contradiction.subject);
                        println!("  Predicate:   {}", resp.contradiction.predicate);
                        println!("  Created at:  {}", resp.contradiction.created_at);
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Verification handlers
// ---------------------------------------------------------------------------

fn handle_verification(project_dir: &Path, args: VerificationArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        VerificationCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListVerifications(
                    autore_app::ListVerificationsQuery { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Verifications(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.records).map_err(|e| e.to_string())?;
                        print_list_json_with_schema("verifications", "records", &val);
                    }
                    OutputFormat::Human => {
                        if resp.records.is_empty() {
                            println!("No verification records.");
                        } else {
                            println!("{:<38} {:<20} Subject", "ID", "State");
                            println!("{}", "-".repeat(80));
                            for r in &resp.records {
                                println!("{:<38} {:<20} {:?}", r.id, r.state, r.subject);
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        VerificationCommand::Show { id, output } => {
            let vid = VerificationRecordId::from_uuid(
                uuid::Uuid::parse_str(&id)
                    .map_err(|e| format!("invalid verification record ID: {e}"))?,
            );
            let result = client
                .query(ApplicationQuery::GetVerification(
                    autore_app::GetVerificationQuery { id: vid },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Verification(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.record).map_err(|e| e.to_string())?;
                        print_json_with_schema("verification", &val);
                    }
                    OutputFormat::Human => {
                        println!("Verification: {}", resp.record.id);
                        println!("  State:      {}", resp.record.state);
                        println!("  Subject:    {:?}", resp.record.subject);
                        println!("  Created at: {}", resp.record.created_at);
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Operation handlers
// ---------------------------------------------------------------------------

fn handle_operation(project_dir: &Path, args: OperationArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        OperationCommand::List { output } => {
            let result = client
                .query(ApplicationQuery::ListOperations(
                    autore_app::ListOperationsQuery { project: project_id },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Operations(resp) => match output {
                    OutputFormat::Json => {
                        let val = serde_json::to_value(&resp.operations)
                            .map_err(|e| e.to_string())?;
                        print_list_json_with_schema("operations", "operations", &val);
                    }
                    OutputFormat::Human => {
                        if resp.operations.is_empty() {
                            println!("No operations.");
                        } else {
                            println!(
                                "{:<38} {:<30} {:<15}",
                                "ID", "Kind", "State"
                            );
                            println!("{}", "-".repeat(85));
                            for op in &resp.operations {
                                println!("{:<38} {:<30} {:<15}", op.id, op.kind, op.state);
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        OperationCommand::Show { id, output } => {
            let op_id = OperationId::from_uuid(
                uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid operation ID: {e}"))?,
            );
            let result = client
                .query(ApplicationQuery::GetOperation(
                    autore_app::GetOperationQuery { id: op_id },
                ))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Operation(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.operation).map_err(|e| e.to_string())?;
                        print_json_with_schema("operation", &val);
                    }
                    OutputFormat::Human => {
                        println!("Operation: {}", resp.operation.id);
                        println!("  Kind:         {}", resp.operation.kind);
                        println!("  State:        {}", resp.operation.state);
                        println!("  Requested by: {}", resp.operation.requested_by);
                        println!("  Created at:   {}", resp.operation.created_at);
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
        OperationCommand::Cancel {
            id,
            requested_by,
            reason,
        } => {
            let op_id = OperationId::from_uuid(
                uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid operation ID: {e}"))?,
            );
            let result = client
                .execute(ApplicationCommand::CancelOperation(
                    autore_app::CancelOperationRequest {
                        project: project_id,
                        id: op_id,
                        requested_by,
                        reason,
                    },
                ))
                .map_err(|e| format!("{e}"))?;
            print_command_result("operation-cancelled", &result);
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Events handlers
// ---------------------------------------------------------------------------

fn handle_events(project_dir: &Path, args: EventsArgs) -> Result<(), String> {
    let (client, project_id) = build_client(project_dir)?;
    match args.command {
        EventsCommand::List {
            after,
            limit,
            output,
        } => {
            let result = client
                .query(ApplicationQuery::ListEvents(autore_app::ListEventsQuery {
                    project: project_id,
                    after_sequence: after,
                    limit,
                }))
                .map_err(|e| format!("{e}"))?;
            match result {
                QueryResult::Events(resp) => match output {
                    OutputFormat::Json => {
                        let val =
                            serde_json::to_value(&resp.events).map_err(|e| e.to_string())?;
                        print_list_json_with_schema("events", "events", &val);
                    }
                    OutputFormat::Human => {
                        if resp.events.is_empty() {
                            println!("No events.");
                        } else {
                            println!(
                                "{:<10} {:<38} {:<30}",
                                "Seq", "Kind", "Source"
                            );
                            println!("{}", "-".repeat(80));
                            for ev in &resp.events {
                                println!("{:<10} {:<38} {:<30}", ev.sequence, ev.kind, ev.source);
                            }
                        }
                    }
                },
                _ => unreachable!(),
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Generic command result printer
// ---------------------------------------------------------------------------

fn print_command_result(schema: &str, result: &CommandResult) {
    match serde_json::to_value(result) {
        Ok(val) => print_json_with_schema(schema, &val),
        Err(_) => println!("OK: {result:?}"),
    }
}
