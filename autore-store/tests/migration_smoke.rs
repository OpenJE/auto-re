#[test]
fn migration_runs_v1_then_v2() {
    let db = autore_store::Database::open_in_memory()
        .expect("in-memory database should open and migrate");

    let conn = db.connection().unwrap();

    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type='table' AND name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let v2_tables = [
        "projects",
        "stage0_artifacts",
        "semantic_entities",
        "providers",
        "provider_runs",
        "provider_entity_aliases",
        "native_artifacts",
        "evidence_records",
        "evidence_lifecycle_events",
        "hypotheses",
        "contradictions",
        "verification_records",
        "operations",
        "progress_updates",
        "cancellation_requests",
        "project_events",
    ];
    for v2_table in v2_tables {
        assert!(
            tables.contains(&v2_table.to_string()),
            "V2 table '{v2_table}' should exist after migration"
        );
    }

    let obsolete_v1 = [
        "campaigns",
        "binary_revisions",
        "modules",
        "functions",
        "tasks",
        "claims",
        "evidences",
        "leases",
    ];
    for v1_table in obsolete_v1 {
        assert!(
            !tables.contains(&v1_table.to_string()),
            "obsolete V1 table '{v1_table}' should be dropped after migration"
        );
    }

    assert!(
        tables.contains(&"artifacts".to_string()),
        "retained V1 table 'artifacts' should still exist"
    );
}
