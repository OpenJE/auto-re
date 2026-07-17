use autore_schema::domain::records::{
    VerificationRecord, VerificationState, VerificationSubject,
};
use autore_schema::domain::{ExtensionData, NamespacedId, Timestamp};
use autore_schema::ids::{
    ArtifactId, EntityId, EvidenceRecordId, GenerationTargetId, HypothesisId, ProjectId,
    ProviderRunId, VerificationRecordId,
};

use crate::storage::database::Database;

pub trait VerificationStore: Send + Sync {
    fn insert(&self, record: &VerificationRecord) -> crate::Result<()>;
    fn get(&self, id: VerificationRecordId) -> crate::Result<Option<VerificationRecord>>;
    fn list_by_subject(
        &self,
        subject: VerificationSubject,
    ) -> crate::Result<Vec<VerificationRecord>>;
    fn list_by_check(&self, check: &NamespacedId) -> crate::Result<Vec<VerificationRecord>>;
    fn multi_check_per_subject_supported(&self) -> bool;
}

pub struct SqliteVerificationStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteVerificationStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteVerificationStore { db }
    }
}

fn evidence_record_ids_to_json(ids: &[EvidenceRecordId]) -> crate::Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn evidence_record_ids_from_json(s: &str) -> Result<Vec<EvidenceRecordId>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid evidence record IDs JSON: {e}"))
}

fn details_to_json(d: &ExtensionData) -> crate::Result<String> {
    serde_json::to_string(d).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn details_from_json(s: &str) -> Result<ExtensionData, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid verification details JSON: {e}"))
}

fn state_from_db(state_str: &str) -> Result<VerificationState, String> {
    match state_str {
        "NotChecked" => Ok(VerificationState::NotChecked),
        "Pending" => Ok(VerificationState::Pending),
        "Passed" => Ok(VerificationState::Passed),
        "Failed" => Ok(VerificationState::Failed),
        "Inconclusive" => Ok(VerificationState::Inconclusive),
        "Blocked" => Ok(VerificationState::Blocked),
        other => Err(format!("unknown verification state: {other}")),
    }
}

fn subject_from_db(kind: &str, id_bytes: Vec<u8>) -> Result<VerificationSubject, String> {
    let uuid = uuid::Uuid::from_slice(&id_bytes)
        .map_err(|e| format!("invalid UUID bytes for verification subject: {e}"))?;
    match kind {
        "Entity" => Ok(VerificationSubject::Entity(EntityId::from_uuid(uuid))),
        "Hypothesis" => Ok(VerificationSubject::Hypothesis(HypothesisId::from_uuid(uuid))),
        "Artifact" => Ok(VerificationSubject::Artifact(ArtifactId::from_uuid(uuid))),
        "GenerationTarget" => Ok(VerificationSubject::GenerationTarget(
            GenerationTargetId::from_uuid(uuid),
        )),
        other => Err(format!("unknown verification subject kind: {other}")),
    }
}

impl VerificationStore for SqliteVerificationStore<'_> {
    fn insert(&self, record: &VerificationRecord) -> crate::Result<()> {
        let id_bytes = record.id.as_uuid().as_bytes().to_vec();
        let project_bytes = record.project.as_uuid().as_bytes().to_vec();
        let subject_kind = record.subject.kind();
        let subject_id = record.subject.id_uuid().as_bytes().to_vec();
        let check_str = record.check.to_string();
        let state_str = record.state.kind();
        let provider_run_bytes = record.provider_run.map(|id| id.as_uuid().as_bytes().to_vec());
        let evidence_json = evidence_record_ids_to_json(&record.evidence)?;
        let details_json = record.details.as_ref().map(details_to_json).transpose()?;
        let created_at = record.created_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO verification_records \
             (id, project_id, subject_kind, subject_id, check_kind, state, \
              provider_run, evidence, details, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_kind,
                subject_id,
                check_str,
                state_str,
                provider_run_bytes,
                evidence_json,
                details_json,
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

    fn get(&self, id: VerificationRecordId) -> crate::Result<Option<VerificationRecord>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, subject_kind, subject_id, check_kind, state, \
             provider_run, evidence, details, created_at \
             FROM verification_records WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_verification_record,
        );

        match result {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_subject(
        &self,
        subject: VerificationSubject,
    ) -> crate::Result<Vec<VerificationRecord>> {
        let subject_kind = subject.kind();
        let subject_id = subject.id_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject_kind, subject_id, check_kind, state, \
                 provider_run, evidence, details, created_at \
                 FROM verification_records \
                 WHERE subject_kind = ?1 AND subject_id = ?2 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![subject_kind, subject_id], row_to_verification_record)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_check(&self, check: &NamespacedId) -> crate::Result<Vec<VerificationRecord>> {
        let check_str = check.to_string();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject_kind, subject_id, check_kind, state, \
                 provider_run, evidence, details, created_at \
                 FROM verification_records \
                 WHERE check_kind = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![check_str], row_to_verification_record)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn multi_check_per_subject_supported(&self) -> bool {
        true
    }
}

fn row_to_verification_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<VerificationRecord> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let subject_kind: String = row.get(2)?;
    let subject_id_bytes: Vec<u8> = row.get(3)?;
    let check_str: String = row.get(4)?;
    let state_str: String = row.get(5)?;
    let provider_run_bytes: Option<Vec<u8>> = row.get(6)?;
    let evidence_json: String = row.get(7)?;
    let details_json: Option<String> = row.get(8)?;
    let created_at_str: String = row.get(9)?;

    let id = VerificationRecordId::from_uuid(
        uuid::Uuid::from_slice(&id_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let project = ProjectId::from_uuid(
        uuid::Uuid::from_slice(&project_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let subject = subject_from_db(&subject_kind, subject_id_bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let check = NamespacedId::parse(&check_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;

    let state = state_from_db(&state_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let provider_run = provider_run_bytes.map(|bytes| {
        ProviderRunId::from_uuid(
            uuid::Uuid::from_slice(&bytes).expect("valid UUID bytes from DB"),
        )
    });

    let evidence = evidence_record_ids_from_json(&evidence_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let details = match details_json {
        Some(json) => Some(
            details_from_json(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(ParseError(e))))?,
        ),
        None => None,
    };

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(VerificationRecord {
        id,
        project,
        subject,
        check,
        state,
        provider_run,
        evidence,
        details,
        created_at,
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
    use autore_schema::domain::records::{
        VERIFICATION_CHECK_ABI_LAYOUT, VERIFICATION_CHECK_ARTIFACT_HASH, VERIFICATION_CHECK_BUILD,
    };

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

    fn insert_hypothesis(db: &Database, project: ProjectId, subject: EntityId) -> HypothesisId {
        let id = HypothesisId::new();
        let conn = db.connection().unwrap();
        conn.execute(
            "INSERT INTO hypotheses \
             (id, project_id, subject, predicate, candidate, \
              supporting_evidence, contradicting_evidence, derived_from, \
              confidence, status, superseded_by, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, '[]', '[]', '[]', ?6, 'Proposed', NULL, ?7, ?7)",
            rusqlite::params![
                id.as_uuid().as_bytes().as_slice(),
                project.as_uuid().as_bytes().as_slice(),
                subject.as_uuid().as_bytes().as_slice(),
                "evidence.predicate.function-name",
                r#"{"kind":"String","value":"main"}"#,
                r#"{"score":0.5,"rationale":null}"#,
                "2026-01-01T00:00:00Z",
            ],
        )
        .unwrap();
        id
    }

    fn make_artifact_id() -> ArtifactId {
        ArtifactId::new()
    }

    fn sample_verification(
        project: ProjectId,
        subject: VerificationSubject,
        check: NamespacedId,
    ) -> VerificationRecord {
        VerificationRecord::new(project, subject, check)
    }

    #[test]
    fn verification_store_insert_and_get() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let rec = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        store.insert(&rec).unwrap();

        let fetched = store.get(rec.id).unwrap().unwrap();
        assert_eq!(fetched.id, rec.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.subject, VerificationSubject::Entity(entity_id));
        assert_eq!(fetched.check, VERIFICATION_CHECK_ARTIFACT_HASH.clone());
        assert_eq!(fetched.state, VerificationState::NotChecked);
    }

    #[test]
    fn verification_store_get_not_found() {
        let db = test_db();
        let store = SqliteVerificationStore::new(&db);
        assert!(store.get(VerificationRecordId::new()).unwrap().is_none());
    }

    #[test]
    fn verification_per_subject_type_entity() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let rec = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        store.insert(&rec).unwrap();

        let for_entity = store
            .list_by_subject(VerificationSubject::Entity(entity_id))
            .unwrap();
        assert_eq!(for_entity.len(), 1);
        assert_eq!(for_entity[0].id, rec.id);
    }

    #[test]
    fn verification_per_subject_type_hypothesis() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteVerificationStore::new(&db);

        let rec = sample_verification(
            pid,
            VerificationSubject::Hypothesis(h),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        store.insert(&rec).unwrap();

        let for_h = store
            .list_by_subject(VerificationSubject::Hypothesis(h))
            .unwrap();
        assert_eq!(for_h.len(), 1);
        assert_eq!(for_h[0].id, rec.id);
    }

    #[test]
    fn verification_per_subject_type_artifact() {
        let db = test_db();
        let pid = insert_project(&db);
        let artifact_id = make_artifact_id();
        let store = SqliteVerificationStore::new(&db);

        let rec = sample_verification(
            pid,
            VerificationSubject::Artifact(artifact_id),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        store.insert(&rec).unwrap();

        let for_a = store
            .list_by_subject(VerificationSubject::Artifact(artifact_id))
            .unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].id, rec.id);
    }

    #[test]
    fn verification_per_subject_type_generation_target() {
        let db = test_db();
        let pid = insert_project(&db);
        let gt = GenerationTargetId::new();
        let store = SqliteVerificationStore::new(&db);

        let rec = sample_verification(
            pid,
            VerificationSubject::GenerationTarget(gt),
            VERIFICATION_CHECK_ABI_LAYOUT.clone(),
        );
        store.insert(&rec).unwrap();

        let for_gt = store
            .list_by_subject(VerificationSubject::GenerationTarget(gt))
            .unwrap();
        assert_eq!(for_gt.len(), 1);
        assert_eq!(for_gt[0].id, rec.id);
    }

    #[test]
    fn verification_multi_check_per_subject() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let r1 = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        let r2 = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        let r3 = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_ABI_LAYOUT.clone(),
        );
        store.insert(&r1).unwrap();
        store.insert(&r2).unwrap();
        store.insert(&r3).unwrap();

        assert!(
            store.multi_check_per_subject_supported(),
            "multi_check_per_subject_supported() must return true"
        );

        let for_entity = store
            .list_by_subject(VerificationSubject::Entity(entity_id))
            .unwrap();
        assert_eq!(for_entity.len(), 3, "all three checks must coexist for the same subject");

        let checks: Vec<String> = for_entity.iter().map(|r| r.check.to_string()).collect();
        assert!(checks.contains(&"core.artifact.hash".to_string()));
        assert!(checks.contains(&"verification.build".to_string()));
        assert!(checks.contains(&"verification.abi.layout".to_string()));
    }

    #[test]
    fn verification_store_list_by_check() {
        let db = test_db();
        let pid = insert_project(&db);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let r1 = sample_verification(
            pid,
            VerificationSubject::Entity(e1),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        let r2 = sample_verification(
            pid,
            VerificationSubject::Entity(e2),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        let r3 = sample_verification(
            pid,
            VerificationSubject::Entity(e1),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        store.insert(&r1).unwrap();
        store.insert(&r2).unwrap();
        store.insert(&r3).unwrap();

        let build_checks = store.list_by_check(&VERIFICATION_CHECK_BUILD.clone()).unwrap();
        assert_eq!(build_checks.len(), 2);

        let hash_checks = store.list_by_check(&VERIFICATION_CHECK_ARTIFACT_HASH.clone()).unwrap();
        assert_eq!(hash_checks.len(), 1);
        assert_eq!(hash_checks[0].id, r3.id);
    }

    #[test]
    fn verification_fk_project_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let mut rec = sample_verification(
            ProjectId::new(),
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        rec.project = ProjectId::new();
        let result = store.insert(&rec);
        assert!(result.is_err(), "FK violation for non-existent project should fail");
    }

    #[test]
    fn verification_with_details_and_evidence() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let schema = NamespacedId::parse("core.verification.details").unwrap();
        let ev1 = EvidenceRecordId::new();
        let ev2 = EvidenceRecordId::new();
        let mut rec = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        rec.state = VerificationState::Passed;
        rec.evidence = vec![ev1, ev2];
        rec.details = Some(ExtensionData::new(schema, 1, serde_json::json!({"notes": "ok"})));
        store.insert(&rec).unwrap();

        let fetched = store.get(rec.id).unwrap().unwrap();
        assert_eq!(fetched.state, VerificationState::Passed);
        assert_eq!(fetched.evidence.len(), 2);
        assert_eq!(fetched.evidence[0], ev1);
        assert_eq!(fetched.evidence[1], ev2);
        let d = fetched.details.unwrap();
        assert_eq!(d.version, 1);
    }

    #[test]
    fn verification_trait_object() {
        let db = test_db();
        let store = SqliteVerificationStore::new(&db);
        fn _assert(_: &dyn VerificationStore) {}
        _assert(&store);
    }

    #[test]
    fn verification_subject_kind_discriminator_isolated() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteVerificationStore::new(&db);

        let uuid = entity_id.as_uuid();
        let same_uuid_artifact = ArtifactId::from_uuid(*uuid);

        let re = sample_verification(
            pid,
            VerificationSubject::Entity(entity_id),
            VERIFICATION_CHECK_BUILD.clone(),
        );
        let ra = sample_verification(
            pid,
            VerificationSubject::Artifact(same_uuid_artifact),
            VERIFICATION_CHECK_ARTIFACT_HASH.clone(),
        );
        store.insert(&re).unwrap();
        store.insert(&ra).unwrap();

        let for_entity = store
            .list_by_subject(VerificationSubject::Entity(entity_id))
            .unwrap();
        assert_eq!(for_entity.len(), 1);
        assert_eq!(for_entity[0].id, re.id);

        let for_artifact = store
            .list_by_subject(VerificationSubject::Artifact(same_uuid_artifact))
            .unwrap();
        assert_eq!(for_artifact.len(), 1);
        assert_eq!(for_artifact[0].id, ra.id);
    }
}
