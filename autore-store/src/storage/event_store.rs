use autore_schema::domain::records::{EventSource, EventSubject, ProjectEvent};
use autore_schema::domain::{ExtensionData, NamespacedId, Timestamp};
use autore_schema::ids::{ProjectEventId, ProjectId};

use crate::storage::database::{Database, Transaction};

pub trait EventStore: Send + Sync {
    fn events_after(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
    ) -> crate::Result<Vec<ProjectEvent>>;

    fn events_for_project(&self, project_id: ProjectId) -> crate::Result<Vec<ProjectEvent>>;
}

pub struct SqliteEventStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteEventStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteEventStore { db }
    }
}

pub fn next_project_event_sequence(txn: &Transaction<'_>, project_id: ProjectId) -> crate::Result<u64> {
    let next: u64 = txn
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 \
             FROM project_events WHERE project_id = ?1",
            rusqlite::params![project_id.as_uuid().as_bytes().as_slice()],
            |row| row.get(0),
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;
    Ok(next)
}

pub fn emit_in_tx(txn: &Transaction<'_>, event: &ProjectEvent) -> crate::Result<()> {
    let id_bytes = event.id.as_uuid().as_bytes().to_vec();
    let project_bytes = event.project.as_uuid().as_bytes().to_vec();
    let kind = event.kind.to_string();
    let subject_json = event
        .subject
        .as_ref()
        .map(|s| serde_json::to_string(s).map_err(|e| crate::Error::Serialization(e.to_string())))
        .transpose()?;
    let source = event.source.to_string();
    let payload_json = event
        .payload
        .as_ref()
        .map(|p| serde_json::to_string(p).map_err(|e| crate::Error::Serialization(e.to_string())))
        .transpose()?;
    let created_at = event.created_at.to_string();

    txn.conn()
        .execute(
            "INSERT INTO project_events \
             (project_event_id, project_id, sequence, kind, subject, source, payload, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                event.sequence as i64,
                kind,
                subject_json,
                source,
                payload_json,
                created_at,
            ],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

    Ok(())
}

pub fn with_event<F, T>(
    db: &Database,
    project_id: ProjectId,
    kind: NamespacedId,
    source: EventSource,
    subject: Option<EventSubject>,
    payload: Option<ExtensionData>,
    f: F,
) -> crate::Result<T>
where
    F: FnOnce(&Transaction<'_>) -> crate::Result<T>,
{
    let txn = db.begin_transaction()?;
    let result = f(&txn)?;

    let seq = next_project_event_sequence(&txn, project_id)?;
    let event = ProjectEvent::new(project_id, seq, kind, source, subject, payload);
    emit_in_tx(&txn, &event)?;

    txn.commit()?;
    Ok(result)
}

fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("invalid timestamp: {e}"))?;
    Ok(Timestamp::from_offset_datetime(dt))
}

fn parse_namespaced_id(s: &str) -> Result<NamespacedId, String> {
    NamespacedId::parse(s).map_err(|e| format!("invalid namespaced ID: {e}"))
}

fn parse_event_source(s: &str) -> Result<EventSource, String> {
    match s {
        "Operation" => Ok(EventSource::Operation),
        "Project" => Ok(EventSource::Project),
        "Artifact" => Ok(EventSource::Artifact),
        "Entity" => Ok(EventSource::Entity),
        "Evidence" => Ok(EventSource::Evidence),
        "Hypothesis" => Ok(EventSource::Hypothesis),
        "Contradiction" => Ok(EventSource::Contradiction),
        "Verification" => Ok(EventSource::Verification),
        "Provider" => Ok(EventSource::Provider),
        other => Err(format!("unknown event source: {other}")),
    }
}

fn subject_from_json(s: &str) -> Result<EventSubject, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid event subject JSON: {e}"))
}

fn payload_from_json(s: &str) -> Result<ExtensionData, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid extension data JSON: {e}"))
}

#[derive(Debug)]
struct ParseError(String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectEvent> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let sequence: i64 = row.get(2)?;
    let kind_str: String = row.get(3)?;
    let subject_json: Option<String> = row.get(4)?;
    let source_str: String = row.get(5)?;
    let payload_json: Option<String> = row.get(6)?;
    let created_at_str: String = row.get(7)?;

    let id = ProjectEventId::from_uuid(
        uuid::Uuid::from_slice(&id_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let project = ProjectId::from_uuid(
        uuid::Uuid::from_slice(&project_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let kind = parse_namespaced_id(&kind_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let subject = match subject_json {
        Some(json) => Some(
            subject_from_json(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(ParseError(e))))?,
        ),
        None => None,
    };

    let source = parse_event_source(&source_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let payload = match payload_json {
        Some(json) => Some(
            payload_from_json(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(ParseError(e))))?,
        ),
        None => None,
    };

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(ProjectEvent {
        id,
        project,
        sequence: sequence as u64,
        kind,
        subject,
        source,
        payload,
        created_at,
    })
}

impl EventStore for SqliteEventStore<'_> {
    fn events_after(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
    ) -> crate::Result<Vec<ProjectEvent>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT project_event_id, project_id, sequence, kind, subject, \
                 source, payload, created_at \
                 FROM project_events \
                 WHERE project_id = ?1 AND sequence > ?2 \
                 ORDER BY sequence ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let events = stmt
            .query_map(
                rusqlite::params![project_bytes, after_sequence as i64],
                row_to_event,
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(events)
    }

    fn events_for_project(&self, project_id: ProjectId) -> crate::Result<Vec<ProjectEvent>> {
        self.events_after(project_id, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::records::{
        EVENT_KIND_OPERATION_COMPLETED, EVENT_KIND_OPERATION_STARTED,
        EVENT_KIND_PROJECT_CREATED, OPERATION_KIND_ARTIFACT_IMPORT,
    };
    use crate::storage::operation_store::{OperationStore, SqliteOperationStore};
    use crate::storage::project_store::{ProjectStore, SqliteProjectStore};
    use autore_schema::domain::records::{Operation, Project};
    use autore_core::operation::OperationState;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn insert_project(db: &Database) -> ProjectId {
        let project = Project::new("test-project");
        let store = SqliteProjectStore::new(db);
        store.insert_project(&project).unwrap();
        project.id
    }

    #[test]
    fn event_sequence_monotonic_per_project() {
        let db = test_db();
        let pid = insert_project(&db);

        let txn = db.begin_transaction().unwrap();
        let seq1 = next_project_event_sequence(&txn, pid).unwrap();
        assert_eq!(seq1, 1);

        let ev1 = ProjectEvent::new(
            pid,
            seq1,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
        );
        emit_in_tx(&txn, &ev1).unwrap();

        let seq2 = next_project_event_sequence(&txn, pid).unwrap();
        assert_eq!(seq2, 2);

        let ev2 = ProjectEvent::new(
            pid,
            seq2,
            EVENT_KIND_OPERATION_STARTED.clone(),
            EventSource::Operation,
            None,
            None,
        );
        emit_in_tx(&txn, &ev2).unwrap();

        let seq3 = next_project_event_sequence(&txn, pid).unwrap();
        assert_eq!(seq3, 3);

        txn.commit().unwrap();

        let store = SqliteEventStore::new(&db);
        let events = store.events_for_project(pid).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[1].sequence, 2);
    }

    #[test]
    fn event_sequence_unique_per_project() {
        let db = test_db();
        let pid = insert_project(&db);

        let txn = db.begin_transaction().unwrap();
        let seq = next_project_event_sequence(&txn, pid).unwrap();

        let ev1 = ProjectEvent::new(
            pid,
            seq,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
        );
        emit_in_tx(&txn, &ev1).unwrap();

        let ev2 = ProjectEvent::new(
            pid,
            seq,
            EVENT_KIND_OPERATION_STARTED.clone(),
            EventSource::Operation,
            None,
            None,
        );
        let result = emit_in_tx(&txn, &ev2);
        assert!(result.is_err(), "duplicate sequence must be rejected");
    }

    #[test]
    fn atomic_state_plus_event_rollback() {
        let db = test_db();
        let pid = insert_project(&db);

        let result: crate::Result<()> = with_event(
            &db,
            pid,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
            |_txn| Err(crate::Error::Validation("simulated failure".into())),
        );
        assert!(result.is_err());

        let store = SqliteEventStore::new(&db);
        let events = store.events_for_project(pid).unwrap();
        assert!(events.is_empty(), "no event should exist after rollback");
    }

    #[test]
    fn event_emitted_with_state_change_in_same_tx() {
        let db = test_db();
        let pid = insert_project(&db);
        let op_store = SqliteOperationStore::new(&db);

        let op = Operation::new(pid, OPERATION_KIND_ARTIFACT_IMPORT.clone(), "test");
        op_store.insert(&op).unwrap();

        let op_id_bytes = op.id.as_uuid().as_bytes().to_vec();
        let result = with_event(
            &db,
            pid,
            EVENT_KIND_OPERATION_COMPLETED.clone(),
            EventSource::Operation,
            Some(EventSubject::Operation(op.id)),
            None,
            |txn| {
                txn.conn().execute(
                    "UPDATE operations SET state = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![
                        "Completed",
                        "2026-07-17T00:00:00Z",
                        op_id_bytes,
                    ],
                ).map_err(|e| crate::Error::Database(e.to_string()))?;
                Ok(())
            },
        );
        assert!(result.is_ok());

        let fetched = op_store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.state, OperationState::Completed);

        let event_store = SqliteEventStore::new(&db);
        let events = event_store.events_for_project(pid).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, *EVENT_KIND_OPERATION_COMPLETED);
        assert_eq!(events[0].subject, Some(EventSubject::Operation(op.id)));
    }

    #[test]
    fn event_store_events_after() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEventStore::new(&db);

        for _ in 0..5 {
            let _ = with_event(
                &db,
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
                |_txn| Ok(()),
            );
        }

        let after_3 = store.events_after(pid, 3).unwrap();
        assert_eq!(after_3.len(), 2);
        assert_eq!(after_3[0].sequence, 4);
        assert_eq!(after_3[1].sequence, 5);
    }

    #[test]
    fn event_store_separate_projects() {
        let db = test_db();
        let pid1 = insert_project(&db);
        let pid2 = {
            let project = Project::new("project-2");
            let store = SqliteProjectStore::new(&db);
            store.insert_project(&project).unwrap();
            project.id
        };

        let _ = with_event(
            &db,
            pid1,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
            |_txn| Ok(()),
        );
        let _ = with_event(
            &db,
            pid2,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
            |_txn| Ok(()),
        );

        let store = SqliteEventStore::new(&db);
        let events1 = store.events_for_project(pid1).unwrap();
        let events2 = store.events_for_project(pid2).unwrap();
        assert_eq!(events1.len(), 1);
        assert_eq!(events2.len(), 1);
        assert_eq!(events1[0].sequence, 1);
        assert_eq!(events2[0].sequence, 1);
    }

    #[test]
    fn event_store_trait_object() {
        let db = test_db();
        let store = SqliteEventStore::new(&db);
        fn _assert(_: &dyn EventStore) {}
        _assert(&store);
    }
}
