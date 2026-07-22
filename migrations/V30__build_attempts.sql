-- V30: Build attempts (Stage 1)
-- Records each attempt to compile or otherwise build generated source code.
-- Captures the build configuration (JSON blob), outcome status, exit code,
-- and references to stdout/stderr/log artifact blobs for post-mortem analysis.
-- work_item_id is a loose BLOB reference to reconstruction_work_items.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS build_attempts (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NULL,
    configuration_blob TEXT,
    status TEXT NOT NULL,
    exit_code INT NULL,
    stdout_artifact_id BLOB NULL,
    stderr_artifact_id BLOB NULL,
    log_artifact_id BLOB NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_build_attempts_work_item
    ON build_attempts(work_item_id);

CREATE INDEX IF NOT EXISTS idx_build_attempts_status
    ON build_attempts(status);
