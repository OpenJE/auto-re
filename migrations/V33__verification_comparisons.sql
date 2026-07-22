-- V33: Verification comparisons (Stage 1)
-- Records the outcome of comparing a candidate run against a verification
-- scenario baseline. Counts are bucketed by comparison kind: equal,
-- equivalent, different, inconclusive, not observed, and failed.
-- Links to the candidate run and an optional evidence artifact.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS verification_comparisons (
    id BLOB PRIMARY KEY NOT NULL,
    scenario_id BLOB NOT NULL,
    candidate_run_id BLOB NULL,
    comparison_kind TEXT,
    equal_count INT,
    equivalent_count INT,
    different_count INT,
    inconclusive_count INT,
    not_observed_count INT,
    failed_count INT,
    evidence_artifact_id BLOB NULL,
    generated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_verification_comparisons_scenario
    ON verification_comparisons(scenario_id);

CREATE INDEX IF NOT EXISTS idx_verification_comparisons_kind
    ON verification_comparisons(comparison_kind);
