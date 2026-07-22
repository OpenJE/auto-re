-- V26: Blocked reasons (Stage 1)
-- Records why a work item is currently blocked, with structured detail
-- (JSON-encoded TEXT) capturing the diagnostic or condition that caused
-- the block.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS blocked_reasons (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    reason_kind TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_blocked_reasons_work_item
    ON blocked_reasons(work_item_id);

CREATE INDEX IF NOT EXISTS idx_blocked_reasons_kind
    ON blocked_reasons(reason_kind);
