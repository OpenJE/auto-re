-- V25: Generated source mappings (Stage 1)
-- Links a generated entity to its declaration and definition artifact locations,
-- the generation operation that produced it, and optional build/verification
-- references for downstream traceability.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS generated_source_mappings (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    entity_id BLOB NOT NULL REFERENCES semantic_entities(id),
    declaration_artifact_id BLOB NOT NULL,
    definition_artifact_id BLOB NOT NULL,
    generation_operation_id BLOB NOT NULL REFERENCES operations(id),
    build_attempt_id BLOB NULL,
    verification_comparison_id BLOB NULL,
    last_change_hash BLOB NOT NULL,
    last_change_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gen_source_map_work_item
    ON generated_source_mappings(work_item_id);

CREATE INDEX IF NOT EXISTS idx_gen_source_map_entity
    ON generated_source_mappings(entity_id);

CREATE INDEX IF NOT EXISTS idx_gen_source_map_operation
    ON generated_source_mappings(generation_operation_id);
