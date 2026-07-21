//! Stage 1 migration tests (V14..V23).
//!
//! Verifies that the additive reconstruction migrations apply cleanly,
//! are idempotent, and are safe to roll back within a transaction.

/// All Stage 1 tables introduced by V14..V23.
const STAGE1_TABLES: &[&str] = &[
    "reconstruction_campaigns",
    "reconstruction_work_items",
    "work_dependencies",
    "work_fingerprints",
    "work_leases",
    "provider_installations",
    "provider_instances",
    "stage1_provider_runs",
    "capability_descriptors",
    "native_snapshots",
    "dynamic_observations",
];

/// Total number of migrations (V1..V23).
const EXPECTED_MIGRATION_COUNT: i64 = 23;

fn table_exists(conn: &rusqlite::Connection, name: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |row| row.get(0),
        )
        .unwrap();
    count > 0
}

#[test]
fn migrations_apply_clean() {
    let db = autore_store::Database::open_in_memory()
        .expect("in-memory database should open and apply V1..V23");

    let conn = db.connection().unwrap();

    // refinery_schema_history should contain exactly 23 entries.
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM refinery_schema_history", [], |row| {
            row.get(0)
        })
        .expect("query refinery history");
    assert_eq!(
        count, EXPECTED_MIGRATION_COUNT,
        "refinery_schema_history should have {EXPECTED_MIGRATION_COUNT} entries, got {count}"
    );

    // All Stage 1 tables must exist.
    for table in STAGE1_TABLES {
        assert!(
            table_exists(&conn, table),
            "Stage 1 table '{table}' should exist after migration"
        );
    }
}

#[test]
fn migrations_v14_to_v23_idempotent() {
    let db = autore_store::Database::open_in_memory().expect("initial migration should succeed");

    // Re-running migrations should be a no-op — no errors, no changes.
    db.migrate()
        .expect("second migration run should succeed without errors");

    let conn = db.connection().unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM refinery_schema_history", [], |row| {
            row.get(0)
        })
        .expect("query refinery history after re-run");
    assert_eq!(
        count, EXPECTED_MIGRATION_COUNT,
        "history should not duplicate after idempotent re-run"
    );
}

#[test]
fn migrations_v14_to_v23_rollback_safe() {
    // Open a raw in-memory connection (no auto-migration).
    let conn = rusqlite::Connection::open_in_memory().expect("open raw connection");

    // Apply V1..V13 base schema outside any transaction.
    let base_sql: &[&str] = &[
        include_str!("../../migrations/V1__initial_schema.sql"),
        include_str!("../../migrations/V2__stage0_projects.sql"),
        include_str!("../../migrations/V3__stage0_artifacts.sql"),
        include_str!("../../migrations/V4__semantic_entities.sql"),
        include_str!("../../migrations/V5__providers.sql"),
        include_str!("../../migrations/V6__aliases_native.sql"),
        include_str!("../../migrations/V7__evidence.sql"),
        include_str!("../../migrations/V8__hypotheses.sql"),
        include_str!("../../migrations/V9__contradictions_verification.sql"),
        include_str!("../../migrations/V10__operations.sql"),
        include_str!("../../migrations/V11__events.sql"),
        include_str!("../../migrations/V12__drop_obsolete_v1.sql"),
        include_str!("../../migrations/V13__derived_indexes.sql"),
    ];
    for sql in base_sql {
        conn.execute_batch(sql)
            .expect("base migration should apply cleanly");
    }

    // Verify Stage 1 tables do NOT exist yet.
    for table in STAGE1_TABLES {
        assert!(
            !table_exists(&conn, table),
            "Stage 1 table '{table}' should not exist before V14..V23"
        );
    }

    // Begin a transaction, apply V14..V23, then ROLLBACK.
    conn.execute("BEGIN IMMEDIATE", [])
        .expect("begin transaction");

    let stage1_sql: &[&str] = &[
        include_str!("../../migrations/V14__reconstruction_campaigns.sql"),
        include_str!("../../migrations/V15__work_items.sql"),
        include_str!("../../migrations/V16__work_dependencies.sql"),
        include_str!("../../migrations/V17__work_fingerprints.sql"),
        include_str!("../../migrations/V18__work_leases.sql"),
        include_str!("../../migrations/V19__provider_installations.sql"),
        include_str!("../../migrations/V20__provider_instances.sql"),
        include_str!("../../migrations/V21__provider_runs.sql"),
        include_str!("../../migrations/V22__capability_descriptors.sql"),
        include_str!("../../migrations/V23__native_snapshots_and_dynamic_observations.sql"),
    ];
    for sql in stage1_sql {
        conn.execute_batch(sql)
            .expect("Stage 1 migration should apply within transaction");
    }

    // Verify tables exist within the transaction before rollback.
    for table in STAGE1_TABLES {
        assert!(
            table_exists(&conn, table),
            "Stage 1 table '{table}' should exist within transaction"
        );
    }

    // Rollback — all V14..V23 changes should be undone.
    conn.execute("ROLLBACK", [])
        .expect("rollback should succeed");

    // Verify Stage 1 tables are gone after rollback.
    for table in STAGE1_TABLES {
        assert!(
            !table_exists(&conn, table),
            "Stage 1 table '{table}' should not exist after ROLLBACK"
        );
    }
}
