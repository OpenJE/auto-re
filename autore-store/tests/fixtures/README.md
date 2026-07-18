# `autore-store` V1 Fixture Database

This directory contains a committed, immutable SQLite database that represents a
real V1 (M1) project. It is used by the `migration_fixture` integration test to
verify that `MigrationService::migrate_from_v1` correctly upgrades an on-disk V1
project to the Stage 0 V2 schema.

## File

- `v1_project.sqlite3` — a V1-only database created from `migrations/V1__initial_schema.sql`
  plus one sample row in each of `campaigns`, `tasks`, and `claims`.

## Regenerating the fixture

The fixture must be byte-for-byte reproducible only in content; the exact UUIDs
and insertion order will vary between regenerations. The committed file should
never be modified in-place; if it needs to change, delete it and regenerate from
scratch.

### Option 1: using the `sqlite3` CLI

From the workspace root:

```bash
# 1. Remove the old fixture.
rm autore-store/tests/fixtures/v1_project.sqlite3

# 2. Create a fresh SQLite file and apply the V1 schema.
sqlite3 autore-store/tests/fixtures/v1_project.sqlite3 \
    < migrations/V1__initial_schema.sql

# 3. Insert sample Campaign, Task, and Claim rows.
sqlite3 autore-store/tests/fixtures/v1_project.sqlite3 <<'SQL'
INSERT INTO campaigns (id, name, state)
VALUES ('22a20172-72b1-4d8e-baa4-90a0323f3a43', 'fixture-campaign', 'Pending');

INSERT INTO tasks (id, campaign_id, kind, subject, state, priority,
                 required_capabilities, dependencies, attempt_count,
                 maximum_attempts, preferred_worker, preferred_model_class,
                 input_revision, error_message)
VALUES ('3d1e8e48-e17b-4ce9-8552-9eefc4c05d8f',
        '22a20172-72b1-4d8e-baa4-90a0323f3a43',
        '"AnalyzeFunction"',
        '{"function_id":"func-001"}',
        'Pending', 0, '{}', '[]', 0, 3, NULL, NULL, 0, NULL);

INSERT INTO claims (id, subject, predicate, value, state, confidence,
                    provenance, evidence, dependencies)
VALUES ('e883d692-2ca2-4dfc-a351-d615288bb17d',
        '{"entity_id":"entity-001"}',
        'is.function.entry',
        '{"type":"string","value":"0x401000"}',
        'Proposed', 0.75, 'manual-fixture', '[]', '[]');
SQL

# 4. Verify the fixture.
sqlite3 autore-store/tests/fixtures/v1_project.sqlite3 \
    "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;"
```

### Option 2: using the Python helper

If the `sqlite3` CLI is not available, use the Python script that produced the
committed fixture:

```bash
python3 /tmp/generate_v1_fixture.py
```

The script applies `migrations/V1__initial_schema.sql` and inserts the same
Campaign, Task, and Claim rows shown above.

## Immutability contract

`cargo test` copies this file into a `tempfile::TempDir` before migrating it.
The committed fixture is never opened for write by the test suite.
