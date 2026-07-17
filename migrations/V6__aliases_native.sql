-- V6: Provider entity aliases and native artifacts
-- Bridges provider-specific identifiers to canonical semantic entities,
-- and stores native-format output artifacts from analysis providers.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS provider_entity_aliases (
    provider_run BLOB NOT NULL REFERENCES provider_runs(id),
    provider_kind TEXT NOT NULL,
    provider_identifier TEXT NOT NULL,
    entity BLOB NOT NULL REFERENCES semantic_entities(id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_aliases_provider_identifier
    ON provider_entity_aliases(provider_run, provider_identifier);

CREATE INDEX IF NOT EXISTS idx_aliases_entity
    ON provider_entity_aliases(entity);

CREATE TABLE IF NOT EXISTS native_artifacts (
    id BLOB PRIMARY KEY NOT NULL,
    provider_run BLOB NOT NULL REFERENCES provider_runs(id),
    artifact BLOB NOT NULL REFERENCES stage0_artifacts(id),
    format TEXT NOT NULL,
    subject_entities TEXT NOT NULL DEFAULT '[]',
    description TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_native_artifacts_provider_run
    ON native_artifacts(provider_run);
