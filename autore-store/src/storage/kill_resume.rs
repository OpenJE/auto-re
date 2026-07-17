//! Kill→resume durability tests for Stage 0.
//!
//! These tests prove the Stage 0 descendant of the M1 kill→resume guarantee:
//! - Operation state and its corresponding event are atomically committed
//! - After simulated process kill (drop Database, reopen), no duplicate events
//! - Partial writes that fail emit are fully rolled back (atomicity round trip)

#[cfg(test)]
mod tests {
    use autore_core::operation::OperationState;
    use autore_schema::domain::records::{
        EventSource, EventSubject, Operation, Project,
        EVENT_KIND_OPERATION_STARTED, OPERATION_KIND_ARTIFACT_IMPORT,
    };
    use autore_schema::ids::{OperationId, ProjectId};

    use crate::storage::database::Database;
    use crate::storage::event_store::{EventStore, SqliteEventStore, with_event};
    use crate::storage::operation_store::{OperationStore, SqliteOperationStore};
    use crate::storage::project_store::{ProjectStore, SqliteProjectStore};

    fn insert_project(db: &Database) -> ProjectId {
        let project = Project::new("durability-test-project");
        let store = SqliteProjectStore::new(db);
        store.insert_project(&project).unwrap();
        project.id
    }

    fn insert_queued_operation(db: &Database, pid: ProjectId) -> Operation {
        let op = Operation::new(pid, OPERATION_KIND_ARTIFACT_IMPORT.clone(), "test-agent");
        let store = SqliteOperationStore::new(db);
        store.insert(&op).unwrap();
        op
    }

    // -----------------------------------------------------------------------
    // kill_resume_operation_durability
    // -----------------------------------------------------------------------

    /// Happy path: insert an Operation as Queued, atomically transition to
    /// Running + emit `core.operation.started` event via `with_event`, drop
    /// the Database handle (simulating process kill), reopen the same SQLite
    /// file, and assert both the operation row is Running AND the event row
    /// is present with sequence 1 and kind `core.operation.started`.
    #[test]
    fn kill_resume_operation_durability() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("project.sqlite3");

        // Phase 1: create state + event atomically
        {
            let db = Database::open(&db_path).unwrap();
            let pid = insert_project(&db);
            let op = insert_queued_operation(&db, pid);
            let op_id_bytes = op.id.as_uuid().as_bytes().to_vec();

            // Atomically transition Queued → Running + emit started event.
            // Uses raw SQL inside the closure to avoid the Mutex deadlock
            // (Task 21 lesson: store methods re-acquire the connection lock).
            with_event(
                &db,
                pid,
                EVENT_KIND_OPERATION_STARTED.clone(),
                EventSource::Operation,
                Some(EventSubject::Operation(op.id)),
                None,
                |txn| {
                    // Validate transition via the core state machine.
                    OperationState::Queued
                        .transition(&OperationState::Running)
                        .map_err(|e| crate::Error::Validation(e.to_string()))?;

                    txn.conn()
                        .execute(
                            "UPDATE operations SET state = ?1, updated_at = ?2 WHERE id = ?3",
                            rusqlite::params![
                                OperationState::Running.kind(),
                                "2026-07-17T12:00:00Z",
                                op_id_bytes,
                            ],
                        )
                        .map_err(|e| crate::Error::Database(e.to_string()))?;
                    Ok(())
                },
            )
            .unwrap();

            // Pre-kill assertions (sanity check before simulating the kill).
            let op_store = SqliteOperationStore::new(&db);
            let fetched = op_store.get(op.id).unwrap().unwrap();
            assert_eq!(fetched.state, OperationState::Running);

            let ev_store = SqliteEventStore::new(&db);
            let events = ev_store.events_for_project(pid).unwrap();
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].sequence, 1);

            // Database handle is dropped here — simulates process kill.
        }

        // Phase 2: reopen the same file (simulating process restart)
        {
            let db = Database::open(&db_path).unwrap();

            // Query operation ID and project ID (then release lock).
            let (op_id, pid) = {
                let conn = db.connection().unwrap();
                let op_id_bytes: Vec<u8> = conn
                    .query_row(
                        "SELECT id FROM operations LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                let pid_bytes: Vec<u8> = conn
                    .query_row(
                        "SELECT id FROM projects LIMIT 1",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                (
                    OperationId::from_uuid(uuid::Uuid::from_slice(&op_id_bytes).unwrap()),
                    ProjectId::from_uuid(uuid::Uuid::from_slice(&pid_bytes).unwrap()),
                )
            };

            let op_store = SqliteOperationStore::new(&db);
            let fetched = op_store.get(op_id).unwrap().unwrap();
            assert_eq!(
                fetched.state,
                OperationState::Running,
                "operation must survive process kill as Running"
            );

            let ev_store = SqliteEventStore::new(&db);
            let events = ev_store.events_for_project(pid).unwrap();
            assert_eq!(events.len(), 1, "exactly one event must survive restart");
            assert_eq!(events[0].sequence, 1, "event sequence must be 1");
            assert_eq!(
                events[0].kind,
                *EVENT_KIND_OPERATION_STARTED,
                "event kind must be core.operation.started"
            );
            assert_eq!(
                events[0].source,
                EventSource::Operation,
                "event source must be Operation"
            );
            assert_eq!(
                events[0].subject,
                Some(EventSubject::Operation(op_id)),
                "event subject must reference the operation"
            );
        }
    }

    // -----------------------------------------------------------------------
    // kill_resume_no_duplicate_events
    // -----------------------------------------------------------------------

    /// After the kill/resume, verify that re-emitting the same event kind
    /// does NOT create duplicates of the original. The first event remains
    /// sequence 1 and the total event count for the project is exactly 1
    /// until we explicitly add another.
    #[test]
    fn kill_resume_no_duplicate_events() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("project.sqlite3");

        let pid;
        let op_id;

        // Phase 1: create state + event atomically
        {
            let db = Database::open(&db_path).unwrap();
            pid = insert_project(&db);
            let op = insert_queued_operation(&db, pid);
            op_id = op.id;
            let op_id_bytes = op.id.as_uuid().as_bytes().to_vec();

            with_event(
                &db,
                pid,
                EVENT_KIND_OPERATION_STARTED.clone(),
                EventSource::Operation,
                Some(EventSubject::Operation(op.id)),
                None,
                |txn| {
                    OperationState::Queued
                        .transition(&OperationState::Running)
                        .map_err(|e| crate::Error::Validation(e.to_string()))?;

                    txn.conn()
                        .execute(
                            "UPDATE operations SET state = ?1, updated_at = ?2 WHERE id = ?3",
                            rusqlite::params![
                                OperationState::Running.kind(),
                                "2026-07-17T12:00:00Z",
                                op_id_bytes,
                            ],
                        )
                        .map_err(|e| crate::Error::Database(e.to_string()))?;
                    Ok(())
                },
            )
            .unwrap();
        }

        // Phase 2: reopen — the original event must be the only one
        {
            let db = Database::open(&db_path).unwrap();
            let ev_store = SqliteEventStore::new(&db);

            let events = ev_store.events_for_project(pid).unwrap();
            assert_eq!(events.len(), 1, "exactly one event after restart");
            assert_eq!(events[0].sequence, 1, "original event is sequence 1");
            assert_eq!(events[0].kind, *EVENT_KIND_OPERATION_STARTED);

            // No duplicate of the started event exists.
            let started_count = events
                .iter()
                .filter(|e| e.kind == *EVENT_KIND_OPERATION_STARTED)
                .count();
            assert_eq!(started_count, 1, "no duplicate started events");
        }

        // Phase 3: attempt to emit a second event after restart — should
        // produce sequence 2, not overwrite sequence 1.
        {
            let db = Database::open(&db_path).unwrap();

            // Transition Running → Completed with a new event.
            let op_id_bytes = op_id.as_uuid().as_bytes().to_vec();
            with_event(
                &db,
                pid,
                EVENT_KIND_OPERATION_STARTED.clone(),
                EventSource::Operation,
                Some(EventSubject::Operation(op_id)),
                None,
                |txn| {
                    txn.conn()
                        .execute(
                            "UPDATE operations SET state = ?1, updated_at = ?2 WHERE id = ?3",
                            rusqlite::params![
                                OperationState::Completed.kind(),
                                "2026-07-17T12:01:00Z",
                                op_id_bytes,
                            ],
                        )
                        .map_err(|e| crate::Error::Database(e.to_string()))?;
                    Ok(())
                },
            )
            .unwrap();

            let ev_store = SqliteEventStore::new(&db);
            let events = ev_store.events_for_project(pid).unwrap();
            assert_eq!(events.len(), 2, "two events total after second emit");
            assert_eq!(events[0].sequence, 1, "first event remains sequence 1");
            assert_eq!(events[1].sequence, 2, "second event is sequence 2");
        }
    }

    // -----------------------------------------------------------------------
    // kill_resume_atomic_rollback
    // -----------------------------------------------------------------------

    /// Begin a transaction with `with_event`, force the closure to return
    /// `Err(Validation("simulated failure"))`, so the transaction is rolled
    /// back. Reopen the DB and assert the operation is still in its
    /// pre-mutation state AND no event was persisted.
    #[test]
    fn kill_resume_atomic_rollback() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("project.sqlite3");

        let pid;
        let op_id;

        // Phase 1: create Queued operation, then attempt a failed transition
        {
            let db = Database::open(&db_path).unwrap();
            pid = insert_project(&db);
            let op = insert_queued_operation(&db, pid);
            op_id = op.id;
            let op_id_bytes = op.id.as_uuid().as_bytes().to_vec();

            // Attempt atomic transition + event, but force failure.
            let result: crate::Result<()> = with_event(
                &db,
                pid,
                EVENT_KIND_OPERATION_STARTED.clone(),
                EventSource::Operation,
                Some(EventSubject::Operation(op.id)),
                None,
                |txn| {
                    // Mutate state inside the transaction.
                    txn.conn()
                        .execute(
                            "UPDATE operations SET state = ?1, updated_at = ?2 WHERE id = ?3",
                            rusqlite::params![
                                OperationState::Running.kind(),
                                "2026-07-17T12:00:00Z",
                                op_id_bytes,
                            ],
                        )
                        .map_err(|e| crate::Error::Database(e.to_string()))?;

                    // Simulate a failure AFTER the state mutation.
                    Err(crate::Error::Validation("simulated failure".into()))
                },
            );

            assert!(result.is_err(), "with_event must propagate the closure error");
            let err_msg = format!("{}", result.unwrap_err());
            assert!(err_msg.contains("simulated failure"));
        }

        // Phase 2: reopen — operation must still be Queued, no event
        {
            let db = Database::open(&db_path).unwrap();

            let op_store = SqliteOperationStore::new(&db);
            let fetched = op_store.get(op_id).unwrap().unwrap();
            assert_eq!(
                fetched.state,
                OperationState::Queued,
                "operation must remain Queued after rolled-back transition"
            );

            let ev_store = SqliteEventStore::new(&db);
            let events = ev_store.events_for_project(pid).unwrap();
            assert!(
                events.is_empty(),
                "no event should persist after atomic rollback, got {} events",
                events.len()
            );
        }
    }
}
