-- V31: Build diagnostics (Stage 1)
-- Structured diagnostics extracted from a build attempt: compiler errors,
-- warnings, and hints. Each row captures the diagnostic code, severity,
-- source location, human-readable message, and optional classification
-- fields for downstream repair-work creation.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS build_diagnostics (
    id BLOB PRIMARY KEY NOT NULL,
    build_attempt_id BLOB NOT NULL,
    diagnostic_code TEXT,
    severity TEXT,
    file_path TEXT,
    line INT NULL,
    column INT NULL,
    related_entity_id BLOB NULL,
    message TEXT,
    candidate_cause TEXT NULL,
    suggested_work_kind TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_build_diagnostics_attempt
    ON build_diagnostics(build_attempt_id);

CREATE INDEX IF NOT EXISTS idx_build_diagnostics_severity
    ON build_diagnostics(severity);
