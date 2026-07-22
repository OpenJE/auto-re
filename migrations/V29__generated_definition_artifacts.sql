-- V29: Generated definition artifacts (Stage 1)
-- Links a generated source mapping to individual definition artifacts
-- (function bodies, type implementations, etc.) observed in generated output.
-- Each row ties a kind-tagged definition to its stored artifact blob.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS generated_definition_artifacts (
    id BLOB PRIMARY KEY NOT NULL,
    source_mapping_id BLOB NOT NULL,
    kind TEXT NOT NULL,
    definition_artifact_id BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gen_defn_art_source_mapping
    ON generated_definition_artifacts(source_mapping_id);
