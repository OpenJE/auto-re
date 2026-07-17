-- V10: Operations + Progress Updates + Cancellation Requests
-- Operations track long-running work (artifact imports, validation, migration,
-- index rebuilds) with a state machine (Queued -> Running -> terminal states),
-- structured progress updates, and cooperative cancellation requests.
-- No AUTOINCREMENT, no DEFAULT uuid(). IDs are application-assigned UUIDv7 BLOBs.

CREATE TABLE IF NOT EXISTS operations (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    subject TEXT NULL,
    requested_by TEXT NOT NULL,
    parent BLOB NULL REFERENCES operations(id),
    failure TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_operations_project_state
    ON operations(project_id, state);

CREATE INDEX IF NOT EXISTS idx_operations_parent
    ON operations(parent);

CREATE TABLE IF NOT EXISTS progress_updates (
    id BLOB PRIMARY KEY NOT NULL,
    operation_id BLOB NOT NULL REFERENCES operations(id),
    sequence INTEGER NOT NULL,
    message TEXT NOT NULL,
    metrics TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_progress_operation_sequence
    ON progress_updates(operation_id, sequence);

CREATE TABLE IF NOT EXISTS cancellation_requests (
    id BLOB PRIMARY KEY NOT NULL,
    operation_id BLOB NOT NULL REFERENCES operations(id),
    requested_by TEXT NOT NULL,
    reason TEXT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_cancellation_operation
    ON cancellation_requests(operation_id);
