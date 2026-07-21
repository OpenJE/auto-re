-- V22: Capability descriptors (Stage 1)
-- Declared capabilities of a provider installation, with per-capability
-- request/response schemas and concurrency limits.
-- Schemas are JSON-encoded TEXT columns.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS capability_descriptors (
    id BLOB PRIMARY KEY NOT NULL,
    installation_id BLOB NOT NULL REFERENCES provider_installations(id),
    capability_id TEXT NOT NULL,
    version TEXT NOT NULL,
    name TEXT NOT NULL,
    request_schema TEXT NOT NULL DEFAULT '{}',
    response_schema TEXT NOT NULL DEFAULT '{}',
    max_concurrency INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_capability_descriptors_installation
    ON capability_descriptors(installation_id);

CREATE INDEX IF NOT EXISTS idx_capability_descriptors_capability
    ON capability_descriptors(capability_id);
