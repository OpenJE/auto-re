-- V15: Reconstruction work items (Stage 1)
-- Atomic units of reconstruction work within a campaign. Each item targets
-- a specific entity or address range and tracks scheduling state.
-- required_capabilities is JSON-encoded TEXT.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS reconstruction_work_items (
    id BLOB PRIMARY KEY NOT NULL,
    campaign_id BLOB NOT NULL REFERENCES reconstruction_campaigns(id),
    kind TEXT NOT NULL,
    subject_entity_id BLOB NULL,
    subject_address INTEGER NULL,
    subject_kind TEXT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    dependencies_summary TEXT NULL,
    required_capabilities TEXT NOT NULL DEFAULT '{}',
    preferred_worker BLOB NULL,
    preferred_model_class TEXT NULL,
    maximum_attempts INTEGER NOT NULL DEFAULT 3,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    input_revision INTEGER NOT NULL DEFAULT 0,
    fingerprint BLOB NULL,
    blocked_reason TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_recon_work_items_campaign_state
    ON reconstruction_work_items(campaign_id, state);

CREATE INDEX IF NOT EXISTS idx_recon_work_items_state_priority
    ON reconstruction_work_items(state, priority DESC);

CREATE INDEX IF NOT EXISTS idx_recon_work_items_subject_entity
    ON reconstruction_work_items(subject_entity_id)
    WHERE subject_entity_id IS NOT NULL;
