-- V3: Stage 0 artifact storage table
-- Content-addressed artifacts (managed blobs + external file references).
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS stage0_artifacts (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    kind TEXT NOT NULL,
    hash_algorithm TEXT NOT NULL,
    hash_digest BLOB NOT NULL,
    size INTEGER NOT NULL,
    storage_kind TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_stage0_artifacts_project
    ON stage0_artifacts(project_id);
CREATE INDEX IF NOT EXISTS idx_stage0_artifacts_hash
    ON stage0_artifacts(hash_algorithm, hash_digest);
