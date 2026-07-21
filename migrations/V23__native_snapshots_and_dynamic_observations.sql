-- V23: Native snapshots and dynamic observations (Stage 1)
-- Native snapshots capture point-in-time state of entities as produced by
-- analysis providers. Dynamic observations record runtime findings that
-- may or may not be tied to a specific work item.
-- payload is JSON-encoded TEXT.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS native_snapshots (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NOT NULL REFERENCES reconstruction_work_items(id),
    entity_id BLOB NOT NULL REFERENCES semantic_entities(id),
    kind TEXT NOT NULL,
    artifact_id BLOB NOT NULL REFERENCES stage0_artifacts(id),
    data_hash BLOB NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_native_snapshots_work_item
    ON native_snapshots(work_item_id);

CREATE INDEX IF NOT EXISTS idx_native_snapshots_entity
    ON native_snapshots(entity_id);

CREATE TABLE IF NOT EXISTS dynamic_observations (
    id BLOB PRIMARY KEY NOT NULL,
    work_item_id BLOB NULL REFERENCES reconstruction_work_items(id),
    entity_id BLOB NOT NULL REFERENCES semantic_entities(id),
    observation_kind TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    observed_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dynamic_observations_work_item
    ON dynamic_observations(work_item_id)
    WHERE work_item_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_dynamic_observations_entity
    ON dynamic_observations(entity_id);

CREATE INDEX IF NOT EXISTS idx_dynamic_observations_kind
    ON dynamic_observations(observation_kind);
