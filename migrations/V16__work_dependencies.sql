-- V16: Work dependencies (Stage 1)
-- Directed edges between reconstruction work items forming a DAG.
-- edge_kind distinguishes dependency types (e.g., "requires", "blocks").
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS work_dependencies (
    id BLOB PRIMARY KEY NOT NULL,
    predecessor BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    successor BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    edge_kind TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_work_deps_predecessor
    ON work_dependencies(predecessor);

CREATE INDEX IF NOT EXISTS idx_work_deps_successor
    ON work_dependencies(successor);
