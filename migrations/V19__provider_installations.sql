-- V19: Provider installations (Stage 1)
-- Records of installed analysis provider packages with their capabilities,
-- concurrency limits, and configuration schemas. Capabilities and schemas
-- are JSON-encoded TEXT columns.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS provider_installations (
    id BLOB PRIMARY KEY NOT NULL,
    package_id TEXT NOT NULL,
    version TEXT NOT NULL,
    content_hash BLOB NOT NULL,
    capabilities TEXT NOT NULL DEFAULT '[]',
    max_concurrency_per_cap TEXT NOT NULL DEFAULT '{}',
    configuration_schema TEXT NOT NULL DEFAULT '{}',
    root_path TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_provider_installations_package
    ON provider_installations(package_id, version);
