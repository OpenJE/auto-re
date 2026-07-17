-- V1: Initial schema for auto-re M1
-- All tables use IF NOT EXISTS for idempotent migration.
-- UUIDs are stored as TEXT (36-char canonical form).
-- Complex domain values (JSON enums, arrays) are stored as TEXT (JSON-encoded).

-- Campaigns: a coordinated set of analysis tasks.
CREATE TABLE IF NOT EXISTS campaigns (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'Pending'
);

-- Binary revisions: a specific version of a binary under analysis.
CREATE TABLE IF NOT EXISTS binary_revisions (
    id TEXT PRIMARY KEY NOT NULL,
    binary_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    content_hash TEXT
);

-- Modules: compilation units within a binary revision.
CREATE TABLE IF NOT EXISTS modules (
    id TEXT PRIMARY KEY NOT NULL,
    binary_revision_id TEXT NOT NULL REFERENCES binary_revisions(id),
    name TEXT NOT NULL,
    entry_address INTEGER
);

-- Functions: individual functions within a module.
CREATE TABLE IF NOT EXISTS functions (
    id TEXT PRIMARY KEY NOT NULL,
    binary_revision_id TEXT NOT NULL REFERENCES binary_revisions(id),
    module_id TEXT NOT NULL REFERENCES modules(id),
    entry_address INTEGER NOT NULL,
    current_name TEXT NOT NULL,
    backend_name TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    control_flow_hash TEXT,
    locked INTEGER NOT NULL DEFAULT 0,
    analysis_revision INTEGER NOT NULL DEFAULT 0
);

-- Tasks: atomic units of work within a campaign.
-- `kind`, `subject`, `required_capabilities`, and `dependencies` are JSON-encoded.
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY NOT NULL,
    campaign_id TEXT NOT NULL REFERENCES campaigns(id),
    kind TEXT NOT NULL,
    subject TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'Pending',
    priority INTEGER NOT NULL DEFAULT 0,
    required_capabilities TEXT NOT NULL DEFAULT '{}',
    dependencies TEXT NOT NULL DEFAULT '[]',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    maximum_attempts INTEGER NOT NULL DEFAULT 3,
    preferred_worker TEXT,
    preferred_model_class TEXT,
    input_revision INTEGER NOT NULL DEFAULT 0,
    error_message TEXT
);

-- Claims: assertions about binary entities.
-- `subject`, `predicate`, `value`, `evidence`, and `dependencies` are JSON-encoded.
CREATE TABLE IF NOT EXISTS claims (
    id TEXT PRIMARY KEY NOT NULL,
    subject TEXT NOT NULL,
    predicate TEXT NOT NULL,
    value TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'Proposed',
    confidence REAL NOT NULL,
    provenance TEXT NOT NULL,
    evidence TEXT NOT NULL DEFAULT '[]',
    dependencies TEXT NOT NULL DEFAULT '[]'
);

-- Evidence: data supporting or refuting claims.
-- `entity` and `location` are JSON-encoded when present.
CREATE TABLE IF NOT EXISTS evidences (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    artifact TEXT,
    entity TEXT,
    location TEXT,
    provenance TEXT NOT NULL
);

-- Leases: exclusive worker locks on tasks (one per task).
CREATE TABLE IF NOT EXISTS leases (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(id),
    worker_id TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

-- Artifacts: content-addressed blobs produced during analysis.
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    content_hash TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    mime_type TEXT
);

-- Indexes for common query patterns.
CREATE INDEX IF NOT EXISTS idx_tasks_campaign_state ON tasks(campaign_id, state);
CREATE INDEX IF NOT EXISTS idx_tasks_state_priority ON tasks(state, priority DESC);
CREATE INDEX IF NOT EXISTS idx_functions_binary_revision ON functions(binary_revision_id);
CREATE INDEX IF NOT EXISTS idx_functions_module ON functions(module_id);
CREATE INDEX IF NOT EXISTS idx_claims_state ON claims(state);
CREATE INDEX IF NOT EXISTS idx_leases_expires ON leases(expires_at);
