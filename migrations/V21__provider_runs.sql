-- V21: Stage 1 provider runs
-- Operational runs of provider instances, linked to an operation record.
-- Named stage1_provider_runs to avoid collision with the Stage 0
-- provider_runs table (V5).
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS stage1_provider_runs (
    id BLOB PRIMARY KEY NOT NULL,
    instance_id BLOB NOT NULL REFERENCES provider_instances(id),
    request_id BLOB NOT NULL,
    operation_id BLOB NOT NULL REFERENCES operations(id),
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NULL,
    error_message TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_stage1_provider_runs_instance
    ON stage1_provider_runs(instance_id);

CREATE INDEX IF NOT EXISTS idx_stage1_provider_runs_operation
    ON stage1_provider_runs(operation_id);

CREATE INDEX IF NOT EXISTS idx_stage1_provider_runs_status
    ON stage1_provider_runs(status);
