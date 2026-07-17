//! SQLite implementation of `TaskRepository` with atomic leasing.
//!
//! `SqliteTaskRepository` provides persistent storage for `Task` entities
//! using `rusqlite`. The `lease_next` method uses `BEGIN IMMEDIATE` to
//! ensure safe concurrent access from multiple scheduler instances.

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;

use crate::domain::{RequiredCapabilities, Task, TaskKind, TaskState, TaskSubject};
use crate::ids::{CampaignId, TaskId, WorkerRunId};
use crate::storage::database::Database;
use crate::storage::repositories::TaskRepository;

/// SQLite-backed task repository with atomic leasing.
pub struct SqliteTaskRepository {
    database: Arc<Database>,
}

impl SqliteTaskRepository {
    /// Creates a new repository backed by the given database.
    pub fn new(database: Arc<Database>) -> Self {
        Self { database }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        crate::domain::TaskPriority::new(priority_score as u64),
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

fn db_err(e: rusqlite::Error) -> crate::Error {
    crate::Error::Database(e.to_string())
}

fn json_err(e: serde_json::Error) -> crate::Error {
    crate::Error::Database(format!("JSON serialization error: {e}"))
}

// ---------------------------------------------------------------------------
// TaskRepository implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn create(&self, task: &Task) -> crate::Result<TaskId> {
        let conn = self.database.connection()?;

        let kind_json = serde_json::to_string(&task.kind).map_err(json_err)?;
        let subject_json = serde_json::to_string(&task.subject).map_err(json_err)?;
        let caps_json = serde_json::to_string(&task.required_capabilities).map_err(json_err)?;
        let deps_uuids: Vec<uuid::Uuid> =
            task.dependencies.iter().map(|id| *id.as_uuid()).collect();
        let deps_json = serde_json::to_string(&deps_uuids).map_err(json_err)?;
        let preferred_worker = task.preferred_worker.map(|w| w.to_string());
        let state_str = task_state_to_str(&task.state);

        conn.execute(
            "INSERT INTO tasks (id, campaign_id, kind, subject, state, priority, \
             required_capabilities, dependencies, attempt_count, maximum_attempts, \
             preferred_worker, preferred_model_class, input_revision) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                task.id.to_string(),
                task.campaign_id.to_string(),
                kind_json,
                subject_json,
                state_str,
                task.priority.score() as i64,
                caps_json,
                deps_json,
                task.attempt_count,
                task.maximum_attempts,
                preferred_worker,
                task.preferred_model_class,
                task.input_revision as i64,
            ],
        )
        .map_err(db_err)?;

        Ok(task.id)
    }

    async fn lease_next(
        &self,
        campaign_id: CampaignId,
        now: OffsetDateTime,
    ) -> crate::Result<Option<Task>> {
        let mut conn = self.database.connection()?;

        // Use BEGIN IMMEDIATE for concurrent safety.
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(db_err)?;

        let now_ts = now.unix_timestamp();

        // Find highest-priority Ready task (or Leased task with expired lease)
        // with all dependencies completed.
        let result = tx
            .query_row(
                "SELECT t.id, t.campaign_id, t.kind, t.subject, t.state, t.priority, \
                 t.required_capabilities, t.dependencies, t.attempt_count, \
                 t.maximum_attempts, t.preferred_worker, t.preferred_model_class, \
                 t.input_revision \
                 FROM tasks t \
                 WHERE t.campaign_id = ?1 \
                   AND ( \
                     t.state = 'Ready' \
                     OR (t.state = 'Leased' AND EXISTS ( \
                       SELECT 1 FROM leases WHERE leases.task_id = t.id \
                       AND CAST(leases.expires_at AS INTEGER) <= ?2 \
                     )) \
                   ) \
                   AND NOT EXISTS ( \
                     SELECT 1 FROM json_each(t.dependencies) AS dep \
                     LEFT JOIN tasks dt ON dt.id = dep.value \
                     WHERE dt.id IS NULL OR dt.state != 'Completed' \
                   ) \
                 ORDER BY t.priority DESC \
                 LIMIT 1",
                params![campaign_id.to_string(), now_ts],
                task_from_row,
            )
            .optional()
            .map_err(db_err)?;

        let Some(mut task) = result else {
            tx.commit().map_err(db_err)?;
            return Ok(None);
        };

        // Transition to Leased.
        task.state = TaskState::Leased;
        tx.execute(
            "UPDATE tasks SET state = 'Leased' WHERE id = ?1",
            params![task.id.to_string()],
        )
        .map_err(db_err)?;

        // Insert lease with 5-minute expiry (replaces any expired lease via PK).
        let expires_at = now_ts + 300;
        let lease_id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT OR REPLACE INTO leases (task_id, worker_id, expires_at) \
             VALUES (?1, ?2, ?3)",
            params![task.id.to_string(), lease_id, expires_at.to_string()],
        )
        .map_err(db_err)?;

        tx.commit().map_err(db_err)?;
        Ok(Some(task))
    }

    async fn renew_lease(&self, task_id: TaskId, until: OffsetDateTime) -> crate::Result<()> {
        let conn = self.database.connection()?;
        let now_ts = OffsetDateTime::now_utc().unix_timestamp();

        // Check lease exists and is not expired.
        let expires_str: String = conn
            .query_row(
                "SELECT expires_at FROM leases WHERE task_id = ?1",
                params![task_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    crate::Error::Validation(format!("no active lease for task {task_id}"))
                }
                other => db_err(other),
            })?;

        let expires_ts: i64 = expires_str.parse().map_err(|_| {
            crate::Error::Database(format!("invalid expires_at value: {expires_str}"))
        })?;

        if expires_ts <= now_ts {
            return Err(crate::Error::Validation(format!(
                "lease for task {task_id} has expired"
            )));
        }

        conn.execute(
            "UPDATE leases SET expires_at = ?1 WHERE task_id = ?2",
            params![until.unix_timestamp().to_string(), task_id.to_string()],
        )
        .map_err(db_err)?;

        Ok(())
    }

    async fn complete(&self, task_id: TaskId) -> crate::Result<()> {
        let conn = self.database.connection()?;

        conn.execute(
            "UPDATE tasks SET state = 'Completed' WHERE id = ?1",
            params![task_id.to_string()],
        )
        .map_err(db_err)?;

        conn.execute(
            "DELETE FROM leases WHERE task_id = ?1",
            params![task_id.to_string()],
        )
        .map_err(db_err)?;

        Ok(())
    }

    async fn fail(&self, task_id: TaskId, error: String) -> crate::Result<()> {
        let conn = self.database.connection()?;

        conn.execute(
            "UPDATE tasks SET state = 'Failed', attempt_count = attempt_count + 1, \
             error_message = ?1 WHERE id = ?2",
            params![error, task_id.to_string()],
        )
        .map_err(db_err)?;

        conn.execute(
            "DELETE FROM leases WHERE task_id = ?1",
            params![task_id.to_string()],
        )
        .map_err(db_err)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{TaskKind, TaskPriority, TaskSubject};

    fn setup() -> (Arc<Database>, SqliteTaskRepository) {
        let db = Arc::new(Database::open_in_memory().expect("in-memory DB"));
        let repo = SqliteTaskRepository::new(Arc::clone(&db));
        // Insert a campaign for foreign key constraint.
        {
            let conn = db.connection().unwrap();
            let cid = campaign_id();
            conn.execute(
                "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
                params![cid.to_string(), "test", "Pending"],
            )
            .unwrap();
        }
        (db, repo)
    }

    fn campaign_id() -> CampaignId {
        // Use a deterministic UUID for tests.
        CampaignId::from_uuid(
            uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        )
    }

    fn make_task(campaign: CampaignId, priority: u64) -> Task {
        Task::new(
            TaskId::new(),
            campaign,
            TaskKind::AnalyzeFunction,
            TaskSubject::Binary,
            TaskPriority::new(priority),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        )
    }

    fn make_ready_task(campaign: CampaignId, priority: u64) -> Task {
        let mut task = make_task(campaign, priority);
        task.state = TaskState::Ready;
        task
    }

    #[tokio::test]
    async fn task_repository_create_and_fetch() {
        let (db, repo) = setup();
        let cid = campaign_id();
        let task = make_task(cid, 100);

        let id = repo.create(&task).await.unwrap();
        assert_eq!(id, task.id);

        let conn = db.connection().unwrap();
        let state: String = conn
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                params![task.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "Pending");
    }

    #[tokio::test]
    async fn task_repository_lease_next_returns_ready_task() {
        let (_db, repo) = setup();
        let cid = campaign_id();
        let task = make_ready_task(cid, 100);
        repo.create(&task).await.unwrap();

        let now = OffsetDateTime::now_utc();
        let leased = repo.lease_next(cid, now).await.unwrap();

        assert!(leased.is_some());
        let leased_task = leased.unwrap();
        assert_eq!(leased_task.id, task.id);
        assert_eq!(leased_task.state, TaskState::Leased);
    }

    #[tokio::test]
    async fn task_repository_lease_next_respects_dependencies() {
        let (_db, repo) = setup();
        let cid = campaign_id();

        let dep_task = make_ready_task(cid, 200);
        repo.create(&dep_task).await.unwrap();

        let mut dependent = make_ready_task(cid, 100);
        dependent.dependencies.push(dep_task.id);
        repo.create(&dependent).await.unwrap();

        let now = OffsetDateTime::now_utc();

        let leased = repo.lease_next(cid, now).await.unwrap().unwrap();
        assert_eq!(leased.id, dep_task.id);

        let leased2 = repo.lease_next(cid, now).await.unwrap();
        assert!(leased2.is_none(), "dependent task should not be leaseable");

        repo.complete(dep_task.id).await.unwrap();

        let leased3 = repo.lease_next(cid, now).await.unwrap();
        assert!(leased3.is_some());
        assert_eq!(leased3.unwrap().id, dependent.id);
    }

    #[tokio::test]
    async fn task_repository_complete_updates_state_and_claims() {
        let (db, repo) = setup();
        let cid = campaign_id();
        let task = make_ready_task(cid, 100);
        repo.create(&task).await.unwrap();

        let now = OffsetDateTime::now_utc();
        repo.lease_next(cid, now).await.unwrap();

        repo.complete(task.id).await.unwrap();

        {
            let conn = db.connection().unwrap();
            let state: String = conn
                .query_row(
                    "SELECT state FROM tasks WHERE id = ?1",
                    params![task.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(state, "Completed");

            let lease_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM leases WHERE task_id = ?1",
                    params![task.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(lease_count, 0);
        }

        repo.complete(task.id).await.unwrap();
    }

    #[tokio::test]
    async fn task_repository_fail_increments_attempt_count() {
        let (db, repo) = setup();
        let cid = campaign_id();
        let task = make_ready_task(cid, 100);
        repo.create(&task).await.unwrap();

        let now = OffsetDateTime::now_utc();
        repo.lease_next(cid, now).await.unwrap();
        repo.fail(task.id, "test error".to_string()).await.unwrap();

        let conn = db.connection().unwrap();
        let (state, attempt_count, error_msg): (String, u32, Option<String>) = conn
            .query_row(
                "SELECT state, attempt_count, error_message FROM tasks WHERE id = ?1",
                params![task.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert_eq!(state, "Failed");
        assert_eq!(attempt_count, 1);
        assert_eq!(error_msg, Some("test error".to_string()));

        let lease_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM leases WHERE task_id = ?1",
                params![task.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lease_count, 0);
    }

    #[tokio::test]
    async fn concurrent_lease_exactly_one_wins() {
        let db = Arc::new(Database::open_in_memory().expect("in-memory DB"));
        let cid = campaign_id();
        {
            let conn = db.connection().unwrap();
            conn.execute(
                "INSERT INTO campaigns (id, name, state) VALUES (?1, ?2, ?3)",
                params![cid.to_string(), "test", "Pending"],
            )
            .unwrap();
        }

        let task = make_ready_task(cid, 100);
        let repo1 = Arc::new(SqliteTaskRepository::new(Arc::clone(&db)));
        let repo2 = Arc::clone(&repo1);
        repo1.create(&task).await.unwrap();

        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let now = OffsetDateTime::now_utc();

        let b1 = Arc::clone(&barrier);
        let r1 = Arc::clone(&repo1);
        let h1 = tokio::spawn(async move {
            b1.wait().await;
            r1.lease_next(cid, now).await.unwrap()
        });

        let b2 = Arc::clone(&barrier);
        let r2 = Arc::clone(&repo2);
        let h2 = tokio::spawn(async move {
            b2.wait().await;
            r2.lease_next(cid, now).await.unwrap()
        });

        let result1 = h1.await.unwrap();
        let result2 = h2.await.unwrap();

        let winners: Vec<&Task> = result1.iter().chain(result2.iter()).collect();
        assert_eq!(winners.len(), 1, "exactly one caller must win the lease");
        assert_eq!(winners[0].id, task.id);
        assert_eq!(winners[0].state, TaskState::Leased);
    }

    #[tokio::test]
    async fn expired_lease_is_reclaimed() {
        let (_db, repo) = setup();
        let cid = campaign_id();
        let task = make_ready_task(cid, 100);
        repo.create(&task).await.unwrap();

        let early_now = OffsetDateTime::from_unix_timestamp(1_000).unwrap();
        let leased = repo.lease_next(cid, early_now).await.unwrap();
        assert!(leased.is_some(), "first lease should succeed");
        assert_eq!(leased.unwrap().id, task.id);

        let second_call = repo.lease_next(cid, early_now).await.unwrap();
        assert!(
            second_call.is_none(),
            "non-expired lease should not be reclaimed"
        );

        let after_expiry = OffsetDateTime::from_unix_timestamp(1_301).unwrap();
        let reclaimed = repo.lease_next(cid, after_expiry).await.unwrap();
        assert!(reclaimed.is_some(), "expired lease should be reclaimed");
        let reclaimed_task = reclaimed.unwrap();
        assert_eq!(reclaimed_task.id, task.id);
        assert_eq!(reclaimed_task.state, TaskState::Leased);
    }

    #[tokio::test]
    async fn complete_is_idempotent() {
        let (db, repo) = setup();
        let cid = campaign_id();
        let task = make_ready_task(cid, 100);
        repo.create(&task).await.unwrap();

        let now = OffsetDateTime::now_utc();
        repo.lease_next(cid, now).await.unwrap();

        repo.complete(task.id).await.unwrap();
        repo.complete(task.id).await.unwrap();

        let conn = db.connection().unwrap();
        let state: String = conn
            .query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                params![task.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "Completed");
    }

    #[tokio::test]
    async fn artifact_reference_integrity() {
        let (db, repo) = setup();
        let cid = campaign_id();

        let mut task = make_task(cid, 42);
        task.state = TaskState::Ready;
        task.input_revision = 7;
        task.dependencies = vec![];
        repo.create(&task).await.unwrap();

        let now = OffsetDateTime::now_utc();
        let leased = repo.lease_next(cid, now).await.unwrap().unwrap();
        assert_eq!(leased.id, task.id);
        assert_eq!(leased.priority.score(), 42);
        assert_eq!(leased.input_revision, 7);
        assert_eq!(leased.kind, TaskKind::AnalyzeFunction);
        assert_eq!(leased.subject, TaskSubject::Binary);
        assert_eq!(leased.maximum_attempts, 3);

        repo.complete(task.id).await.unwrap();

        let conn = db.connection().unwrap();
        let (kind_json, subject_json, caps_json, priority, input_rev): (
            String,
            String,
            String,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT kind, subject, required_capabilities, priority, input_revision \
                 FROM tasks WHERE id = ?1",
                params![task.id.to_string()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();

        let kind: TaskKind = serde_json::from_str(&kind_json).unwrap();
        let subject: TaskSubject = serde_json::from_str(&subject_json).unwrap();
        let caps: RequiredCapabilities = serde_json::from_str(&caps_json).unwrap();

        assert_eq!(kind, TaskKind::AnalyzeFunction);
        assert_eq!(subject, TaskSubject::Binary);
        assert!(!caps.decompilation);
        assert!(caps.disassembly);
        assert_eq!(priority, 42);
        assert_eq!(input_rev, 7);
    }
}
