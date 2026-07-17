-- V2: Stage 0 projects table
-- IDs are generated in the application layer and stored as BLOB (16-byte UUIDv7).
-- No AUTOINCREMENT, no DEFAULT uuid() — all IDs are application-assigned.

CREATE TABLE IF NOT EXISTS projects (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name);
CREATE INDEX IF NOT EXISTS idx_projects_created_at ON projects(created_at);
