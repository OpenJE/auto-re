//! Campaign smoke test — scheduler + mocks + SQLite + TUI channel.
//!
//! Integration test that builds a runtime with mock backends, SQLite storage,
//! scheduler, worker runner, and TUI channel. Runs a campaign with the
//! 10-function fixture and asserts completion, claims, and TUI updates.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use auto_re::analysis::{
    AnalysisBackend, AnalysisCapability, MockAnalysisBackend, MockPacketBuilder, PacketBuilder,
};
use auto_re::domain::*;
use auto_re::ids::*;
use auto_re::model::*;
use auto_re::scheduler::*;
use auto_re::storage::repositories::*;
use auto_re::storage::*;
use auto_re::tui::state::TuiUpdate;
use auto_re::worker::output::*;
use auto_re::worker::*;

// ---------------------------------------------------------------------------
// SQLite SchedulerQueries — replicates task_from_row from task.rs
// ---------------------------------------------------------------------------

struct SqliteQueries {
    database: Arc<Database>,
}

fn task_state_from_str(s: &str) -> TaskState {
    match s {
        "Pending" => TaskState::Pending,
        "Ready" => TaskState::Ready,
        "Leased" => TaskState::Leased,
        "Running" => TaskState::Running,
        "Blocked" => TaskState::Blocked,
        "Completed" => TaskState::Completed,
        "Failed" => TaskState::Failed,
        "Cancelled" => TaskState::Cancelled,
        "Stale" => TaskState::Stale,
        _ => TaskState::Pending,
    }
}

fn task_state_to_str(state: &TaskState) -> &'static str {
    match state {
        TaskState::Pending => "Pending",
        TaskState::Ready => "Ready",
        TaskState::Leased => "Leased",
        TaskState::Running => "Running",
        TaskState::Blocked => "Blocked",
        TaskState::Completed => "Completed",
        TaskState::Failed => "Failed",
        TaskState::Cancelled => "Cancelled",
        TaskState::Stale => "Stale",
    }
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id_str: String = row.get(0)?;
    let campaign_str: String = row.get(1)?;
    let kind_json: String = row.get(2)?;
    let subject_json: String = row.get(3)?;
    let state_str: String = row.get(4)?;
    let priority_score: i64 = row.get(5)?;
    let caps_json: String = row.get(6)?;
    let deps_json: String = row.get(7)?;
    let attempt_count: u32 = row.get(8)?;
    let maximum_attempts: u32 = row.get(9)?;
    let preferred_worker: Option<String> = row.get(10)?;
    let preferred_model_class: Option<String> = row.get(11)?;
    let input_revision: u64 = row.get::<_, i64>(12)? as u64;

    let id = TaskId::from_uuid(uuid::Uuid::parse_str(&id_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?);
    let campaign_id = CampaignId::from_uuid(uuid::Uuid::parse_str(&campaign_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?);
    let kind: TaskKind = serde_json::from_str(&kind_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let subject: TaskSubject = serde_json::from_str(&subject_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let required_capabilities: RequiredCapabilities =
        serde_json::from_str(&caps_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let dependencies: Vec<TaskId> = serde_json::from_str::<Vec<uuid::Uuid>>(&deps_json)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?
        .into_iter()
        .map(TaskId::from_uuid)
        .collect();

    let preferred_worker_id = preferred_worker
        .map(|s| {
            uuid::Uuid::parse_str(&s)
                .map(WorkerRunId::from_uuid)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
        })
        .transpose()?;

    let mut task = Task::new(
        id,
        campaign_id,
        kind,
        subject,
        TaskPriority::new(priority_score as u64),
        required_capabilities,
        preferred_worker_id,
        preferred_model_class,
        maximum_attempts,
    );
    task.state = task_state_from_str(&state_str);
    task.dependencies = dependencies;
    task.attempt_count = attempt_count;
    task.input_revision = input_revision;
    Ok(task)
}

#[async_trait]
impl SchedulerQueries for SqliteQueries {
    async fn find_tasks_by_campaign(&self, campaign_id: CampaignId) -> auto_re::Result<Vec<Task>> {
        let conn = self.database.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, campaign_id, kind, subject, state, priority, \
				 required_capabilities, dependencies, attempt_count, maximum_attempts, \
				 preferred_worker, preferred_model_class, input_revision \
				 FROM tasks WHERE campaign_id = ?1",
            )
            .map_err(|e| auto_re::Error::Database(e.to_string()))?;
        let tasks = stmt
            .query_map([campaign_id.to_string()], task_from_row)
            .map_err(|e| auto_re::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| auto_re::Error::Database(e.to_string()))?;
        Ok(tasks)
    }

    async fn find_expired_leases(
        &self,
        _campaign_id: CampaignId,
        _now: OffsetDateTime,
    ) -> auto_re::Result<Vec<TaskLease>> {
        // Leases have 300s TTL; the smoke test runs in <1s. No expiry.
        Ok(vec![])
    }

    async fn update_task_state(&self, task_id: TaskId, state: TaskState) -> auto_re::Result<()> {
        let conn = self.database.connection()?;
        conn.execute(
            "UPDATE tasks SET state = ?1 WHERE id = ?2",
            rusqlite::params![task_state_to_str(&state), task_id.to_string()],
        )
        .map_err(|e| auto_re::Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete_lease(&self, _task_id: TaskId) -> auto_re::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Stub repositories
// ---------------------------------------------------------------------------

struct StubCampaignRepo;

#[async_trait]
impl CampaignRepository for StubCampaignRepo {
    async fn create(&self, _c: &Campaign) -> auto_re::Result<CampaignId> {
        Ok(CampaignId::new())
    }
    async fn find_by_id(&self, _id: CampaignId) -> auto_re::Result<Option<Campaign>> {
        Ok(None)
    }
    async fn update_state(&self, _id: CampaignId, _state: CampaignState) -> auto_re::Result<()> {
        Ok(())
    }
}

struct CollectingClaimRepo {
    claims: Mutex<Vec<Claim>>,
}

impl CollectingClaimRepo {
    fn new() -> Self {
        Self {
            claims: Mutex::new(Vec::new()),
        }
    }
    fn count(&self) -> usize {
        self.claims.lock().unwrap().len()
    }
}

#[async_trait]
impl ClaimRepository for CollectingClaimRepo {
    async fn create(&self, claim: &Claim) -> auto_re::Result<ClaimId> {
        self.claims.lock().unwrap().push(claim.clone());
        Ok(claim.id)
    }
    async fn find_by_id(&self, _id: ClaimId) -> auto_re::Result<Option<Claim>> {
        Ok(None)
    }
}

struct CollectingEvidenceRepo;

#[async_trait]
impl EvidenceRepository for CollectingEvidenceRepo {
    async fn create(&self, _e: &Evidence) -> auto_re::Result<EvidenceId> {
        Ok(EvidenceId::new())
    }
    async fn find_by_id(&self, _id: EvidenceId) -> auto_re::Result<Option<Evidence>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Model provider returning valid FunctionAnalysisOutput JSON
// ---------------------------------------------------------------------------

struct SmokeTestProvider;

#[async_trait]
impl ModelProvider for SmokeTestProvider {
    async fn list_models(&self) -> auto_re::Result<Vec<ModelDescriptor>> {
        Ok(vec![])
    }
    async fn complete(
        &self,
        _request: ModelRequest,
        cancel: CancellationToken,
    ) -> auto_re::Result<ModelResponse> {
        if cancel.is_cancelled() {
            return Err(auto_re::Error::ModelProvider("cancelled".into()));
        }
        let output = FunctionAnalysisOutput {
            function_id: FunctionId::new(),
            symbol_name: Some(SymbolName::new("test_func")),
            address: Address::new(AddressSpace::Virtual, 0x1000),
            confidence: Confidence::new(0.9).unwrap(),
            claims: vec![ProposedClaim {
                predicate: ClaimPredicate::FunctionName,
                value: ClaimValue::String("test_func".into()),
                confidence: Confidence::new(0.95).unwrap(),
                dependencies: vec![],
            }],
            evidence: vec![ProposedEvidence {
                kind: EvidenceKind::Disassembly,
                location: Some(EvidenceLocation::new(
                    Some(Address::new(AddressSpace::Virtual, 0x1000)),
                    None,
                )),
                description: "push rbp; mov rbp, rsp".into(),
                confidence: Confidence::new(0.85).unwrap(),
            }],
            metadata: serde_json::json!({}),
        };
        Ok(ModelResponse {
            content: serde_json::to_string(&output).unwrap(),
            tokens_used: 100,
        })
    }
}

// ---------------------------------------------------------------------------
// Smoke test results and runner
// ---------------------------------------------------------------------------

struct SmokeResults {
    evaluation: CampaignEvaluation,
    tasks: Vec<Task>,
    claim_count: usize,
    tui_updates: Vec<TuiUpdate>,
}

fn model_descriptor() -> ModelDescriptor {
    ModelDescriptor {
        id: "smoke-analyzer".into(),
        name: "Smoke Analyzer".into(),
        class: ModelClass::Analyzer,
        capabilities: ModelCapabilities {
            json_mode: true,
            tool_use: false,
            analysis: true,
            verification: false,
        },
        max_context_tokens: 8192,
    }
}

async fn run_smoke_test() -> SmokeResults {
    let db = Arc::new(Database::open_in_memory().unwrap());
    let campaign_id = CampaignId::new();

    // Insert campaign for foreign key constraint.
    {
        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
            rusqlite::params![campaign_id.to_string(), "Smoke Test", "Active"],
        )
        .unwrap();
    }

    // Get 10 fixture functions from mock backend.
    let backend = MockAnalysisBackend::new();
    let functions = backend.inventory(BinaryRevisionId::new()).await.unwrap();
    assert_eq!(functions.len(), 10);

    // Create 10 Pending tasks, one per fixture function.
    let task_repo = Arc::new(SqliteTaskRepository::new(Arc::clone(&db)));
    for func in &functions {
        let task = Task::new(
            TaskId::new(),
            campaign_id,
            TaskKind::AnalyzeFunction,
            TaskSubject::Entity(EntityId::Function(func.id)),
            TaskPriority::new(100),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        task_repo.create(&task).await.unwrap();
    }

    // Set up repositories.
    let claim_repo = Arc::new(CollectingClaimRepo::new());
    let evidence_repo = Arc::new(CollectingEvidenceRepo);
    let queries = Arc::new(SqliteQueries {
        database: Arc::clone(&db),
    });

    let repos = RepositorySet {
        tasks: Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        queries: Arc::clone(&queries) as Arc<dyn SchedulerQueries>,
        campaigns: Arc::new(StubCampaignRepo) as Arc<dyn CampaignRepository>,
        claims: Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
        evidence: Arc::clone(&evidence_repo) as Arc<dyn EvidenceRepository>,
    };

    // Scheduler with high dispatch limit to lease all tasks in one tick.
    let desc = model_descriptor();
    let router = ModelRouter::new(vec![desc.clone()]);
    let scheduler = Scheduler::new(router).with_max_dispatch(10);

    // Worker runner with mock provider and real SQLite task repo.
    let worker = WorkerRunner::new(
        Arc::new(SmokeTestProvider),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
        Arc::clone(&evidence_repo) as Arc<dyn EvidenceRepository>,
    );

    let packet_builder = MockPacketBuilder::new(MockAnalysisBackend::new());

    // TUI update channel.
    let (tx, mut rx) = mpsc::channel::<TuiUpdate>(256);

    let mut campaign = Campaign::new(campaign_id, "Smoke Test");
    campaign.state = CampaignState::Active;

    // Drive the scheduler loop until Complete or timeout.
    let mut evaluation = CampaignEvaluation::Idle;
    let test_timeout = Duration::from_secs(30);

    let result = timeout(test_timeout, async {
        loop {
            let now = OffsetDateTime::now_utc();
            scheduler
                .run_campaign(campaign_id, &mut evaluation, &repos, now)
                .await
                .unwrap();

            if evaluation == CampaignEvaluation::Complete {
                break;
            }

            // Find leased tasks and run workers on them.
            let tasks = queries.find_tasks_by_campaign(campaign_id).await.unwrap();
            let leased: Vec<_> = tasks
                .iter()
                .filter(|t| t.state == TaskState::Leased)
                .cloned()
                .collect();

            for task in &leased {
                if let TaskSubject::Entity(EntityId::Function(func_id)) = task.subject {
                    let packet = packet_builder
                        .build_packet(func_id, vec![AnalysisCapability::Decompile])
                        .await
                        .unwrap();
                    let input = WorkerInput {
                        task_id: task.id,
                        campaign_id,
                        packet,
                        model_descriptor: desc.clone(),
                        time_budget: Duration::from_secs(10),
                    };
                    let cancel = CancellationToken::new();
                    worker.run(input, cancel).await.unwrap();

                    // Send TUI update for completed task.
                    let updated = queries.find_tasks_by_campaign(campaign_id).await.unwrap();
                    if let Some(t) = updated.iter().find(|t| t.id == task.id) {
                        let _ = tx.send(TuiUpdate::TaskUpdated(t.clone())).await;
                    }
                }
            }

            // Send campaign update.
            let _ = tx.send(TuiUpdate::CampaignUpdated(campaign.clone())).await;

            // Brief yield to avoid busy-spinning.
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;

    assert!(result.is_ok(), "smoke test timed out after 30s");

    // Send final campaign Complete update.
    campaign.state = CampaignState::Complete;
    let _ = tx.send(TuiUpdate::CampaignUpdated(campaign.clone())).await;

    // Collect results.
    let final_tasks = queries.find_tasks_by_campaign(campaign_id).await.unwrap();
    let claim_count = claim_repo.count();

    drop(tx);
    let mut tui_updates = Vec::new();
    while let Ok(update) = rx.try_recv() {
        tui_updates.push(update);
    }

    SmokeResults {
        evaluation,
        tasks: final_tasks,
        claim_count,
        tui_updates,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn campaign_smoke_completes() {
    let results = run_smoke_test().await;
    assert_eq!(
        results.evaluation,
        CampaignEvaluation::Complete,
        "campaign should reach Complete evaluation"
    );
}

#[tokio::test]
async fn campaign_smoke_analyzes_all_functions() {
    let results = run_smoke_test().await;
    let completed = results
        .tasks
        .iter()
        .filter(|t| t.state == TaskState::Completed)
        .count();
    assert_eq!(
        completed, 10,
        "all 10 tasks should reach Completed, got {completed}"
    );
}

#[tokio::test]
async fn campaign_smoke_produces_claims() {
    let results = run_smoke_test().await;
    assert!(
        results.claim_count > 0,
        "expected at least one claim, got {}",
        results.claim_count
    );
}

#[tokio::test]
async fn campaign_smoke_updates_tui() {
    let results = run_smoke_test().await;
    assert!(
        !results.tui_updates.is_empty(),
        "TUI channel should receive at least one update"
    );
}
