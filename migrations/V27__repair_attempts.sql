-- V27: Repair attempts (Stage 1)
-- Records of automatic or manual repair actions applied to a work item.
-- Each attempt carries a sequential number, the repair kind, who/what
-- sponsored the attempt, and its lifecycle state.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS repair_attempts (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    build_attempt_id BLOB NULL,
    attempt_seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    sponsor TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    ended_at TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_repair_attempts_work_item
    ON repair_attempts(work_item_id);

CREATE INDEX IF NOT EXISTS idx_repair_attempts_seq
    ON repair_attempts(work_item_id, attempt_seq);
