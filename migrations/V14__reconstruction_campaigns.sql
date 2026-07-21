-- V14: Reconstruction campaigns (Stage 1)
-- Top-level campaigns coordinating reconstruction work items against a
-- binary artifact. Policies are JSON-encoded TEXT columns.
-- IDs are application-assigned UUIDv7 BLOBs — no AUTOINCREMENT.

CREATE TABLE IF NOT EXISTS reconstruction_campaigns (
    id BLOB PRIMARY KEY NOT NULL,
    project_id BLOB NOT NULL REFERENCES projects(id),
    binary_artifact_id BLOB NOT NULL REFERENCES stage0_artifacts(id),
    binary_revision_id BLOB NULL,
    output_target_id BLOB NULL,
    provider_policy TEXT NOT NULL DEFAULT '{}',
    model_policy TEXT NOT NULL DEFAULT '{}',
    build_policy TEXT NOT NULL DEFAULT '{}',
    verification_policy TEXT NOT NULL DEFAULT '{}',
    completion_policy TEXT NOT NULL DEFAULT '{}',
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_recon_campaigns_project
    ON reconstruction_campaigns(project_id);

CREATE INDEX IF NOT EXISTS idx_recon_campaigns_state
    ON reconstruction_campaigns(state);
