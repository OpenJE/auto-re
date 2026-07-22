-- V24: Conflict records (Stage 1)
-- Records of observed contradictions between two analysis observations
-- (evidence sources). Tracks resolution status and the conflicting evidence
-- payloads as JSON-encoded TEXT columns.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS conflict_records (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    kind_a TEXT NOT NULL,
    kind_b TEXT NOT NULL,
    evidence_a TEXT NOT NULL DEFAULT '{}',
    evidence_b TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_conflict_records_work_item
    ON conflict_records(work_item_id);

CREATE INDEX IF NOT EXISTS idx_conflict_records_status
    ON conflict_records(status);
