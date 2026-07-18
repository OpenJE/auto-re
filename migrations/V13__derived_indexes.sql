-- V13: Derived Stage 0 indexes.
-- These tables are reconstructed from canonical records only and never mutate
-- the source-of-truth tables. They live in a dedicated `derived_*` namespace.
-- All IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT, no DEFAULT uuid().

CREATE TABLE IF NOT EXISTS derived_project_summary (
    project_id BLOB PRIMARY KEY NOT NULL REFERENCES projects(id),
    artifact_count INTEGER NOT NULL DEFAULT 0,
    entity_count INTEGER NOT NULL DEFAULT 0,
    provider_run_count INTEGER NOT NULL DEFAULT 0,
    native_artifact_count INTEGER NOT NULL DEFAULT 0,
    evidence_count INTEGER NOT NULL DEFAULT 0,
    hypothesis_count INTEGER NOT NULL DEFAULT 0,
    contradiction_count INTEGER NOT NULL DEFAULT 0,
    verification_count INTEGER NOT NULL DEFAULT 0,
    operation_count INTEGER NOT NULL DEFAULT 0,
    event_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS derived_hypothesis_progress (
    project_id BLOB NOT NULL REFERENCES projects(id),
    status TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (project_id, status)
);

CREATE TABLE IF NOT EXISTS derived_evidence_progress (
    project_id BLOB NOT NULL REFERENCES projects(id),
    state TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (project_id, state)
);

CREATE TABLE IF NOT EXISTS derived_reverse_references (
    project_id BLOB NOT NULL REFERENCES projects(id),
    subject_kind TEXT NOT NULL,
    subject_id BLOB NOT NULL,
    reference_kind TEXT NOT NULL,
    reference_id BLOB NOT NULL,
    PRIMARY KEY (project_id, subject_kind, subject_id, reference_kind, reference_id)
);

CREATE INDEX IF NOT EXISTS idx_derived_reverse_subject
    ON derived_reverse_references(project_id, subject_kind, subject_id);
