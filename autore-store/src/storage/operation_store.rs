use std::collections::BTreeMap;

use autore_core::operation::{operation_state_from_str, OperationState};
use autore_schema::domain::records::{
    CancellationRequest, EventSubject, MetricMap, Operation, OperationFailure, ProgressUpdate,
};
use autore_schema::domain::{NamespacedId, Timestamp};
use autore_schema::ids::{OperationId, ProjectId};

use crate::storage::database::Database;

pub trait OperationStore: Send + Sync {
    fn insert(&self, operation: &Operation) -> crate::Result<()>;
    fn get(&self, id: OperationId) -> crate::Result<Option<Operation>>;
    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Operation>>;
    fn list_by_state(
        &self,
        project_id: ProjectId,
        state: OperationState,
    ) -> crate::Result<Vec<Operation>>;
    fn transition(
        &self,
        id: OperationId,
        target: OperationState,
        failure: Option<OperationFailure>,
    ) -> crate::Result<()>;
    fn record_progress(&self, update: &ProgressUpdate) -> crate::Result<()>;
    fn list_progress(&self, operation_id: OperationId) -> crate::Result<Vec<ProgressUpdate>>;
    fn request_cancellation(&self, request: &CancellationRequest) -> crate::Result<()>;
    fn list_cancellation_requests(
        &self,
        operation_id: OperationId,
    ) -> crate::Result<Vec<CancellationRequest>>;
}

pub struct SqliteOperationStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteOperationStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteOperationStore { db }
    }
}

fn subject_to_json(s: &Option<EventSubject>) -> crate::Result<Option<String>> {
    match s {
        Some(subject) => {
            let json =
                serde_json::to_string(subject).map_err(|e| crate::Error::Serialization(e.to_string()))?;
            Ok(Some(json))
        }
        None => Ok(None),
    }
}

fn subject_from_json(s: &str) -> Result<EventSubject, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid event subject JSON: {e}"))
}

fn failure_to_json(f: &OperationFailure) -> crate::Result<String> {
    serde_json::to_string(f).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn failure_from_json(s: &str) -> Result<OperationFailure, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid operation failure JSON: {e}"))
}

fn metrics_to_json(m: &MetricMap) -> crate::Result<String> {
    serde_json::to_string(m).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn metrics_from_json(s: &str) -> Result<MetricMap, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid metrics JSON: {e}"))
}

fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("invalid timestamp: {e}"))?;
    Ok(Timestamp::from_offset_datetime(dt))
}

fn parse_namespaced_id(s: &str) -> Result<NamespacedId, String> {
    NamespacedId::parse(s).map_err(|e| format!("invalid namespaced ID: {e}"))
}

#[derive(Debug)]
struct ParseError(String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn row_to_operation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Operation> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let kind_str: String = row.get(2)?;
    let state_str: String = row.get(3)?;
    let subject_json: Option<String> = row.get(4)?;
    let requested_by: String = row.get(5)?;
    let parent_bytes: Option<Vec<u8>> = row.get(6)?;
    let failure_json: Option<String> = row.get(7)?;
    let created_at_str: String = row.get(8)?;
    let updated_at_str: String = row.get(9)?;

    let id = OperationId::from_uuid(
        uuid::Uuid::from_slice(&id_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let project = ProjectId::from_uuid(
        uuid::Uuid::from_slice(&project_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let kind = parse_namespaced_id(&kind_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let state = operation_state_from_str(&state_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let subject = match subject_json {
        Some(json) => Some(
            subject_from_json(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(ParseError(e))))?,
        ),
        None => None,
    };

    let parent = match parent_bytes {
        Some(bytes) => Some(OperationId::from_uuid(
            uuid::Uuid::from_slice(&bytes)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Blob, Box::new(e)))?,
        )),
        None => None,
    };

    let failure = match failure_json {
        Some(json) => Some(
            failure_from_json(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(ParseError(e))))?,
        ),
        None => None,
    };

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let updated_at = parse_timestamp(&updated_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(Operation {
        id,
        project,
        kind,
        state,
        subject,
        requested_by,
        parent,
        failure,
        created_at,
        updated_at,
    })
}

fn row_to_progress(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProgressUpdate> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let op_bytes: Vec<u8> = row.get(1)?;
    let sequence: i64 = row.get(2)?;
    let message: String = row.get(3)?;
    let metrics_json: String = row.get(4)?;
    let created_at_str: String = row.get(5)?;

    let id = uuid::Uuid::from_slice(&id_bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;

    let operation_id = OperationId::from_uuid(
        uuid::Uuid::from_slice(&op_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let metrics: BTreeMap<NamespacedId, f64> = metrics_from_json(&metrics_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(ProgressUpdate {
        id,
        operation_id,
        sequence: sequence as u64,
        message,
        metrics,
        created_at,
    })
}

fn row_to_cancellation(row: &rusqlite::Row<'_>) -> rusqlite::Result<CancellationRequest> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let op_bytes: Vec<u8> = row.get(1)?;
    let requested_by: String = row.get(2)?;
    let reason: Option<String> = row.get(3)?;
    let created_at_str: String = row.get(4)?;

    let id = uuid::Uuid::from_slice(&id_bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;

    let operation_id = OperationId::from_uuid(
        uuid::Uuid::from_slice(&op_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(CancellationRequest {
        id,
        operation_id,
        requested_by,
        reason,
        created_at,
    })
}

impl OperationStore for SqliteOperationStore<'_> {
    fn insert(&self, operation: &Operation) -> crate::Result<()> {
        let id_bytes = operation.id.as_uuid().as_bytes().to_vec();
        let project_bytes = operation.project.as_uuid().as_bytes().to_vec();
        let kind = operation.kind.to_string();
        let state = operation.state.kind();
        let subject_json = subject_to_json(&operation.subject)?;
        let parent_bytes = operation
            .parent
            .map(|p| p.as_uuid().as_bytes().to_vec());
        let failure_json = operation
            .failure
            .as_ref()
            .map(failure_to_json)
            .transpose()?;
        let created_at = operation.created_at.to_string();
        let updated_at = operation.updated_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO operations \
             (id, project_id, kind, state, subject, requested_by, parent, failure, \
              created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                kind,
                state,
                subject_json,
                operation.requested_by,
                parent_bytes,
                failure_json,
                created_at,
                updated_at,
            ],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("FOREIGN KEY constraint failed") {
                crate::Error::Database(format!("foreign key violation: {msg}"))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn get(&self, id: OperationId) -> crate::Result<Option<Operation>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, kind, state, subject, requested_by, parent, \
             failure, created_at, updated_at \
             FROM operations WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_operation,
        );

        match result {
            Ok(op) => Ok(Some(op)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Operation>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, kind, state, subject, requested_by, parent, \
                 failure, created_at, updated_at \
                 FROM operations \
                 WHERE project_id = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes], row_to_operation)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_state(
        &self,
        project_id: ProjectId,
        state: OperationState,
    ) -> crate::Result<Vec<Operation>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let state_str = state.kind();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, kind, state, subject, requested_by, parent, \
                 failure, created_at, updated_at \
                 FROM operations \
                 WHERE project_id = ?1 AND state = ?2 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes, state_str], row_to_operation)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn transition(
        &self,
        id: OperationId,
        target: OperationState,
        failure: Option<OperationFailure>,
    ) -> crate::Result<()> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let current_state_str: String = conn
            .query_row(
                "SELECT state FROM operations WHERE id = ?1",
                rusqlite::params![id_bytes],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    crate::Error::NotFound(format!("operation {id} not found"))
                }
                _ => crate::Error::Database(e.to_string()),
            })?;

        let current_state =
            operation_state_from_str(&current_state_str).map_err(crate::Error::Database)?;

        current_state.transition(&target)?;

        let failure_json = failure
            .as_ref()
            .map(failure_to_json)
            .transpose()?;
        let updated_at = Timestamp::now().to_string();

        conn.execute(
            "UPDATE operations \
             SET state = ?1, failure = COALESCE(?2, failure), updated_at = ?3 \
             WHERE id = ?4",
            rusqlite::params![target.kind(), failure_json, updated_at, id_bytes],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }

    fn record_progress(&self, update: &ProgressUpdate) -> crate::Result<()> {
        let id_bytes = update.id.as_bytes().to_vec();
        let op_bytes = update.operation_id.as_uuid().as_bytes().to_vec();
        let metrics_json = metrics_to_json(&update.metrics)?;
        let created_at = update.created_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO progress_updates \
             (id, operation_id, sequence, message, metrics, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id_bytes,
                op_bytes,
                update.sequence as i64,
                update.message,
                metrics_json,
                created_at,
            ],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("FOREIGN KEY constraint failed") {
                crate::Error::Database(format!("foreign key violation: {msg}"))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn list_progress(&self, operation_id: OperationId) -> crate::Result<Vec<ProgressUpdate>> {
        let op_bytes = operation_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, operation_id, sequence, message, metrics, created_at \
                 FROM progress_updates \
                 WHERE operation_id = ?1 \
                 ORDER BY sequence ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![op_bytes], row_to_progress)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn request_cancellation(&self, request: &CancellationRequest) -> crate::Result<()> {
        let id_bytes = request.id.as_bytes().to_vec();
        let op_bytes = request.operation_id.as_uuid().as_bytes().to_vec();
        let created_at = request.created_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO cancellation_requests \
             (id, operation_id, requested_by, reason, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                id_bytes,
                op_bytes,
                request.requested_by,
                request.reason,
                created_at,
            ],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("FOREIGN KEY constraint failed") {
                crate::Error::Database(format!("foreign key violation: {msg}"))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn list_cancellation_requests(
        &self,
        operation_id: OperationId,
    ) -> crate::Result<Vec<CancellationRequest>> {
        let op_bytes = operation_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, operation_id, requested_by, reason, created_at \
                 FROM cancellation_requests \
                 WHERE operation_id = ?1 \
                 ORDER BY created_at ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![op_bytes], row_to_cancellation)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::records::OPERATION_KIND_ARTIFACT_IMPORT;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn insert_project(db: &Database) -> ProjectId {
        let pid = ProjectId::new();
        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                pid.as_uuid().as_bytes().as_slice(),
                "test-project",
                "2.0",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "{}",
            ],
        )
        .unwrap();
        pid
    }

    fn sample_operation(project: ProjectId) -> Operation {
        Operation::new(project, OPERATION_KIND_ARTIFACT_IMPORT.clone(), "test")
    }

    #[test]
    fn operation_store_insert_and_get() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();

        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.id, op.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.state, OperationState::Queued);
        assert_eq!(fetched.requested_by, "test");
    }

    #[test]
    fn operation_store_get_not_found() {
        let db = test_db();
        let store = SqliteOperationStore::new(&db);
        assert!(store.get(OperationId::new()).unwrap().is_none());
    }

    #[test]
    fn operation_store_list_by_project() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        store.insert(&sample_operation(pid)).unwrap();
        store.insert(&sample_operation(pid)).unwrap();

        let all = store.list_by_project(pid).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn operation_store_list_by_state() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op1 = sample_operation(pid);
        store.insert(&op1).unwrap();
        store.transition(op1.id, OperationState::Running, None).unwrap();

        let op2 = sample_operation(pid);
        store.insert(&op2).unwrap();

        let queued = store.list_by_state(pid, OperationState::Queued).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].id, op2.id);

        let running = store.list_by_state(pid, OperationState::Running).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, op1.id);
    }

    #[test]
    fn operation_store_transition_valid() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();

        store.transition(op.id, OperationState::Running, None).unwrap();
        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.state, OperationState::Running);

        store.transition(op.id, OperationState::Completed, None).unwrap();
        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.state, OperationState::Completed);
    }

    #[test]
    fn operation_store_transition_rejects_invalid() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();

        let result = store.transition(op.id, OperationState::Completed, None);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("invalid state transition"), "got: {msg}");

        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.state, OperationState::Queued);
    }

    #[test]
    fn operation_store_transition_with_failure() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();
        store.transition(op.id, OperationState::Running, None).unwrap();

        let failure = OperationFailure {
            code: NamespacedId::parse("core.error.timeout").unwrap(),
            message: "timed out".into(),
            details: None,
        };
        store
            .transition(op.id, OperationState::Failed, Some(failure.clone()))
            .unwrap();

        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.state, OperationState::Failed);
        let f = fetched.failure.unwrap();
        assert_eq!(f.code, failure.code);
        assert_eq!(f.message, "timed out");
    }

    #[test]
    fn operation_store_transition_not_found() {
        let db = test_db();
        let store = SqliteOperationStore::new(&db);
        let result = store.transition(OperationId::new(), OperationState::Running, None);
        assert!(result.is_err());
    }

    #[test]
    fn operation_progress_structured() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();

        let mut metrics: MetricMap = BTreeMap::new();
        metrics.insert(NamespacedId::parse("progress.percent").unwrap(), 50.0);

        let pu1 = ProgressUpdate::new(op.id, 0, "halfway", metrics.clone());
        store.record_progress(&pu1).unwrap();

        let pu2 = ProgressUpdate::new(op.id, 1, "done", BTreeMap::new());
        store.record_progress(&pu2).unwrap();

        let progress = store.list_progress(op.id).unwrap();
        assert_eq!(progress.len(), 2);
        assert_eq!(progress[0].sequence, 0);
        assert_eq!(progress[0].message, "halfway");
        assert_eq!(progress[0].metrics.len(), 1);
        assert_eq!(progress[1].sequence, 1);
    }

    #[test]
    fn progress_update_sequence_per_operation() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op1 = sample_operation(pid);
        store.insert(&op1).unwrap();

        let op2 = sample_operation(pid);
        store.insert(&op2).unwrap();

        store
            .record_progress(&ProgressUpdate::new(op1.id, 0, "op1-first", BTreeMap::new()))
            .unwrap();
        store
            .record_progress(&ProgressUpdate::new(op1.id, 1, "op1-second", BTreeMap::new()))
            .unwrap();
        store
            .record_progress(&ProgressUpdate::new(op2.id, 0, "op2-first", BTreeMap::new()))
            .unwrap();

        let p1 = store.list_progress(op1.id).unwrap();
        assert_eq!(p1.len(), 2);
        assert_eq!(p1[0].sequence, 0);
        assert_eq!(p1[1].sequence, 1);

        let p2 = store.list_progress(op2.id).unwrap();
        assert_eq!(p2.len(), 1);
        assert_eq!(p2[0].sequence, 0);
    }

    #[test]
    fn operation_store_cancellation_request() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();

        let cr = CancellationRequest::new(op.id, "user", Some("no longer needed".into()));
        store.request_cancellation(&cr).unwrap();

        let requests = store.list_cancellation_requests(op.id).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].requested_by, "user");
        assert_eq!(requests[0].reason, Some("no longer needed".into()));
    }

    #[test]
    fn operation_store_cancellation_cooperative() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(pid);
        store.insert(&op).unwrap();
        store.transition(op.id, OperationState::Running, None).unwrap();

        let cr = CancellationRequest::new(op.id, "user", None);
        store.request_cancellation(&cr).unwrap();

        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(
            fetched.state,
            OperationState::Running,
            "cancellation request must NOT force state change (cooperative)"
        );
    }

    #[test]
    fn operation_parent_child_relationship() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let parent = sample_operation(pid);
        store.insert(&parent).unwrap();

        let mut child = sample_operation(pid);
        child.parent = Some(parent.id);
        store.insert(&child).unwrap();

        let fetched = store.get(child.id).unwrap().unwrap();
        assert_eq!(fetched.parent, Some(parent.id));
    }

    #[test]
    fn operation_store_with_subject() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteOperationStore::new(&db);

        let mut op = sample_operation(pid);
        op.subject = Some(EventSubject::Project(pid));
        store.insert(&op).unwrap();

        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.subject, Some(EventSubject::Project(pid)));
    }

    #[test]
    fn operation_store_fk_project_enforced() {
        let db = test_db();
        let store = SqliteOperationStore::new(&db);

        let op = sample_operation(ProjectId::new());
        let result = store.insert(&op);
        assert!(result.is_err(), "FK violation for non-existent project");
    }

    #[test]
    fn operation_store_trait_object() {
        let db = test_db();
        let store = SqliteOperationStore::new(&db);
        fn _assert(_: &dyn OperationStore) {}
        _assert(&store);
    }

    #[test]
    fn operation_store_no_db_generated_ids() {
        let db = test_db();
        let conn = db.connection().unwrap();
        let ddl: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='operations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let upper = ddl.to_uppercase();
        assert!(!upper.contains("AUTOINCREMENT"), "operations must not use AUTOINCREMENT");
        assert!(
            !upper.contains("DEFAULT") || !upper.contains("UUID"),
            "operations must not use DEFAULT uuid()"
        );
    }
}
