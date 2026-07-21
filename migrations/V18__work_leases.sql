-- V18: Work leases (Stage 1)
-- Exclusive worker locks on reconstruction work items, extending the
-- lease-based scheduling pattern from V1. version enables optimistic
-- concurrency control for lease renewal.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS work_leases (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    owner TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_leases_item
    ON work_leases(work_item_id);

CREATE INDEX IF NOT EXISTS idx_work_leases_expires
    ON work_leases(expires_at);
