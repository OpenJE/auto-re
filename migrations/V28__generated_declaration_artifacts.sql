-- V28: Generated declaration artifacts (Stage 1)
-- Links a generated source mapping to individual declaration artifacts
-- (header prototypes, type declarations, etc.) observed in generated output.
-- Each row ties a kind-tagged declaration to its stored artifact blob and
-- the content hash at the time of observation.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS generated_declaration_artifacts (
    id BLOB PRIMARY KEY NOT NULL,
    source_mapping_id BLOB NOT NULL,
    kind TEXT NOT NULL,
    declaration_artifact_id BLOB NOT NULL,
    last_change_hash BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gen_decl_art_source_mapping
    ON generated_declaration_artifacts(source_mapping_id);
