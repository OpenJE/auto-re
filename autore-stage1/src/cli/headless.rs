//! Headless campaign runner — runs a campaign without TUI.
//!
//! On restart after a kill, recovers stale leases and continues.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::OptionalExtension;
use time::OffsetDateTime;
use tokio_util::sync::CancellationToken;

use crate::analysis::{
    AnalysisBackend, AnalysisCapability, MockAnalysisBackend, MockPacketBuilder, PacketBuilder,
};
use crate::domain::EntityId;
use crate::domain::*;
use crate::ids::*;
use crate::model::*;
use crate::scheduler::*;
use crate::storage::Database;
use crate::storage::repositories::*;
use crate::worker::output::*;
use crate::worker::*;

use crate::cli::headless_queries::{NoopCampaignRepo, NoopEvidenceRepo, SqliteQueries};

struct DeterministicProvider;

#[async_trait]
impl ModelProvider for DeterministicProvider {
    async fn list_models(&self) -> crate::Result<Vec<ModelDescriptor>> {
        Ok(vec![])
    }
    async fn complete(
        &self,
        _request: ModelRequest,
        cancel: CancellationToken,
    ) -> crate::Result<ModelResponse> {
        if cancel.is_cancelled() {
            return Err(crate::Error::ModelProvider("cancelled".into()));
        }
        let output = FunctionAnalysisOutput {
            function_id: FunctionId::new(),
            symbol_name: Some(SymbolName::new("analyzed_func")),
            address: Address::new(AddressSpace::Virtual, 0x1000),
            confidence: Confidence::new(0.9).unwrap(),
            claims: vec![ProposedClaim {
                predicate: ClaimPredicate::FunctionName,
                value: ClaimValue::String("analyzed_func".into()),
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

/// Runs a headless campaign using mock backends and SQLite storage.
pub async fn run_headless(db: Arc<Database>) -> crate::Result<()> {
    let campaign_id = get_or_create_campaign(&db)?;

    let task_repo = Arc::new(SqliteTaskRepository::new(Arc::clone(&db)));
    let existing_tasks = count_tasks(&db, campaign_id)?;
    if existing_tasks == 0 {
        let backend = MockAnalysisBackend::new();
        let functions = backend.inventory(BinaryRevisionId::new()).await?;
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
            task_repo.create(&task).await?;
        }
    }

    let claim_repo = Arc::new(SqliteClaimRepository::new(Arc::clone(&db)));
    let queries = Arc::new(SqliteQueries {
        database: Arc::clone(&db),
    });

    recover_stale_leases(&db)?;

    let repos = RepositorySet {
        tasks: Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        queries: Arc::clone(&queries) as Arc<dyn SchedulerQueries>,
        campaigns: Arc::new(NoopCampaignRepo) as Arc<dyn CampaignRepository>,
        claims: Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
        evidence: Arc::new(NoopEvidenceRepo) as Arc<dyn EvidenceRepository>,
    };

    let desc = ModelDescriptor {
        id: "headless-analyzer".into(),
        name: "Headless Analyzer".into(),
        class: ModelClass::Analyzer,
        capabilities: ModelCapabilities {
            json_mode: true,
            tool_use: false,
            analysis: true,
            verification: false,
        },
        max_context_tokens: 8192,
    };

    let router = ModelRouter::new(vec![desc.clone()]);
    let scheduler = Scheduler::new(router).with_max_dispatch(10);
    let worker = WorkerRunner::new(
        Arc::new(DeterministicProvider),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
        Arc::new(NoopEvidenceRepo) as Arc<dyn EvidenceRepository>,
    );
    let packet_builder = MockPacketBuilder::new(MockAnalysisBackend::new());

    let mut evaluation = CampaignEvaluation::Idle;
    loop {
        let now = OffsetDateTime::now_utc();
        scheduler
            .run_campaign(campaign_id, &mut evaluation, &repos, now)
            .await?;

        if evaluation == CampaignEvaluation::Complete {
            break;
        }

        let tasks = queries.find_tasks_by_campaign(campaign_id).await?;
        let leased: Vec<_> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Leased)
            .cloned()
            .collect();

        for task in &leased {
            if let TaskSubject::Entity(EntityId::Function(func_id)) = task.subject {
                if function_has_claims(&db, func_id)? {
                    task_repo.complete(task.id).await?;
                    continue;
                }

                let packet = packet_builder
                    .build_packet(func_id, vec![AnalysisCapability::Decompile])
                    .await?;
                let input = WorkerInput {
                    task_id: task.id,
                    campaign_id,
                    packet,
                    model_descriptor: desc.clone(),
                    time_budget: Duration::from_secs(10),
                };
                let cancel = CancellationToken::new();
                let result = worker.run(input, cancel).await;
                if result.is_ok() {
                    accept_new_claims(&db)?;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(10)).await;

        if let Ok(delay_ms) = std::env::var("AUTO_RE_HEADLESS_DELAY_MS") {
            if let Ok(ms) = delay_ms.parse::<u64>() {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
        }
    }

    {
        let conn = db.connection()?;
        conn.execute(
            "UPDATE campaigns SET state = 'Complete' WHERE id = ?1",
            rusqlite::params![campaign_id.to_string()],
        )
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    }

    Ok(())
}

fn get_or_create_campaign(db: &Database) -> crate::Result<CampaignId> {
    let conn = db.connection()?;

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM campaigns WHERE state IN ('Active', 'Pending') LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

    if let Some(id_str) = existing {
        let uuid = uuid::Uuid::parse_str(&id_str)
            .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
        return Ok(CampaignId::from_uuid(uuid));
    }

    let with_leases: Option<String> = conn
        .query_row(
            "SELECT c.id FROM campaigns c \
             INNER JOIN tasks t ON t.campaign_id = c.id \
             WHERE t.state IN ('Leased', 'Ready', 'Pending') \
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;

    if let Some(id_str) = with_leases {
        let uuid = uuid::Uuid::parse_str(&id_str)
            .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
        return Ok(CampaignId::from_uuid(uuid));
    }

    let campaign_id = CampaignId::new();
    conn.execute(
        "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
        rusqlite::params![campaign_id.to_string(), "Headless Campaign", "Active"],
    )
    .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    Ok(campaign_id)
}

fn count_tasks(db: &Database, campaign_id: CampaignId) -> crate::Result<usize> {
    let conn = db.connection()?;
    let count: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE campaign_id = ?1",
            rusqlite::params![campaign_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    Ok(count)
}

fn accept_new_claims(db: &Database) -> crate::Result<()> {
    let conn = db.connection()?;
    conn.execute(
        "UPDATE claims SET state = 'Accepted' WHERE state = 'Proposed'",
        [],
    )
    .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    Ok(())
}

fn recover_stale_leases(db: &Database) -> crate::Result<()> {
    let conn = db.connection()?;
    conn.execute(
        "UPDATE tasks SET state = 'Ready' WHERE state = 'Leased'",
        [],
    )
    .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    conn.execute("DELETE FROM leases", [])
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    Ok(())
}

fn function_has_claims(db: &Database, func_id: FunctionId) -> crate::Result<bool> {
    let conn = db.connection()?;
    let subject_json = serde_json::to_string(&EntityId::Function(func_id))
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    let count: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM claims WHERE subject = ?1",
            rusqlite::params![subject_json],
            |row| row.get(0),
        )
        .map_err(|e| crate::Error::from(autore_core::Error::Database(e.to_string())))?;
    Ok(count > 0)
}
