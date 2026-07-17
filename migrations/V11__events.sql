-- V11: Project Events
-- Append-only event stream per project. Each event has a monotonic
-- per-project sequence number (not AUTOINCREMENT). The global
-- project_event_id is a UUIDv7 BLOB assigned by the application.
-- No AUTOINCREMENT, no DEFAULT uuid().

CREATE TABLE IF NOT EXISTS project_events (
    project_event_id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    sequence INTEGER NOT NULL,
    kind TEXT NOT NULL,
    subject TEXT NULL,
    source TEXT NOT NULL,
    payload TEXT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (project_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_events_project_sequence
    ON project_events(project_id, sequence);
