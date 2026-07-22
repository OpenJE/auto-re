-- V32: Verification scenarios (Stage 1)
-- Defines a named verification scenario (unit, integration, system, etc.)
-- with its configuration, the original execution trace, and any captured
-- observations used as the comparison baseline.
-- JSON payloads are stored as TEXT columns.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS verification_scenarios (
    id BLOB PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    level TEXT NOT NULL,
    scenario_blob TEXT,
    original_execution_blob TEXT NULL,
    captured_observations_blob TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_verification_scenarios_level
    ON verification_scenarios(level);
