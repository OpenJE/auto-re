-- V4: Semantic entities table
-- Stores discovered entities (functions, types, globals, strings, etc.)
-- with optional stable keys for cross-revision identity.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS semantic_entities (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    kind TEXT NOT NULL,
    stable_key TEXT NULL,
    display_name TEXT NULL,
    created_at TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_entities_project_kind
    ON semantic_entities(project_id, kind);

CREATE INDEX IF NOT EXISTS idx_entities_stable_key
    ON semantic_entities(stable_key)
    WHERE stable_key IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_entities_project_stable_key
    ON semantic_entities(project_id, stable_key)
    WHERE stable_key IS NOT NULL;
