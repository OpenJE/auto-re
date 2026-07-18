//! Derived Stage 0 state.
//!
//! `build_derived_state` reconstructs all Stage 0 derived indexes from canonical
//! records only. Derived tables are in the dedicated `derived_*` namespace and are
//! never written to by canonical store operations.

use std::collections::HashMap;

use autore_schema::ids::ProjectId;
use rusqlite::OptionalExtension;
use rusqlite::params;

use crate::Error;
use crate::storage::database::{Database, Transaction};

const REF_KIND_EVIDENCE: &str = "core.evidence.record";
const REF_KIND_HYPOTHESIS: &str = "core.hypothesis";
const REF_KIND_CONTRADICTION: &str = "core.contradiction";
const REF_KIND_VERIFICATION: &str = "core.verification.record";
const SUBJECT_KIND_ENTITY: &str = "Entity";

/// Rebuilds all derived indexes for `project_id` inside a single explicit
/// transaction. Canonical tables are never modified.
pub fn build_derived_state(db: &Database, project_id: ProjectId) -> crate::Result<()> {
    let txn = db.begin_transaction()?;
    build_derived_state_in_tx(&txn, project_id)?;
    txn.commit()
}

/// Rebuilds all derived indexes for `project_id` using the supplied transaction.
/// This is the variant used by `ApplicationService::with_event` so the rebuild
/// and the emitted event are atomic.
pub fn build_derived_state_in_tx(
    txn: &Transaction<'_>,
    project_id: ProjectId,
) -> crate::Result<()> {
    let conn = txn.conn();
    let pid = project_id.as_uuid().as_bytes().as_slice();

    // Clear any existing derived rows for the project. The canonical tables are
    // left untouched.
    conn.execute(
        "DELETE FROM derived_project_summary WHERE project_id = ?1",
        [pid],
    )
    .map_err(|e| Error::Database(e.to_string()))?;
    conn.execute(
        "DELETE FROM derived_hypothesis_progress WHERE project_id = ?1",
        [pid],
    )
    .map_err(|e| Error::Database(e.to_string()))?;
    conn.execute(
        "DELETE FROM derived_evidence_progress WHERE project_id = ?1",
        [pid],
    )
    .map_err(|e| Error::Database(e.to_string()))?;
    conn.execute(
        "DELETE FROM derived_reverse_references WHERE project_id = ?1",
        [pid],
    )
    .map_err(|e| Error::Database(e.to_string()))?;

    // Project summary counts.
    let artifact_count: i64 = count_project_rows(conn, "stage0_artifacts", pid)?;
    let entity_count: i64 = count_project_rows(conn, "semantic_entities", pid)?;
    let provider_run_count: i64 = count_project_rows(conn, "provider_runs", pid)?;
    let native_artifact_count: i64 = count_native_artifacts(conn, pid)?;
    let evidence_count: i64 = count_project_rows(conn, "evidence_records", pid)?;
    let hypothesis_count: i64 = count_project_rows(conn, "hypotheses", pid)?;
    let contradiction_count: i64 = count_project_rows(conn, "contradictions", pid)?;
    let verification_count: i64 = count_project_rows(conn, "verification_records", pid)?;
    let operation_count: i64 = count_project_rows(conn, "operations", pid)?;
    let event_count: i64 = count_project_rows(conn, "project_events", pid)?;

    conn.execute(
        "INSERT INTO derived_project_summary \
         (project_id, artifact_count, entity_count, provider_run_count, \
          native_artifact_count, evidence_count, hypothesis_count, \
          contradiction_count, verification_count, operation_count, event_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            pid,
            artifact_count,
            entity_count,
            provider_run_count,
            native_artifact_count,
            evidence_count,
            hypothesis_count,
            contradiction_count,
            verification_count,
            operation_count,
            event_count,
        ],
    )
    .map_err(|e| Error::Database(e.to_string()))?;

    // Hypothesis progress aggregates.
    let mut stmt = conn
        .prepare("SELECT status, COUNT(*) FROM hypotheses WHERE project_id = ?1 GROUP BY status")
        .map_err(|e| Error::Database(e.to_string()))?;
    let rows = stmt
        .query_map([pid], |row| {
            let status: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((status, count))
        })
        .map_err(|e| Error::Database(e.to_string()))?;
    for row in rows {
        let (status, count) = row.map_err(|e| Error::Database(e.to_string()))?;
        conn.execute(
            "INSERT INTO derived_hypothesis_progress (project_id, status, count) \
             VALUES (?1, ?2, ?3)",
            rusqlite::params![pid, status, count],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    }

    // Evidence progress aggregates: the current state of each evidence record is
    // its latest lifecycle event, or `Active` if no lifecycle events exist.
    let mut evidence_states: HashMap<String, i64> = HashMap::new();
    let mut stmt = conn
        .prepare("SELECT id FROM evidence_records WHERE project_id = ?1")
        .map_err(|e| Error::Database(e.to_string()))?;
    let evidence_ids: Vec<Vec<u8>> = stmt
        .query_map([pid], |row| row.get(0))
        .map_err(|e| Error::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Database(e.to_string()))?;
    drop(stmt);

    for evidence_id in evidence_ids {
        let state: Option<String> = conn
            .query_row(
                "SELECT state FROM evidence_lifecycle_events \
                 WHERE evidence = ?1 ORDER BY timestamp DESC, rowid DESC LIMIT 1",
                [&evidence_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| Error::Database(e.to_string()))?;
        let state = state.unwrap_or_else(|| "Active".to_string());
        *evidence_states.entry(state).or_insert(0) += 1;
    }

    for (state, count) in evidence_states {
        conn.execute(
            "INSERT INTO derived_evidence_progress (project_id, state, count) \
             VALUES (?1, ?2, ?3)",
            rusqlite::params![pid, state, count],
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    }

    // Reverse references: subject -> records that reference it.
    // Evidence records reference an entity subject.
    let mut stmt = conn
        .prepare("SELECT id, subject FROM evidence_records WHERE project_id = ?1")
        .map_err(|e| Error::Database(e.to_string()))?;
    let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
        .query_map([pid], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| Error::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Database(e.to_string()))?;
    drop(stmt);
    for (reference_id, subject_id) in rows {
        insert_reverse_reference(
            conn,
            pid,
            SUBJECT_KIND_ENTITY,
            &subject_id,
            REF_KIND_EVIDENCE,
            &reference_id,
        )?;
    }

    // Hypotheses reference an entity subject.
    let mut stmt = conn
        .prepare("SELECT id, subject FROM hypotheses WHERE project_id = ?1")
        .map_err(|e| Error::Database(e.to_string()))?;
    let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
        .query_map([pid], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| Error::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Database(e.to_string()))?;
    drop(stmt);
    for (reference_id, subject_id) in rows {
        insert_reverse_reference(
            conn,
            pid,
            SUBJECT_KIND_ENTITY,
            &subject_id,
            REF_KIND_HYPOTHESIS,
            &reference_id,
        )?;
    }

    // Contradictions reference an entity subject.
    let mut stmt = conn
        .prepare("SELECT id, subject FROM contradictions WHERE project_id = ?1")
        .map_err(|e| Error::Database(e.to_string()))?;
    let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
        .query_map([pid], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| Error::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Database(e.to_string()))?;
    drop(stmt);
    for (reference_id, subject_id) in rows {
        insert_reverse_reference(
            conn,
            pid,
            SUBJECT_KIND_ENTITY,
            &subject_id,
            REF_KIND_CONTRADICTION,
            &reference_id,
        )?;
    }

    // Verifications reference typed subjects via a discriminator column.
    let mut stmt = conn
        .prepare(
            "SELECT id, subject_kind, subject_id FROM verification_records WHERE project_id = ?1",
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    let rows: Vec<(Vec<u8>, String, Vec<u8>)> = stmt
        .query_map([pid], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|e| Error::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| Error::Database(e.to_string()))?;
    drop(stmt);
    for (reference_id, subject_kind, subject_id) in rows {
        insert_reverse_reference(
            conn,
            pid,
            &subject_kind,
            &subject_id,
            REF_KIND_VERIFICATION,
            &reference_id,
        )?;
    }

    Ok(())
}

fn count_project_rows(conn: &rusqlite::Connection, table: &str, pid: &[u8]) -> crate::Result<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE project_id = ?1");
    let count: i64 = conn
        .query_row(&sql, [pid], |row| row.get(0))
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(count)
}

fn count_native_artifacts(conn: &rusqlite::Connection, pid: &[u8]) -> crate::Result<i64> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM native_artifacts na \
             JOIN provider_runs pr ON na.provider_run = pr.id \
             WHERE pr.project_id = ?1",
            [pid],
            |row| row.get(0),
        )
        .map_err(|e| Error::Database(e.to_string()))?;
    Ok(count)
}

fn insert_reverse_reference(
    conn: &rusqlite::Connection,
    pid: &[u8],
    subject_kind: &str,
    subject_id: &[u8],
    reference_kind: &str,
    reference_id: &[u8],
) -> crate::Result<()> {
    conn.execute(
        "INSERT INTO derived_reverse_references \
         (project_id, subject_kind, subject_id, reference_kind, reference_id) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![pid, subject_kind, subject_id, reference_kind, reference_id,],
    )
    .map_err(|e| Error::Database(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Returns a per-table deterministic hash of canonical rows for `project_id`.
/// Tables without a `project_id` column are scoped by joining to the relevant
/// project-owned tables. This is used to prove that `build_derived_state` does
/// not modify canonical records.
#[cfg(test)]
pub fn canonical_row_hashes(
    db: &Database,
    project_id: ProjectId,
) -> crate::Result<Vec<(String, String)>> {
    use std::collections::BTreeMap;

    let conn = db.connection()?;
    let pid = project_id.as_uuid().as_bytes().as_slice();

    let queries: Vec<(&str, &str)> = vec![
        (
            "projects",
            "SELECT id, name, schema_version, created_at, updated_at, metadata FROM projects WHERE id = ?1 ORDER BY id",
        ),
        (
            "stage0_artifacts",
            "SELECT id, project_id, kind, hash_algorithm, hash_digest, size, storage_kind, storage_path, created_at, metadata FROM stage0_artifacts WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "semantic_entities",
            "SELECT id, project_id, kind, stable_key, display_name, created_at, metadata FROM semantic_entities WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "providers",
            "SELECT id, package_id, name, kind, version, executable_hash FROM providers WHERE id IN (SELECT provider_id FROM provider_runs WHERE project_id = ?1) ORDER BY id",
        ),
        (
            "provider_runs",
            "SELECT id, project_id, provider_id, operation, input_artifacts, configuration_artifact, configuration_hash, environment, started_at, completed_at, status FROM provider_runs WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "provider_entity_aliases",
            "SELECT a.provider_run, a.provider_kind, a.provider_identifier, a.entity FROM provider_entity_aliases a JOIN provider_runs pr ON a.provider_run = pr.id WHERE pr.project_id = ?1 ORDER BY a.provider_run, a.provider_identifier",
        ),
        (
            "native_artifacts",
            "SELECT na.id, na.provider_run, na.artifact, na.format, na.subject_entities, na.description FROM native_artifacts na JOIN provider_runs pr ON na.provider_run = pr.id WHERE pr.project_id = ?1 ORDER BY na.id",
        ),
        (
            "evidence_records",
            "SELECT id, project_id, subject, predicate, value, derivation, provider_run, native_artifacts, assumptions, created_at FROM evidence_records WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "evidence_lifecycle_events",
            "SELECT l.evidence, l.timestamp, l.state, l.reason, l.caused_by FROM evidence_lifecycle_events l JOIN evidence_records e ON l.evidence = e.id WHERE e.project_id = ?1 ORDER BY l.evidence, l.timestamp, l.rowid",
        ),
        (
            "hypotheses",
            "SELECT id, project_id, subject, predicate, candidate, supporting_evidence, contradicting_evidence, derived_from, confidence, status, superseded_by, created_at, updated_at FROM hypotheses WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "contradictions",
            "SELECT id, project_id, subject, predicate, evidence, hypotheses, status, resolution, created_at, updated_at FROM contradictions WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "verification_records",
            "SELECT id, project_id, subject_kind, subject_id, check_kind, state, provider_run, evidence, details, created_at FROM verification_records WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "operations",
            "SELECT id, project_id, kind, state, subject, requested_by, parent, failure, created_at, updated_at FROM operations WHERE project_id = ?1 ORDER BY id",
        ),
        (
            "progress_updates",
            "SELECT p.id, p.operation_id, p.sequence, p.message, p.metrics, p.created_at FROM progress_updates p JOIN operations o ON p.operation_id = o.id WHERE o.project_id = ?1 ORDER BY p.operation_id, p.sequence, p.id",
        ),
        (
            "cancellation_requests",
            "SELECT c.id, c.operation_id, c.requested_by, c.reason, c.created_at FROM cancellation_requests c JOIN operations o ON c.operation_id = o.id WHERE o.project_id = ?1 ORDER BY c.operation_id, c.id",
        ),
        (
            "project_events",
            "SELECT project_event_id, project_id, sequence, kind, subject, source, payload, created_at FROM project_events WHERE project_id = ?1 ORDER BY sequence, project_event_id",
        ),
    ];

    let mut hashes = BTreeMap::new();
    for (name, sql) in queries {
        let mut hasher = blake3::Hasher::new();
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| Error::Database(e.to_string()))?;
        let rows = stmt
            .query_map([pid], |row| {
                let mut buf = String::new();
                for i in 0..row.as_ref().column_count() {
                    let val: rusqlite::types::Value = row.get(i)?;
                    buf.push_str(&format!("{val:?},"));
                }
                Ok(buf)
            })
            .map_err(|e| Error::Database(e.to_string()))?;
        for row in rows {
            let row = row.map_err(|e| Error::Database(e.to_string()))?;
            hasher.update(row.as_bytes());
        }
        hashes.insert(name.to_string(), hasher.finalize().to_hex().to_string());
    }

    Ok(hashes.into_iter().collect())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use autore_schema::ids::ProjectId;

    use super::*;
    use crate::storage::database::Database;

    fn test_db() -> Database {
        Database::open_in_memory().expect("open test database")
    }

    fn new_uuid_bytes() -> Vec<u8> {
        uuid::Uuid::new_v4().as_bytes().to_vec()
    }

    fn insert_project(conn: &rusqlite::Connection, id: &[u8], name: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                name,
                "2.0",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "{}"
            ],
        )
        .unwrap();
    }

    fn insert_entity(conn: &rusqlite::Connection, project_id: &[u8], id: &[u8], kind: &str) {
        conn.execute(
            "INSERT INTO semantic_entities (id, project_id, kind, stable_key, display_name, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, project_id, kind, rusqlite::types::Null, "e", "2026-01-01T00:00:00Z", "{}"],
        )
        .unwrap();
    }

    fn insert_artifact(conn: &rusqlite::Connection, project_id: &[u8], id: &[u8]) {
        conn.execute(
            "INSERT INTO stage0_artifacts (id, project_id, kind, hash_algorithm, hash_digest, size, storage_kind, storage_path, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                project_id,
                "core.binary",
                "sha256",
                b"digest",
                42i64,
                "managed",
                "sha256/di/gest",
                "2026-01-01T00:00:00Z",
                "{}"
            ],
        )
        .unwrap();
    }

    fn insert_provider(conn: &rusqlite::Connection, id: &[u8]) {
        conn.execute(
            "INSERT INTO providers (id, package_id, name, kind, version, executable_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id,
                rusqlite::types::Null,
                "provider",
                "provider.disassembler",
                "1.0",
                rusqlite::types::Null
            ],
        )
        .unwrap();
    }

    fn insert_provider_run(
        conn: &rusqlite::Connection,
        project_id: &[u8],
        id: &[u8],
        provider_id: &[u8],
    ) {
        conn.execute(
            "INSERT INTO provider_runs (id, project_id, provider_id, operation, input_artifacts, configuration_artifact, configuration_hash, environment, started_at, completed_at, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id,
                project_id,
                provider_id,
                "core.disassemble",
                "[]",
                rusqlite::types::Null,
                "deadbeef",
                "{}",
                "2026-01-01T00:00:00Z",
                rusqlite::types::Null,
                "Running"
            ],
        )
        .unwrap();
    }

    fn insert_native_artifact(
        conn: &rusqlite::Connection,
        id: &[u8],
        run_id: &[u8],
        artifact_id: &[u8],
    ) {
        conn.execute(
            "INSERT INTO native_artifacts (id, provider_run, artifact, format, subject_entities, description) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, run_id, artifact_id, "core.native", "[]", "desc"],
        )
        .unwrap();
    }

    fn insert_evidence(
        conn: &rusqlite::Connection,
        project_id: &[u8],
        id: &[u8],
        subject_id: &[u8],
    ) {
        conn.execute(
            "INSERT INTO evidence_records (id, project_id, subject, predicate, value, derivation, provider_run, native_artifacts, assumptions, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                project_id,
                subject_id,
                "evidence.test",
                "{\"kind\":\"String\",\"value\":\"x\"}",
                "{\"method\":{\"kind\":\"DirectObservation\"},\"operation\":\"core.observe\",\"supporting_evidence\":[],\"source_hypotheses\":[]}",
                rusqlite::types::Null,
                "[]",
                "[]",
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    fn insert_evidence_lifecycle(conn: &rusqlite::Connection, evidence_id: &[u8], state: &str) {
        conn.execute(
            "INSERT INTO evidence_lifecycle_events (evidence, timestamp, state, reason, caused_by) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                evidence_id,
                "2026-01-02T00:00:00Z",
                state,
                rusqlite::types::Null,
                rusqlite::types::Null
            ],
        )
        .unwrap();
    }

    fn insert_hypothesis(
        conn: &rusqlite::Connection,
        project_id: &[u8],
        id: &[u8],
        subject_id: &[u8],
        status: &str,
    ) {
        conn.execute(
            "INSERT INTO hypotheses (id, project_id, subject, predicate, candidate, supporting_evidence, contradicting_evidence, derived_from, confidence, status, superseded_by, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id,
                project_id,
                subject_id,
                "hypothesis.test",
                "{\"kind\":\"String\",\"value\":\"v\"}",
                "[]",
                "[]",
                "[]",
                "{\"score\":0.5,\"rationale\":null}",
                status,
                rusqlite::types::Null,
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    fn insert_contradiction(
        conn: &rusqlite::Connection,
        project_id: &[u8],
        id: &[u8],
        subject_id: &[u8],
    ) {
        conn.execute(
            "INSERT INTO contradictions (id, project_id, subject, predicate, evidence, hypotheses, status, resolution, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                project_id,
                subject_id,
                "core.test",
                "[]",
                "[]",
                "Open",
                rusqlite::types::Null,
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    fn insert_verification(
        conn: &rusqlite::Connection,
        project_id: &[u8],
        id: &[u8],
        subject_kind: &str,
        subject_id: &[u8],
    ) {
        conn.execute(
            "INSERT INTO verification_records (id, project_id, subject_kind, subject_id, check_kind, state, provider_run, evidence, details, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                project_id,
                subject_kind,
                subject_id,
                "core.check",
                "Pending",
                rusqlite::types::Null,
                "[]",
                rusqlite::types::Null,
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    fn insert_operation(conn: &rusqlite::Connection, project_id: &[u8], id: &[u8]) {
        conn.execute(
            "INSERT INTO operations (id, project_id, kind, state, subject, requested_by, parent, failure, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id,
                project_id,
                "core.project.rebuild-indexes",
                "Queued",
                rusqlite::types::Null,
                "test",
                rusqlite::types::Null,
                rusqlite::types::Null,
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    fn insert_event(conn: &rusqlite::Connection, project_id: &[u8], id: &[u8], sequence: i64) {
        conn.execute(
            "INSERT INTO project_events (project_event_id, project_id, sequence, kind, subject, source, payload, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                id,
                project_id,
                sequence,
                "core.project.created",
                rusqlite::types::Null,
                "Project",
                rusqlite::types::Null,
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    fn summary_counts(conn: &rusqlite::Connection, project_id: &[u8]) -> BTreeMap<String, i64> {
        let mut stmt = conn
            .prepare(
                "SELECT artifact_count, entity_count, provider_run_count, \
                 native_artifact_count, evidence_count, hypothesis_count, \
                 contradiction_count, verification_count, operation_count, event_count \
                 FROM derived_project_summary WHERE project_id = ?1",
            )
            .unwrap();
        let row = stmt
            .query_row([project_id], |row| {
                Ok([
                    ("artifact_count".to_string(), row.get(0)?),
                    ("entity_count".to_string(), row.get(1)?),
                    ("provider_run_count".to_string(), row.get(2)?),
                    ("native_artifact_count".to_string(), row.get(3)?),
                    ("evidence_count".to_string(), row.get(4)?),
                    ("hypothesis_count".to_string(), row.get(5)?),
                    ("contradiction_count".to_string(), row.get(6)?),
                    ("verification_count".to_string(), row.get(7)?),
                    ("operation_count".to_string(), row.get(8)?),
                    ("event_count".to_string(), row.get(9)?),
                ])
            })
            .unwrap();
        row.into_iter().collect()
    }

    fn hypothesis_progress(
        conn: &rusqlite::Connection,
        project_id: &[u8],
    ) -> BTreeMap<String, i64> {
        let mut stmt = conn
            .prepare(
                "SELECT status, count FROM derived_hypothesis_progress \
                 WHERE project_id = ?1",
            )
            .unwrap();
        stmt.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<BTreeMap<_, _>, _>>()
        .unwrap()
    }

    fn evidence_progress(conn: &rusqlite::Connection, project_id: &[u8]) -> BTreeMap<String, i64> {
        let mut stmt = conn
            .prepare(
                "SELECT state, count FROM derived_evidence_progress \
                 WHERE project_id = ?1",
            )
            .unwrap();
        stmt.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<BTreeMap<_, _>, _>>()
        .unwrap()
    }

    fn reverse_reference_count(conn: &rusqlite::Connection, project_id: &[u8]) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM derived_reverse_references WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn rebuild_derived_state_preserves_queries() {
        let db = test_db();
        let conn = db.connection().unwrap();
        let pid = new_uuid_bytes();
        let entity1 = new_uuid_bytes();
        let entity2 = new_uuid_bytes();
        let artifact = new_uuid_bytes();
        let provider = new_uuid_bytes();
        let run = new_uuid_bytes();
        let native = new_uuid_bytes();
        let evidence1 = new_uuid_bytes();
        let evidence2 = new_uuid_bytes();
        let evidence3 = new_uuid_bytes();
        let hypothesis1 = new_uuid_bytes();
        let hypothesis2 = new_uuid_bytes();
        let contradiction = new_uuid_bytes();
        let verification = new_uuid_bytes();
        let operation = new_uuid_bytes();
        let event = new_uuid_bytes();

        insert_project(&conn, &pid, "derived-test");
        insert_entity(&conn, &pid, &entity1, "entity.function");
        insert_entity(&conn, &pid, &entity2, "entity.type");
        insert_artifact(&conn, &pid, &artifact);
        insert_provider(&conn, &provider);
        insert_provider_run(&conn, &pid, &run, &provider);
        insert_native_artifact(&conn, &native, &run, &artifact);
        insert_evidence(&conn, &pid, &evidence1, &entity1);
        insert_evidence(&conn, &pid, &evidence2, &entity1);
        insert_evidence(&conn, &pid, &evidence3, &entity2);
        insert_evidence_lifecycle(&conn, &evidence1, "Superseded");
        insert_evidence_lifecycle(&conn, &evidence3, "Invalidated");
        insert_hypothesis(&conn, &pid, &hypothesis1, &entity1, "Proposed");
        insert_hypothesis(&conn, &pid, &hypothesis2, &entity2, "UnderInvestigation");
        insert_contradiction(&conn, &pid, &contradiction, &entity1);
        insert_verification(&conn, &pid, &verification, "Entity", &entity1);
        insert_operation(&conn, &pid, &operation);
        insert_event(&conn, &pid, &event, 1);
        drop(conn);

        let project_id = ProjectId::from_uuid(uuid::Uuid::from_slice(&pid).unwrap());
        build_derived_state(&db, project_id).unwrap();

        let conn = db.connection().unwrap();
        let counts = summary_counts(&conn, &pid);
        assert_eq!(counts["artifact_count"], 1);
        assert_eq!(counts["entity_count"], 2);
        assert_eq!(counts["provider_run_count"], 1);
        assert_eq!(counts["native_artifact_count"], 1);
        assert_eq!(counts["evidence_count"], 3);
        assert_eq!(counts["hypothesis_count"], 2);
        assert_eq!(counts["contradiction_count"], 1);
        assert_eq!(counts["verification_count"], 1);
        assert_eq!(counts["operation_count"], 1);
        assert_eq!(counts["event_count"], 1);

        let hyp_progress = hypothesis_progress(&conn, &pid);
        assert_eq!(hyp_progress["Proposed"], 1);
        assert_eq!(hyp_progress["UnderInvestigation"], 1);

        let ev_progress = evidence_progress(&conn, &pid);
        assert_eq!(ev_progress["Active"], 1);
        assert_eq!(ev_progress["Superseded"], 1);
        assert_eq!(ev_progress["Invalidated"], 1);

        assert_eq!(reverse_reference_count(&conn, &pid), 7);
    }

    #[test]
    fn rebuild_derived_state_does_not_modify_canonical() {
        let db = test_db();
        let conn = db.connection().unwrap();
        let pid = new_uuid_bytes();
        let entity = new_uuid_bytes();
        let evidence = new_uuid_bytes();
        let hypothesis = new_uuid_bytes();
        let event = new_uuid_bytes();

        insert_project(&conn, &pid, "canonical-test");
        insert_entity(&conn, &pid, &entity, "entity.function");
        insert_evidence(&conn, &pid, &evidence, &entity);
        insert_hypothesis(&conn, &pid, &hypothesis, &entity, "Proposed");
        insert_event(&conn, &pid, &event, 1);
        drop(conn);

        let project_id = ProjectId::from_uuid(uuid::Uuid::from_slice(&pid).unwrap());
        let before = canonical_row_hashes(&db, project_id).unwrap();
        build_derived_state(&db, project_id).unwrap();
        let after = canonical_row_hashes(&db, project_id).unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn rebuild_derived_state_clears_old() {
        let db = test_db();
        let conn = db.connection().unwrap();
        let pid = new_uuid_bytes();
        let entity = new_uuid_bytes();
        let evidence1 = new_uuid_bytes();
        let evidence2 = new_uuid_bytes();
        let hypothesis = new_uuid_bytes();
        let event = new_uuid_bytes();

        insert_project(&conn, &pid, "clear-test");
        insert_entity(&conn, &pid, &entity, "entity.function");
        insert_evidence(&conn, &pid, &evidence1, &entity);
        insert_evidence(&conn, &pid, &evidence2, &entity);
        insert_hypothesis(&conn, &pid, &hypothesis, &entity, "Proposed");
        insert_event(&conn, &pid, &event, 1);
        drop(conn);

        let project_id = ProjectId::from_uuid(uuid::Uuid::from_slice(&pid).unwrap());
        build_derived_state(&db, project_id).unwrap();

        // Delete one evidence record and the hypothesis directly from canonical tables.
        {
            let conn = db.connection().unwrap();
            conn.execute("DELETE FROM evidence_records WHERE id = ?1", [&evidence2])
                .unwrap();
            conn.execute("DELETE FROM hypotheses WHERE id = ?1", [&hypothesis])
                .unwrap();
        }

        build_derived_state(&db, project_id).unwrap();

        let conn = db.connection().unwrap();
        let counts = summary_counts(&conn, &pid);
        assert_eq!(counts["evidence_count"], 1);
        assert_eq!(counts["hypothesis_count"], 0);

        let ev_progress = evidence_progress(&conn, &pid);
        assert_eq!(ev_progress["Active"], 1);
        assert!(!ev_progress.contains_key("Superseded"));

        assert_eq!(reverse_reference_count(&conn, &pid), 1);
    }
}
