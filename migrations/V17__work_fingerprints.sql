-- V17: Work fingerprints (Stage 1)
-- Content-based fingerprints for reconstruction work items, enabling
-- deduplication and change detection. computed_from is JSON-encoded TEXT.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS work_fingerprints (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    fingerprint BLOB NOT NULL,
    computed_from TEXT NOT NULL DEFAULT '{}',
    computed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_fingerprints_item
    ON work_fingerprints(work_item_id);

CREATE INDEX IF NOT EXISTS idx_work_fingerprints_hash
    ON work_fingerprints(fingerprint);
