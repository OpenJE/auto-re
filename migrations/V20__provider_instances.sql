-- V20: Provider instances (Stage 1)
-- Running instances of installed providers. Tracks lifecycle from start
-- through optional termination with exit code.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS provider_instances (
    id BLOB PRIMARY KEY NOT NULL,
    installation_id BLOB NOT NULL REFERENCES provider_installations(id),
    instance_id BLOB NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT NULL,
    exit_code INTEGER NULL
);

CREATE INDEX IF NOT EXISTS idx_provider_instances_installation
    ON provider_instances(installation_id);

CREATE INDEX IF NOT EXISTS idx_provider_instances_status
    ON provider_instances(status);
