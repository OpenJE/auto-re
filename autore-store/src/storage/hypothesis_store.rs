use autore_schema::domain::records::{Hypothesis, HypothesisStatus};
use autore_schema::domain::{Confidence, EvidenceValue, NamespacedId, Timestamp};
use autore_schema::ids::{EntityId, EvidenceRecordId, HypothesisId, ProjectId};

use crate::storage::database::Database;

pub trait HypothesisStore: Send + Sync {
    fn insert(&self, hypothesis: &Hypothesis) -> crate::Result<()>;
    fn get(&self, id: HypothesisId) -> crate::Result<Option<Hypothesis>>;
    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Hypothesis>>;
    fn list_by_subject(&self, subject: EntityId) -> crate::Result<Vec<Hypothesis>>;
    fn list_by_status(
        &self,
        project_id: ProjectId,
        status_kind: &str,
    ) -> crate::Result<Vec<Hypothesis>>;
    fn update_status(
        &self,
        id: HypothesisId,
        target: HypothesisStatus,
    ) -> crate::Result<()>;
    fn update_confidence(
        &self,
        id: HypothesisId,
        confidence: Confidence,
    ) -> crate::Result<()>;
    fn get_competing(
        &self,
        subject: EntityId,
        predicate: &NamespacedId,
    ) -> crate::Result<Vec<Hypothesis>>;
}

pub struct SqliteHypothesisStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteHypothesisStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteHypothesisStore { db }
    }
}

fn evidence_record_ids_to_json(ids: &[EvidenceRecordId]) -> crate::Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn evidence_record_ids_from_json(s: &str) -> Result<Vec<EvidenceRecordId>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid evidence record IDs JSON: {e}"))
}

fn hypothesis_ids_to_json(ids: &[HypothesisId]) -> crate::Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn hypothesis_ids_from_json(s: &str) -> Result<Vec<HypothesisId>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid hypothesis IDs JSON: {e}"))
}

fn status_to_db(hypothesis: &Hypothesis) -> (&'static str, Option<Vec<u8>>) {
    match &hypothesis.status {
        HypothesisStatus::Proposed => ("Proposed", None),
        HypothesisStatus::UnderInvestigation => ("UnderInvestigation", None),
        HypothesisStatus::Accepted => ("Accepted", None),
        HypothesisStatus::Rejected => ("Rejected", None),
        HypothesisStatus::Superseded { by } => {
            ("Superseded", Some(by.as_uuid().as_bytes().to_vec()))
        }
    }
}

fn status_from_db(status_str: &str, superseded_by_bytes: Option<Vec<u8>>) -> Result<HypothesisStatus, String> {
    match status_str {
        "Proposed" => Ok(HypothesisStatus::Proposed),
        "UnderInvestigation" => Ok(HypothesisStatus::UnderInvestigation),
        "Accepted" => Ok(HypothesisStatus::Accepted),
        "Rejected" => Ok(HypothesisStatus::Rejected),
        "Superseded" => {
            let bytes = superseded_by_bytes.ok_or("Superseded status requires superseded_by")?;
            let uuid = uuid::Uuid::from_slice(&bytes)
                .map_err(|e| format!("invalid UUID bytes for superseded_by: {e}"))?;
            Ok(HypothesisStatus::Superseded {
                by: HypothesisId::from_uuid(uuid),
            })
        }
        other => Err(format!("unknown hypothesis status: {other}")),
    }
}

impl HypothesisStore for SqliteHypothesisStore<'_> {
    fn insert(&self, hypothesis: &Hypothesis) -> crate::Result<()> {
        let id_bytes = hypothesis.id.as_uuid().as_bytes().to_vec();
        let project_bytes = hypothesis.project.as_uuid().as_bytes().to_vec();
        let subject_bytes = hypothesis.subject.as_uuid().as_bytes().to_vec();
        let predicate = hypothesis.predicate.to_string();
        let candidate_json = serde_json::to_string(&hypothesis.candidate)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;
        let supporting_json = evidence_record_ids_to_json(&hypothesis.supporting_evidence)?;
        let contradicting_json = evidence_record_ids_to_json(&hypothesis.contradicting_evidence)?;
        let derived_json = hypothesis_ids_to_json(&hypothesis.derived_from)?;
        let confidence_json = serde_json::to_string(&hypothesis.confidence)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;
        let (status_str, superseded_by_bytes) = status_to_db(hypothesis);
        let created_at = hypothesis.created_at.to_string();
        let updated_at = hypothesis.updated_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO hypotheses \
             (id, project_id, subject, predicate, candidate, \
              supporting_evidence, contradicting_evidence, derived_from, \
              confidence, status, superseded_by, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_bytes,
                predicate,
                candidate_json,
                supporting_json,
                contradicting_json,
                derived_json,
                confidence_json,
                status_str,
                superseded_by_bytes,
                created_at,
                updated_at,
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

    fn get(&self, id: HypothesisId) -> crate::Result<Option<Hypothesis>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, subject, predicate, candidate, \
             supporting_evidence, contradicting_evidence, derived_from, \
             confidence, status, superseded_by, created_at, updated_at \
             FROM hypotheses WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_hypothesis,
        );

        match result {
            Ok(h) => Ok(Some(h)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Hypothesis>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, candidate, \
                 supporting_evidence, contradicting_evidence, derived_from, \
                 confidence, status, superseded_by, created_at, updated_at \
                 FROM hypotheses \
                 WHERE project_id = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes], row_to_hypothesis)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_subject(&self, subject: EntityId) -> crate::Result<Vec<Hypothesis>> {
        let subject_bytes = subject.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, candidate, \
                 supporting_evidence, contradicting_evidence, derived_from, \
                 confidence, status, superseded_by, created_at, updated_at \
                 FROM hypotheses \
                 WHERE subject = ?1 \
                 ORDER BY predicate ASC, created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![subject_bytes], row_to_hypothesis)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_status(
        &self,
        project_id: ProjectId,
        status_kind: &str,
    ) -> crate::Result<Vec<Hypothesis>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, candidate, \
                 supporting_evidence, contradicting_evidence, derived_from, \
                 confidence, status, superseded_by, created_at, updated_at \
                 FROM hypotheses \
                 WHERE project_id = ?1 AND status = ?2 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes, status_kind], row_to_hypothesis)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn update_status(
        &self,
        id: HypothesisId,
        target: HypothesisStatus,
    ) -> crate::Result<()> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let (current_status_str, current_superseded_bytes): (String, Option<Vec<u8>>) = conn
            .query_row(
                "SELECT status, superseded_by FROM hypotheses WHERE id = ?1",
                rusqlite::params![id_bytes],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    crate::Error::NotFound(format!("hypothesis {id} not found"))
                }
                _ => crate::Error::Database(e.to_string()),
            })?;

        let current_status = status_from_db(&current_status_str, current_superseded_bytes)
            .map_err(crate::Error::Database)?;

        current_status.transition(&target)?;

        let (new_status_str, new_superseded_bytes) = match &target {
            HypothesisStatus::Proposed => ("Proposed", None),
            HypothesisStatus::UnderInvestigation => ("UnderInvestigation", None),
            HypothesisStatus::Accepted => ("Accepted", None),
            HypothesisStatus::Rejected => ("Rejected", None),
            HypothesisStatus::Superseded { by } => {
                ("Superseded", Some(by.as_uuid().as_bytes().to_vec()))
            }
        };

        let updated_at = Timestamp::now().to_string();
        conn.execute(
            "UPDATE hypotheses SET status = ?1, superseded_by = ?2, updated_at = ?3 WHERE id = ?4",
            rusqlite::params![new_status_str, new_superseded_bytes, updated_at, id_bytes],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }

    fn update_confidence(
        &self,
        id: HypothesisId,
        confidence: Confidence,
    ) -> crate::Result<()> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let confidence_json = serde_json::to_string(&confidence)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;
        let updated_at = Timestamp::now().to_string();

        let conn = self.db.connection()?;
        let changes = conn.execute(
            "UPDATE hypotheses SET confidence = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![confidence_json, updated_at, id_bytes],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        if changes == 0 {
            return Err(crate::Error::NotFound(format!("hypothesis {id} not found")));
        }

        Ok(())
    }

    fn get_competing(
        &self,
        subject: EntityId,
        predicate: &NamespacedId,
    ) -> crate::Result<Vec<Hypothesis>> {
        let subject_bytes = subject.as_uuid().as_bytes().to_vec();
        let predicate_str = predicate.to_string();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, candidate, \
                 supporting_evidence, contradicting_evidence, derived_from, \
                 confidence, status, superseded_by, created_at, updated_at \
                 FROM hypotheses \
                 WHERE subject = ?1 AND predicate = ?2 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![subject_bytes, predicate_str], row_to_hypothesis)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }
}

fn row_to_hypothesis(row: &rusqlite::Row<'_>) -> rusqlite::Result<Hypothesis> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let subject_bytes: Vec<u8> = row.get(2)?;
    let predicate_str: String = row.get(3)?;
    let candidate_json: String = row.get(4)?;
    let supporting_json: String = row.get(5)?;
    let contradicting_json: String = row.get(6)?;
    let derived_json: String = row.get(7)?;
    let confidence_json: String = row.get(8)?;
    let status_str: String = row.get(9)?;
    let superseded_bytes: Option<Vec<u8>> = row.get(10)?;
    let created_at_str: String = row.get(11)?;
    let updated_at_str: String = row.get(12)?;

    let id = HypothesisId::from_uuid(
        uuid::Uuid::from_slice(&id_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let project = ProjectId::from_uuid(
        uuid::Uuid::from_slice(&project_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let subject = EntityId::from_uuid(
        uuid::Uuid::from_slice(&subject_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let predicate = NamespacedId::parse(&predicate_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;

    let candidate: EvidenceValue = serde_json::from_str(&candidate_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;

    let supporting_evidence = evidence_record_ids_from_json(&supporting_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let contradicting_evidence = evidence_record_ids_from_json(&contradicting_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let derived_from = hypothesis_ids_from_json(&derived_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let confidence: Confidence = serde_json::from_str(&confidence_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e)))?;

    let status = status_from_db(&status_str, superseded_bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let updated_at = parse_timestamp(&updated_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(Hypothesis {
        id,
        project,
        subject,
        predicate,
        candidate,
        supporting_evidence,
        contradicting_evidence,
        derived_from,
        confidence,
        status,
        created_at,
        updated_at,
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
    use autore_schema::domain::records::EVIDENCE_PREDICATE_FUNCTION_NAME;

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

    fn sample_hypothesis(project: ProjectId, subject: EntityId) -> Hypothesis {
        Hypothesis {
            id: HypothesisId::new(),
            project,
            subject,
            predicate: EVIDENCE_PREDICATE_FUNCTION_NAME.clone(),
            candidate: EvidenceValue::String("main".to_string()),
            supporting_evidence: vec![],
            contradicting_evidence: vec![],
            derived_from: vec![],
            confidence: Confidence::new(0.5).unwrap(),
            status: HypothesisStatus::Proposed,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        }
    }

    #[test]
    fn hypothesis_store_insert_and_get() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h = sample_hypothesis(pid, entity_id);
        store.insert(&h).unwrap();

        let fetched = store.get(h.id).unwrap().unwrap();
        assert_eq!(fetched.id, h.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.subject, entity_id);
        assert_eq!(fetched.predicate, EVIDENCE_PREDICATE_FUNCTION_NAME.clone());
        assert_eq!(fetched.status, HypothesisStatus::Proposed);
    }

    #[test]
    fn hypothesis_store_get_not_found() {
        let db = test_db();
        let store = SqliteHypothesisStore::new(&db);
        assert!(store.get(HypothesisId::new()).unwrap().is_none());
    }

    #[test]
    fn hypothesis_store_list_by_project() {
        let db = test_db();
        let pid = insert_project(&db);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        store.insert(&sample_hypothesis(pid, e1)).unwrap();
        store.insert(&sample_hypothesis(pid, e2)).unwrap();

        let all = store.list_by_project(pid).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn hypothesis_store_list_by_subject() {
        let db = test_db();
        let pid = insert_project(&db);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        store.insert(&sample_hypothesis(pid, e1)).unwrap();
        store.insert(&sample_hypothesis(pid, e1)).unwrap();
        store.insert(&sample_hypothesis(pid, e2)).unwrap();

        let for_e1 = store.list_by_subject(e1).unwrap();
        assert_eq!(for_e1.len(), 2);

        let for_e2 = store.list_by_subject(e2).unwrap();
        assert_eq!(for_e2.len(), 1);
    }

    #[test]
    fn hypothesis_store_list_by_status() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h1 = sample_hypothesis(pid, entity_id);
        store.insert(&h1).unwrap();

        let h2 = sample_hypothesis(pid, entity_id);
        store.insert(&h2).unwrap();
        store.update_status(h2.id, HypothesisStatus::UnderInvestigation).unwrap();

        let proposed = store.list_by_status(pid, "Proposed").unwrap();
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].id, h1.id);

        let investigating = store.list_by_status(pid, "UnderInvestigation").unwrap();
        assert_eq!(investigating.len(), 1);
        assert_eq!(investigating[0].id, h2.id);
    }

    #[test]
    fn hypothesis_store_update_status_valid_transition() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h = sample_hypothesis(pid, entity_id);
        store.insert(&h).unwrap();

        store.update_status(h.id, HypothesisStatus::UnderInvestigation).unwrap();
        let fetched = store.get(h.id).unwrap().unwrap();
        assert_eq!(fetched.status, HypothesisStatus::UnderInvestigation);

        store.update_status(h.id, HypothesisStatus::Accepted).unwrap();
        let fetched = store.get(h.id).unwrap().unwrap();
        assert_eq!(fetched.status, HypothesisStatus::Accepted);
    }

    #[test]
    fn hypothesis_store_update_status_rejects_invalid() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h = sample_hypothesis(pid, entity_id);
        store.insert(&h).unwrap();

        let result = store.update_status(h.id, HypothesisStatus::Accepted);
        assert!(result.is_err(), "Proposed -> Accepted should be rejected");

        let fetched = store.get(h.id).unwrap().unwrap();
        assert_eq!(fetched.status, HypothesisStatus::Proposed);
    }

    #[test]
    fn hypothesis_store_update_status_not_found() {
        let db = test_db();
        let store = SqliteHypothesisStore::new(&db);

        let result = store.update_status(HypothesisId::new(), HypothesisStatus::UnderInvestigation);
        assert!(result.is_err());
    }

    #[test]
    fn hypothesis_store_update_confidence() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h = sample_hypothesis(pid, entity_id);
        store.insert(&h).unwrap();

        let new_confidence = Confidence::with_rationale(0.9, "strong evidence").unwrap();
        store.update_confidence(h.id, new_confidence.clone()).unwrap();

        let fetched = store.get(h.id).unwrap().unwrap();
        assert!((fetched.confidence.score() - 0.9).abs() < f32::EPSILON);
        assert_eq!(fetched.confidence.rationale(), Some("strong evidence"));
        assert_eq!(fetched.status, HypothesisStatus::Proposed, "confidence update must not change status");
    }

    #[test]
    fn hypothesis_store_update_confidence_not_found() {
        let db = test_db();
        let store = SqliteHypothesisStore::new(&db);

        let result = store.update_confidence(HypothesisId::new(), Confidence::new(0.5).unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn hypothesis_competing_coexist() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h1 = sample_hypothesis(pid, entity_id);
        let h2 = sample_hypothesis(pid, entity_id);
        store.insert(&h1).unwrap();
        store.insert(&h2).unwrap();

        store.update_status(h1.id, HypothesisStatus::UnderInvestigation).unwrap();
        store.update_status(h1.id, HypothesisStatus::Accepted).unwrap();

        let competing = store.get_competing(entity_id, &EVIDENCE_PREDICATE_FUNCTION_NAME).unwrap();
        assert_eq!(competing.len(), 2, "both hypotheses must coexist after accepting one");
        assert_eq!(competing[0].id, h1.id);
        assert_eq!(competing[0].status, HypothesisStatus::Accepted);
        assert_eq!(competing[1].id, h2.id);
        assert_eq!(competing[1].status, HypothesisStatus::Proposed);
    }

    #[test]
    fn hypothesis_store_supersession() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let h1 = sample_hypothesis(pid, entity_id);
        let h2 = sample_hypothesis(pid, entity_id);
        store.insert(&h1).unwrap();
        store.insert(&h2).unwrap();

        store.update_status(h1.id, HypothesisStatus::UnderInvestigation).unwrap();
        store.update_status(h1.id, HypothesisStatus::Accepted).unwrap();

        store.update_status(h2.id, HypothesisStatus::UnderInvestigation).unwrap();
        store.update_status(h2.id, HypothesisStatus::Accepted).unwrap();

        let superseded = HypothesisStatus::Superseded { by: h2.id };
        store.update_status(h1.id, superseded).unwrap();

        let fetched = store.get(h1.id).unwrap().unwrap();
        assert_eq!(fetched.status.kind(), "Superseded");
        if let HypothesisStatus::Superseded { by } = fetched.status {
            assert_eq!(by, h2.id);
        } else {
            panic!("expected Superseded status");
        }
    }

    #[test]
    fn hypothesis_store_fk_project_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let mut h = sample_hypothesis(pid, entity_id);
        h.project = ProjectId::new();
        let result = store.insert(&h);
        assert!(result.is_err(), "FK violation for non-existent project should fail");
    }

    #[test]
    fn hypothesis_store_fk_subject_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteHypothesisStore::new(&db);

        let h = sample_hypothesis(pid, EntityId::new());
        let result = store.insert(&h);
        assert!(result.is_err(), "FK violation for non-existent entity should fail");
    }

    #[test]
    fn hypothesis_store_trait_object() {
        let db = test_db();
        let store = SqliteHypothesisStore::new(&db);
        fn _assert(_: &dyn HypothesisStore) {}
        _assert(&store);
    }

    #[test]
    fn hypothesis_store_with_evidence_refs() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteHypothesisStore::new(&db);

        let ev1 = EvidenceRecordId::new();
        let ev2 = EvidenceRecordId::new();
        let mut h = sample_hypothesis(pid, entity_id);
        h.supporting_evidence = vec![ev1, ev2];
        h.contradicting_evidence = vec![EvidenceRecordId::new()];
        h.derived_from = vec![HypothesisId::new()];

        store.insert(&h).unwrap();

        let fetched = store.get(h.id).unwrap().unwrap();
        assert_eq!(fetched.supporting_evidence.len(), 2);
        assert_eq!(fetched.supporting_evidence[0], ev1);
        assert_eq!(fetched.supporting_evidence[1], ev2);
        assert_eq!(fetched.contradicting_evidence.len(), 1);
        assert_eq!(fetched.derived_from.len(), 1);
    }
}
