use std::sync::Arc;

use async_trait::async_trait;
use time::OffsetDateTime;

use crate::domain::*;
use crate::ids::*;
use crate::scheduler::{SchedulerQueries, TaskLease};
use crate::storage::Database;
use crate::storage::repositories::*;

pub(super) struct SqliteQueries {
    pub(super) database: Arc<Database>,
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
    async fn find_tasks_by_campaign(&self, campaign_id: CampaignId) -> crate::Result<Vec<Task>> {
        let conn = self.database.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, campaign_id, kind, subject, state, priority, \
                 required_capabilities, dependencies, attempt_count, maximum_attempts, \
                 preferred_worker, preferred_model_class, input_revision \
                 FROM tasks WHERE campaign_id = ?1",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        let tasks = stmt
            .query_map([campaign_id.to_string()], task_from_row)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;
        Ok(tasks)
    }

    async fn find_expired_leases(
        &self,
        _campaign_id: CampaignId,
        _now: OffsetDateTime,
    ) -> crate::Result<Vec<TaskLease>> {
        Ok(vec![])
    }

    async fn update_task_state(&self, task_id: TaskId, state: TaskState) -> crate::Result<()> {
        let conn = self.database.connection()?;
        let state_str = match state {
            TaskState::Pending => "Pending",
            TaskState::Ready => "Ready",
            TaskState::Leased => "Leased",
            TaskState::Running => "Running",
            TaskState::Blocked => "Blocked",
            TaskState::Completed => "Completed",
            TaskState::Failed => "Failed",
            TaskState::Cancelled => "Cancelled",
            TaskState::Stale => "Stale",
        };
        conn.execute(
            "UPDATE tasks SET state = ?1 WHERE id = ?2",
            rusqlite::params![state_str, task_id.to_string()],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;
        Ok(())
    }

    async fn delete_lease(&self, _task_id: TaskId) -> crate::Result<()> {
        Ok(())
    }
}

pub(super) struct NoopCampaignRepo;

#[async_trait]
impl CampaignRepository for NoopCampaignRepo {
    async fn create(&self, _c: &Campaign) -> crate::Result<CampaignId> {
        Ok(CampaignId::new())
    }
    async fn find_by_id(&self, _id: CampaignId) -> crate::Result<Option<Campaign>> {
        Ok(None)
    }
    async fn update_state(&self, _id: CampaignId, _state: CampaignState) -> crate::Result<()> {
        Ok(())
    }
}

pub(super) struct NoopEvidenceRepo;

#[async_trait]
impl EvidenceRepository for NoopEvidenceRepo {
    async fn create(&self, _e: &Evidence) -> crate::Result<EvidenceId> {
        Ok(EvidenceId::new())
    }
    async fn find_by_id(&self, _id: EvidenceId) -> crate::Result<Option<Evidence>> {
        Ok(None)
    }
}
