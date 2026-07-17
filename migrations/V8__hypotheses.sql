-- V8: Hypotheses with confidence-independent status and supersession
-- Hypotheses are proposed explanations for semantic entities within a project.
-- Status transitions are enforced at the application layer (HypothesisStatus::transition).
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT, no DEFAULT uuid().

CREATE TABLE IF NOT EXISTS hypotheses (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    subject BLOB NOT NULL REFERENCES semantic_entities(id),
    predicate TEXT NOT NULL,
    candidate TEXT NOT NULL,
    supporting_evidence TEXT NOT NULL DEFAULT '[]',
    contradicting_evidence TEXT NOT NULL DEFAULT '[]',
    derived_from TEXT NOT NULL DEFAULT '[]',
    confidence TEXT NOT NULL,
    status TEXT NOT NULL,
    superseded_by BLOB NULL REFERENCES hypotheses(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_hypotheses_subject_predicate_status
    ON hypotheses(subject, predicate, status);

CREATE INDEX IF NOT EXISTS idx_hypotheses_project
    ON hypotheses(project_id);
