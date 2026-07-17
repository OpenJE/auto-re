-- V9: Contradictions + Verification records
-- Contradictions track Open -> Investigating -> Resolved/Deferred transitions
-- for a subject entity + predicate, listing the competing hypotheses and
-- supporting evidence. Status transitions are enforced at the application
-- layer (ContradictionStatus::transition). Resolution is stored as JSON TEXT
-- and is NULL until the contradiction transitions to Resolved.
--
-- Verification records are generic (§15) — they record the result of a named
-- check against one of four subject types (Entity, Hypothesis, Artifact,
-- GenerationTarget). The subject is stored via a discriminator column
-- (subject_kind TEXT) + subject_id BLOB. No AUTOINCREMENT, no DEFAULT uuid().
-- IDs are application-assigned UUIDv7 BLOBs.

CREATE TABLE IF NOT EXISTS contradictions (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    subject BLOB NOT NULL REFERENCES semantic_entities(id),
    predicate TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '[]',
    hypotheses TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL,
    resolution TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_contradictions_subject_status
    ON contradictions(subject, status);

CREATE INDEX IF NOT EXISTS idx_contradictions_project
    ON contradictions(project_id);

CREATE TABLE IF NOT EXISTS verification_records (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    subject_kind TEXT NOT NULL,
    subject_id BLOB NOT NULL,
    check_kind TEXT NOT NULL,
    state TEXT NOT NULL,
    provider_run BLOB NULL REFERENCES provider_runs(id),
    evidence TEXT NOT NULL DEFAULT '[]',
    details TEXT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_verification_subject_check
    ON verification_records(subject_kind, subject_id, check_kind);

CREATE INDEX IF NOT EXISTS idx_verification_project
    ON verification_records(project_id);
