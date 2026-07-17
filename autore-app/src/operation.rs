use autore_core::operation::OperationState;
use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::{
    CancellationRequest, OPERATION_KIND_ARTIFACT_IMPORT, Operation, OperationFailure,
};
use autore_schema::ids::ProjectId;
use autore_store::storage::database::Database;
use autore_store::storage::operation_store::{OperationStore, SqliteOperationStore};

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
    Operation::new(project, OPERATION_KIND_ARTIFACT_IMPORT.clone(), "cli")
}

#[test]
fn operation_cooperative_cancellation_request() {
    let db = test_db();
    let pid = insert_project(&db);
    let store = SqliteOperationStore::new(&db);

    let op = sample_operation(pid);
    store.insert(&op).unwrap();
    store
        .transition(op.id, OperationState::Running, None)
        .unwrap();

    let cr = CancellationRequest::new(op.id, "user", Some("user requested stop".into()));
    store.request_cancellation(&cr).unwrap();

    let fetched = store.get(op.id).unwrap().unwrap();
    assert_eq!(
        fetched.state,
        OperationState::Running,
        "cancellation request is cooperative — state must NOT change"
    );

    let requests = store.list_cancellation_requests(op.id).unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].requested_by, "user");
    assert_eq!(requests[0].reason, Some("user requested stop".into()));

    store
        .transition(op.id, OperationState::Cancelling, None)
        .unwrap();
    let fetched = store.get(op.id).unwrap().unwrap();
    assert_eq!(fetched.state, OperationState::Cancelling);

    store
        .transition(op.id, OperationState::Cancelled, None)
        .unwrap();
    let fetched = store.get(op.id).unwrap().unwrap();
    assert_eq!(fetched.state, OperationState::Cancelled);
    assert!(fetched.state.is_terminal());
}

#[test]
fn operation_failure_details_stored() {
    let db = test_db();
    let pid = insert_project(&db);
    let store = SqliteOperationStore::new(&db);

    let op = sample_operation(pid);
    store.insert(&op).unwrap();
    store
        .transition(op.id, OperationState::Running, None)
        .unwrap();

    let failure = OperationFailure {
        code: NamespacedId::parse("core.error.resource-exhausted").unwrap(),
        message: "disk space insufficient".into(),
        details: None,
    };
    store
        .transition(op.id, OperationState::Failed, Some(failure.clone()))
        .unwrap();

    let fetched = store.get(op.id).unwrap().unwrap();
    assert_eq!(fetched.state, OperationState::Failed);
    assert!(fetched.state.is_terminal());

    let f = fetched.failure.expect("failure details must be stored");
    assert_eq!(f.code, failure.code);
    assert_eq!(f.message, "disk space insufficient");
}

#[test]
fn operation_terminal_states_terminal() {
    let db = test_db();
    let pid = insert_project(&db);

    for terminal_state in [
        OperationState::Completed,
        OperationState::Failed,
        OperationState::Cancelled,
        OperationState::Inconclusive,
    ] {
        let store = SqliteOperationStore::new(&db);
        let op = sample_operation(pid);
        store.insert(&op).unwrap();
        store
            .transition(op.id, OperationState::Running, None)
            .unwrap();

        if terminal_state == OperationState::Cancelled {
            store
                .transition(op.id, OperationState::Cancelling, None)
                .unwrap();
        }

        let failure = if terminal_state == OperationState::Failed {
            Some(OperationFailure {
                code: NamespacedId::parse("core.error.test").unwrap(),
                message: "test failure".into(),
                details: None,
            })
        } else {
            None
        };

        store.transition(op.id, terminal_state, failure).unwrap();

        let fetched = store.get(op.id).unwrap().unwrap();
        assert_eq!(fetched.state, terminal_state);
        assert!(fetched.state.is_terminal());

        for target in [
            OperationState::Queued,
            OperationState::Running,
            OperationState::Paused,
            OperationState::Cancelling,
            OperationState::Completed,
            OperationState::Failed,
            OperationState::Cancelled,
            OperationState::Blocked,
            OperationState::Inconclusive,
        ] {
            let result = store.transition(op.id, target, None);
            assert!(
                result.is_err(),
                "terminal state {terminal_state} -> {target} must be rejected"
            );
        }
    }
}
