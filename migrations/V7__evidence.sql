-- V7: Evidence records and lifecycle events
-- Append-only evidence records supporting or refuting semantic entities.
-- Lifecycle events track state changes without mutating the original record.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS evidence_records (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    subject BLOB NOT NULL REFERENCES semantic_entities(id),
    predicate TEXT NOT NULL,
    value TEXT NOT NULL,
    derivation TEXT NOT NULL,
    provider_run BLOB NULL REFERENCES provider_runs(id),
    native_artifacts TEXT NOT NULL DEFAULT '[]',
    assumptions TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_evidence_subject_predicate
    ON evidence_records(subject, predicate);

CREATE INDEX IF NOT EXISTS idx_evidence_provider_run
    ON evidence_records(provider_run);

CREATE TABLE IF NOT EXISTS evidence_lifecycle_events (
    evidence BLOB NOT NULL REFERENCES evidence_records(id),
    timestamp TEXT NOT NULL,
    state TEXT NOT NULL,
    reason TEXT NULL,
    caused_by BLOB NULL REFERENCES evidence_records(id)
);

CREATE INDEX IF NOT EXISTS idx_evidence_lifecycle_evidence_time
    ON evidence_lifecycle_events(evidence, timestamp);
