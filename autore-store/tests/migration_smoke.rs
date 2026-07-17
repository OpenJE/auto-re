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

    for v1_table in [
        "campaigns",
        "binary_revisions",
        "modules",
        "functions",
        "tasks",
        "claims",
        "evidences",
        "leases",
        "artifacts",
    ] {
        assert!(
            tables.contains(&v1_table.to_string()),
            "V1 table '{v1_table}' should exist after migration"
        );
    }

    assert!(
        tables.contains(&"projects".to_string()),
        "V2 table 'projects' should exist after migration"
    );
}
