-- V5: Providers and provider runs
-- Tracks analysis providers (tools, models, humans) and their execution runs.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS providers (
    id BLOB PRIMARY KEY NOT NULL,
    package_id BLOB NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    version TEXT NOT NULL,
    executable_hash TEXT NULL
);

CREATE TABLE IF NOT EXISTS provider_runs (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    provider_id BLOB NOT NULL REFERENCES providers(id),
    operation TEXT NOT NULL,
    input_artifacts TEXT NOT NULL DEFAULT '[]',
    configuration_artifact BLOB NULL,
    configuration_hash TEXT NOT NULL,
    environment TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT NULL,
    status TEXT NOT NULL DEFAULT 'Running'
);

CREATE INDEX IF NOT EXISTS idx_provider_runs_project_status
    ON provider_runs(project_id, status);
