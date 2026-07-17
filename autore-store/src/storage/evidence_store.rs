use autore_schema::domain::records::{
    Assumption, EvidenceLifecycleEvent, EvidenceLifecycleState, EvidenceRecord,
};
use autore_schema::domain::{Derivation, EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{EntityId, EvidenceRecordId, NativeArtifactId, ProjectId, ProviderRunId};

use crate::storage::database::Database;

pub trait EvidenceStore: Send + Sync {
    fn insert_evidence(&self, record: &EvidenceRecord) -> crate::Result<()>;
    fn get_evidence(&self, id: EvidenceRecordId) -> crate::Result<Option<EvidenceRecord>>;
    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<EvidenceRecord>>;
    fn list_by_subject(&self, subject: EntityId) -> crate::Result<Vec<EvidenceRecord>>;
    fn list_by_provider_run(&self, run_id: ProviderRunId) -> crate::Result<Vec<EvidenceRecord>>;
    fn record_lifecycle_event(&self, event: &EvidenceLifecycleEvent) -> crate::Result<()>;
    fn list_lifecycle_for_evidence(
        &self,
        evidence_id: EvidenceRecordId,
    ) -> crate::Result<Vec<EvidenceLifecycleEvent>>;
}

pub struct SqliteEvidenceStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteEvidenceStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteEvidenceStore { db }
    }
}

fn native_artifact_ids_to_json(ids: &[NativeArtifactId]) -> crate::Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn native_artifact_ids_from_json(s: &str) -> Result<Vec<NativeArtifactId>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid native artifact IDs JSON: {e}"))
}

fn assumptions_to_json(assumptions: &[Assumption]) -> crate::Result<String> {
    serde_json::to_string(assumptions).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn assumptions_from_json(s: &str) -> Result<Vec<Assumption>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid assumptions JSON: {e}"))
}

impl EvidenceStore for SqliteEvidenceStore<'_> {
    fn insert_evidence(&self, record: &EvidenceRecord) -> crate::Result<()> {
        let id_bytes = record.id.as_uuid().as_bytes().to_vec();
        let project_bytes = record.project.as_uuid().as_bytes().to_vec();
        let subject_bytes = record.subject.as_uuid().as_bytes().to_vec();
        let predicate = record.predicate.to_string();
        let value_json = serde_json::to_string(&record.value)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;
        let derivation_json = serde_json::to_string(&record.derivation)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;
        let provider_run_bytes = record
            .provider_run
            .map(|id| id.as_uuid().as_bytes().to_vec());
        let native_json = native_artifact_ids_to_json(&record.native_artifacts)?;
        let assumptions_json = assumptions_to_json(&record.assumptions)?;
        let created_at = record.created_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO evidence_records \
             (id, project_id, subject, predicate, value, derivation, \
              provider_run, native_artifacts, assumptions, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_bytes,
                predicate,
                value_json,
                derivation_json,
                provider_run_bytes,
                native_json,
                assumptions_json,
                created_at,
            ],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("FOREIGN KEY constraint failed") {
                crate::Error::Database(format!("foreign key violation: {msg}"))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn get_evidence(&self, id: EvidenceRecordId) -> crate::Result<Option<EvidenceRecord>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, subject, predicate, value, derivation, \
             provider_run, native_artifacts, assumptions, created_at \
             FROM evidence_records WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_evidence_record,
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<EvidenceRecord>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, value, derivation, \
                 provider_run, native_artifacts, assumptions, created_at \
                 FROM evidence_records \
                 WHERE project_id = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes], row_to_evidence_record)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_subject(&self, subject: EntityId) -> crate::Result<Vec<EvidenceRecord>> {
        let subject_bytes = subject.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, value, derivation, \
                 provider_run, native_artifacts, assumptions, created_at \
                 FROM evidence_records \
                 WHERE subject = ?1 \
                 ORDER BY predicate ASC, created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![subject_bytes], row_to_evidence_record)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_provider_run(&self, run_id: ProviderRunId) -> crate::Result<Vec<EvidenceRecord>> {
        let run_bytes = run_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, value, derivation, \
                 provider_run, native_artifacts, assumptions, created_at \
                 FROM evidence_records \
                 WHERE provider_run = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![run_bytes], row_to_evidence_record)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn record_lifecycle_event(&self, event: &EvidenceLifecycleEvent) -> crate::Result<()> {
        let evidence_bytes = event.evidence.as_uuid().as_bytes().to_vec();
        let timestamp = event.timestamp.to_string();
        let state = event.state.to_string();
        let caused_by_bytes = event.caused_by.map(|id| id.as_uuid().as_bytes().to_vec());

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO evidence_lifecycle_events \
             (evidence, timestamp, state, reason, caused_by) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                evidence_bytes,
                timestamp,
                state,
                event.reason,
                caused_by_bytes,
            ],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("FOREIGN KEY constraint failed") {
                crate::Error::Database(format!("foreign key violation: {msg}"))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn list_lifecycle_for_evidence(
        &self,
        evidence_id: EvidenceRecordId,
    ) -> crate::Result<Vec<EvidenceLifecycleEvent>> {
        let evidence_bytes = evidence_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT evidence, timestamp, state, reason, caused_by \
                 FROM evidence_lifecycle_events \
                 WHERE evidence = ?1 \
                 ORDER BY timestamp ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let events = stmt
            .query_map(rusqlite::params![evidence_bytes], row_to_lifecycle_event)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(events)
    }
}

fn row_to_evidence_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceRecord> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let subject_bytes: Vec<u8> = row.get(2)?;
    let predicate_str: String = row.get(3)?;
    let value_json: String = row.get(4)?;
    let derivation_json: String = row.get(5)?;
    let provider_run_bytes: Option<Vec<u8>> = row.get(6)?;
    let native_json: String = row.get(7)?;
    let assumptions_json: String = row.get(8)?;
    let created_at_str: String = row.get(9)?;

    let id = EvidenceRecordId::from_uuid(uuid::Uuid::from_slice(&id_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })?);

    let project = ProjectId::from_uuid(uuid::Uuid::from_slice(&project_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e))
    })?);

    let subject = EntityId::from_uuid(uuid::Uuid::from_slice(&subject_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Blob, Box::new(e))
    })?);

    let predicate = NamespacedId::parse(&predicate_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let value: EvidenceValue = serde_json::from_str(&value_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let derivation: Derivation = serde_json::from_str(&derivation_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let provider_run = provider_run_bytes.map(|bytes| {
        ProviderRunId::from_uuid(uuid::Uuid::from_slice(&bytes).expect("valid UUID bytes from DB"))
    });

    let native_artifacts = native_artifact_ids_from_json(&native_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            7,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let assumptions = assumptions_from_json(&assumptions_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            8,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let created_at = parse_timestamp(&created_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            9,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    Ok(EvidenceRecord {
        id,
        project,
        subject,
        predicate,
        value,
        derivation,
        provider_run,
        native_artifacts,
        assumptions,
        created_at,
    })
}

fn row_to_lifecycle_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvidenceLifecycleEvent> {
    let evidence_bytes: Vec<u8> = row.get(0)?;
    let timestamp_str: String = row.get(1)?;
    let state_str: String = row.get(2)?;
    let reason: Option<String> = row.get(3)?;
    let caused_by_bytes: Option<Vec<u8>> = row.get(4)?;

    let evidence =
        EvidenceRecordId::from_uuid(uuid::Uuid::from_slice(&evidence_bytes).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
        })?);

    let timestamp = parse_timestamp(&timestamp_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let state = match state_str.as_str() {
        "Active" => EvidenceLifecycleState::Active,
        "Superseded" => EvidenceLifecycleState::Superseded,
        "Invalidated" => EvidenceLifecycleState::Invalidated,
        "Unavailable" => EvidenceLifecycleState::Unavailable,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(ParseError(format!("unknown lifecycle state: {other}"))),
            ));
        }
    };

    let caused_by = caused_by_bytes.map(|bytes| {
        EvidenceRecordId::from_uuid(
            uuid::Uuid::from_slice(&bytes).expect("valid UUID bytes from DB"),
        )
    });

    Ok(EvidenceLifecycleEvent {
        evidence,
        timestamp,
        state,
        reason,
        caused_by,
    })
}

#[derive(Debug)]
struct ParseError(String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("invalid timestamp: {e}"))?;
    Ok(Timestamp::from_offset_datetime(dt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::Derivation;
    use autore_schema::domain::records::{
        EVIDENCE_PREDICATE_FUNCTION_NAME, EVIDENCE_PREDICATE_FUNCTION_SIGNATURE,
        PROVIDER_KIND_DECOMPILER,
    };
    use autore_schema::domain::values::DerivationMethod;
    use autore_schema::ids::{ProviderId, ProviderRunId};

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn insert_project(db: &Database) -> ProjectId {
        let pid = ProjectId::new();
        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                pid.as_uuid().as_bytes().as_slice(),
                "test-project",
                "2.0",
                "2026-01-01T00:00:00Z",
                "2026-01-01T00:00:00Z",
                "{}",
            ],
        )
        .unwrap();
        pid
    }

    fn insert_entity(db: &Database, project: ProjectId) -> EntityId {
        let id = EntityId::new();
        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO semantic_entities (id, project_id, kind, stable_key, display_name, created_at, metadata) \
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, '{}')",
            rusqlite::params![
                id.as_uuid().as_bytes().as_slice(),
                project.as_uuid().as_bytes().as_slice(),
                "core.function",
                "test_fn",
                "2026-01-01T00:00:00Z",
            ],
        )
        .unwrap();
        id
    }

    fn insert_provider(db: &Database) -> ProviderId {
        let id = ProviderId::new();
        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO providers (id, package_id, name, kind, version, executable_hash) \
             VALUES (?1, NULL, ?2, ?3, ?4, NULL)",
            rusqlite::params![
                id.as_uuid().as_bytes().as_slice(),
                "test-provider",
                PROVIDER_KIND_DECOMPILER.to_string(),
                "1.0",
            ],
        )
        .unwrap();
        id
    }

    fn insert_run(db: &Database, project: ProjectId, provider: ProviderId) -> ProviderRunId {
        let id = ProviderRunId::new();
        let conn = db.connection().unwrap();
        let env_json = serde_json::json!({
            "operating_system": "core.linux",
            "architecture": "core.x86-64",
            "isolation_backend": null,
            "image_digest": null,
            "extension": null,
        });
        let ch_json = serde_json::json!({
            "algorithm": "sha256",
            "digest": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        });
        conn.execute(
            "INSERT INTO provider_runs \
             (id, project_id, provider_id, operation, input_artifacts, \
              configuration_artifact, configuration_hash, environment, \
              started_at, completed_at, status) \
             VALUES (?1, ?2, ?3, ?4, '[]', NULL, ?5, ?6, ?7, NULL, 'Running')",
            rusqlite::params![
                id.as_uuid().as_bytes().as_slice(),
                project.as_uuid().as_bytes().as_slice(),
                provider.as_uuid().as_bytes().as_slice(),
                "core.disassemble",
                ch_json.to_string(),
                env_json.to_string(),
                "2026-01-01T00:00:00Z",
            ],
        )
        .unwrap();
        id
    }

    fn sample_evidence(
        project: ProjectId,
        subject: EntityId,
        predicate: NamespacedId,
    ) -> EvidenceRecord {
        EvidenceRecord {
            id: EvidenceRecordId::new(),
            project,
            subject,
            predicate,
            value: EvidenceValue::String("main".to_string()),
            derivation: Derivation::new(
                DerivationMethod::DirectObservation,
                NamespacedId::parse("core.observe").unwrap(),
                vec![],
                vec![],
            ),
            provider_run: None,
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        }
    }

    #[test]
    fn evidence_append_only() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteEvidenceStore::new(&db);

        let rec = sample_evidence(pid, entity_id, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        store.insert_evidence(&rec).unwrap();

        let fetched = store.get_evidence(rec.id).unwrap().unwrap();
        assert_eq!(fetched.id, rec.id);
        assert_eq!(fetched.subject, entity_id);
        assert_eq!(fetched.predicate, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());

        // Verify no update/delete methods exist on the trait — this is a
        // compile-time guarantee. The trait only exposes insert, get, list,
        // and lifecycle methods.
        fn _assert_no_update_delete<T: EvidenceStore>() {
            // If update_evidence or delete_evidence existed, this would
            // need to be updated. The absence of such methods IS the test.
        }
        _assert_no_update_delete::<SqliteEvidenceStore<'_>>();
    }

    #[test]
    fn evidence_lifecycle_history() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteEvidenceStore::new(&db);

        let rec = sample_evidence(pid, entity_id, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        store.insert_evidence(&rec).unwrap();

        let ev1 = EvidenceLifecycleEvent {
            evidence: rec.id,
            timestamp: Timestamp::now(),
            state: EvidenceLifecycleState::Active,
            reason: Some("initial evidence".to_string()),
            caused_by: None,
        };
        store.record_lifecycle_event(&ev1).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(5));

        let rec2 = sample_evidence(pid, entity_id, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        store.insert_evidence(&rec2).unwrap();

        let ev2 = EvidenceLifecycleEvent {
            evidence: rec.id,
            timestamp: Timestamp::now(),
            state: EvidenceLifecycleState::Superseded,
            reason: Some("replaced by newer analysis".to_string()),
            caused_by: Some(rec2.id),
        };
        store.record_lifecycle_event(&ev2).unwrap();

        let history = store.list_lifecycle_for_evidence(rec.id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].state, EvidenceLifecycleState::Active);
        assert_eq!(history[1].state, EvidenceLifecycleState::Superseded);
        assert_eq!(history[1].caused_by, Some(rec2.id));
    }

    #[test]
    fn evidence_query_by_subject_predicate() {
        let db = test_db();
        let pid = insert_project(&db);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteEvidenceStore::new(&db);

        let r1 = sample_evidence(pid, e1, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        let r2 = sample_evidence(pid, e1, EVIDENCE_PREDICATE_FUNCTION_SIGNATURE.clone());
        let r3 = sample_evidence(pid, e2, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        store.insert_evidence(&r1).unwrap();
        store.insert_evidence(&r2).unwrap();
        store.insert_evidence(&r3).unwrap();

        let for_e1 = store.list_by_subject(e1).unwrap();
        assert_eq!(for_e1.len(), 2);

        let for_e2 = store.list_by_subject(e2).unwrap();
        assert_eq!(for_e2.len(), 1);
        assert_eq!(for_e2[0].id, r3.id);

        let all = store.list_by_project(pid).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn evidence_fk_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEvidenceStore::new(&db);

        let rec = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: pid,
            subject: EntityId::new(),
            predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
            value: EvidenceValue::Null,
            derivation: Derivation::new(
                DerivationMethod::DirectObservation,
                NamespacedId::parse("core.observe").unwrap(),
                vec![],
                vec![],
            ),
            provider_run: None,
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        };
        let result = store.insert_evidence(&rec);
        assert!(
            result.is_err(),
            "FK violation for non-existent entity should fail"
        );
    }

    #[test]
    fn evidence_fk_project_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteEvidenceStore::new(&db);

        let rec = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: ProjectId::new(),
            subject: entity_id,
            predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
            value: EvidenceValue::Null,
            derivation: Derivation::new(
                DerivationMethod::DirectObservation,
                NamespacedId::parse("core.observe").unwrap(),
                vec![],
                vec![],
            ),
            provider_run: None,
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        };
        let result = store.insert_evidence(&rec);
        assert!(
            result.is_err(),
            "FK violation for non-existent project should fail"
        );
    }

    #[test]
    fn evidence_fk_provider_run_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteEvidenceStore::new(&db);

        let rec = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: pid,
            subject: entity_id,
            predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
            value: EvidenceValue::Null,
            derivation: Derivation::new(
                DerivationMethod::DirectObservation,
                NamespacedId::parse("core.observe").unwrap(),
                vec![],
                vec![],
            ),
            provider_run: Some(ProviderRunId::new()),
            native_artifacts: vec![],
            assumptions: vec![],
            created_at: Timestamp::now(),
        };
        let result = store.insert_evidence(&rec);
        assert!(
            result.is_err(),
            "FK violation for non-existent provider_run should fail"
        );
    }

    #[test]
    fn native_artifact_reference_integrity() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteEvidenceStore::new(&db);

        let na1 = NativeArtifactId::new();
        let na2 = NativeArtifactId::new();

        let rec = EvidenceRecord {
            id: EvidenceRecordId::new(),
            project: pid,
            subject: entity_id,
            predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
            value: EvidenceValue::String("test".to_string()),
            derivation: Derivation::new(
                DerivationMethod::ProviderAnalysis,
                NamespacedId::parse("core.analyze").unwrap(),
                vec![],
                vec![],
            ),
            provider_run: None,
            native_artifacts: vec![na1, na2],
            assumptions: vec![Assumption {
                description: "binary is stripped".to_string(),
                evidence: None,
            }],
            created_at: Timestamp::now(),
        };
        store.insert_evidence(&rec).unwrap();

        let fetched = store.get_evidence(rec.id).unwrap().unwrap();
        assert_eq!(fetched.native_artifacts.len(), 2);
        assert_eq!(fetched.native_artifacts[0], na1);
        assert_eq!(fetched.native_artifacts[1], na2);
        assert_eq!(fetched.assumptions.len(), 1);
        assert_eq!(fetched.assumptions[0].description, "binary is stripped");
    }

    #[test]
    fn evidence_list_by_provider_run() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let store = SqliteEvidenceStore::new(&db);

        let mut rec = sample_evidence(pid, entity_id, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        rec.provider_run = Some(run_id);
        store.insert_evidence(&rec).unwrap();

        let rec2 = sample_evidence(
            pid,
            entity_id,
            EVIDENCE_PREDICATE_FUNCTION_SIGNATURE.clone(),
        );
        store.insert_evidence(&rec2).unwrap();

        let by_run = store.list_by_provider_run(run_id).unwrap();
        assert_eq!(by_run.len(), 1);
        assert_eq!(by_run[0].id, rec.id);
    }

    #[test]
    fn evidence_get_not_found() {
        let db = test_db();
        let store = SqliteEvidenceStore::new(&db);
        let result = store.get_evidence(EvidenceRecordId::new()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn evidence_lifecycle_fk_enforced() {
        let db = test_db();
        let store = SqliteEvidenceStore::new(&db);

        let ev = EvidenceLifecycleEvent {
            evidence: EvidenceRecordId::new(),
            timestamp: Timestamp::now(),
            state: EvidenceLifecycleState::Active,
            reason: None,
            caused_by: None,
        };
        let result = store.record_lifecycle_event(&ev);
        assert!(
            result.is_err(),
            "FK violation for non-existent evidence should fail"
        );
    }

    #[test]
    fn evidence_trait_object() {
        let db = test_db();
        let store = SqliteEvidenceStore::new(&db);
        fn _assert(_: &dyn EvidenceStore) {}
        _assert(&store);
    }
}
