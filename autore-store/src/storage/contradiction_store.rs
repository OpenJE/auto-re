use autore_schema::domain::records::{
    Contradiction, ContradictionResolution, ContradictionStatus,
};
use autore_schema::domain::{NamespacedId, Timestamp};
use autore_schema::ids::{ContradictionId, EntityId, EvidenceRecordId, HypothesisId, ProjectId};

use crate::storage::database::Database;

pub trait ContradictionStore: Send + Sync {
    fn insert(&self, contradiction: &Contradiction) -> crate::Result<()>;
    fn get(&self, id: ContradictionId) -> crate::Result<Option<Contradiction>>;
    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Contradiction>>;
    fn list_by_subject(&self, subject: EntityId) -> crate::Result<Vec<Contradiction>>;
    fn list_by_status(
        &self,
        project_id: ProjectId,
        status_kind: &str,
    ) -> crate::Result<Vec<Contradiction>>;
    fn resolve(
        &self,
        id: ContradictionId,
        resolution: ContradictionResolution,
    ) -> crate::Result<()>;
}

pub struct SqliteContradictionStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteContradictionStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteContradictionStore { db }
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

fn resolution_to_json(r: &ContradictionResolution) -> crate::Result<String> {
    serde_json::to_string(r).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn resolution_from_json(s: &str) -> Result<ContradictionResolution, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid contradiction resolution JSON: {e}"))
}

fn status_from_db(status_str: &str) -> Result<ContradictionStatus, String> {
    match status_str {
        "Open" => Ok(ContradictionStatus::Open),
        "Investigating" => Ok(ContradictionStatus::Investigating),
        "Resolved" => Ok(ContradictionStatus::Resolved),
        "Deferred" => Ok(ContradictionStatus::Deferred),
        other => Err(format!("unknown contradiction status: {other}")),
    }
}

impl ContradictionStore for SqliteContradictionStore<'_> {
    fn insert(&self, contradiction: &Contradiction) -> crate::Result<()> {
        let id_bytes = contradiction.id.as_uuid().as_bytes().to_vec();
        let project_bytes = contradiction.project.as_uuid().as_bytes().to_vec();
        let subject_bytes = contradiction.subject.as_uuid().as_bytes().to_vec();
        let predicate = contradiction.predicate.to_string();
        let evidence_json = evidence_record_ids_to_json(&contradiction.evidence)?;
        let hypotheses_json = hypothesis_ids_to_json(&contradiction.hypotheses)?;
        let status_str = contradiction.status.kind();
        let resolution_json = contradiction
            .resolution
            .as_ref()
            .map(resolution_to_json)
            .transpose()?;
        let created_at = contradiction.created_at.to_string();
        let updated_at = contradiction.updated_at.to_string();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO contradictions \
             (id, project_id, subject, predicate, evidence, hypotheses, \
              status, resolution, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                subject_bytes,
                predicate,
                evidence_json,
                hypotheses_json,
                status_str,
                resolution_json,
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

    fn get(&self, id: ContradictionId) -> crate::Result<Option<Contradiction>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, subject, predicate, evidence, hypotheses, \
             status, resolution, created_at, updated_at \
             FROM contradictions WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_contradiction,
        );

        match result {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_project(&self, project_id: ProjectId) -> crate::Result<Vec<Contradiction>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, evidence, hypotheses, \
                 status, resolution, created_at, updated_at \
                 FROM contradictions \
                 WHERE project_id = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes], row_to_contradiction)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_subject(&self, subject: EntityId) -> crate::Result<Vec<Contradiction>> {
        let subject_bytes = subject.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, evidence, hypotheses, \
                 status, resolution, created_at, updated_at \
                 FROM contradictions \
                 WHERE subject = ?1 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![subject_bytes], row_to_contradiction)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn list_by_status(
        &self,
        project_id: ProjectId,
        status_kind: &str,
    ) -> crate::Result<Vec<Contradiction>> {
        let project_bytes = project_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, subject, predicate, evidence, hypotheses, \
                 status, resolution, created_at, updated_at \
                 FROM contradictions \
                 WHERE project_id = ?1 AND status = ?2 \
                 ORDER BY created_at ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let records = stmt
            .query_map(rusqlite::params![project_bytes, status_kind], row_to_contradiction)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(records)
    }

    fn resolve(
        &self,
        id: ContradictionId,
        resolution: ContradictionResolution,
    ) -> crate::Result<()> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let current_status_str: String = conn
            .query_row(
                "SELECT status FROM contradictions WHERE id = ?1",
                rusqlite::params![id_bytes],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    crate::Error::NotFound(format!("contradiction {id} not found"))
                }
                _ => crate::Error::Database(e.to_string()),
            })?;

        let current_status =
            status_from_db(&current_status_str).map_err(crate::Error::Database)?;

        current_status.transition(&ContradictionStatus::Resolved)?;

        let resolution_json = resolution_to_json(&resolution)?;
        let updated_at = Timestamp::now().to_string();
        conn.execute(
            "UPDATE contradictions \
             SET status = ?1, resolution = ?2, updated_at = ?3 \
             WHERE id = ?4",
            rusqlite::params![
                ContradictionStatus::Resolved.kind(),
                resolution_json,
                updated_at,
                id_bytes,
            ],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }
}

fn row_to_contradiction(row: &rusqlite::Row<'_>) -> rusqlite::Result<Contradiction> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let subject_bytes: Vec<u8> = row.get(2)?;
    let predicate_str: String = row.get(3)?;
    let evidence_json: String = row.get(4)?;
    let hypotheses_json: String = row.get(5)?;
    let status_str: String = row.get(6)?;
    let resolution_json: Option<String> = row.get(7)?;
    let created_at_str: String = row.get(8)?;
    let updated_at_str: String = row.get(9)?;

    let id = ContradictionId::from_uuid(
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

    let evidence = evidence_record_ids_from_json(&evidence_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let hypotheses = hypothesis_ids_from_json(&hypotheses_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let status = status_from_db(&status_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let resolution = match resolution_json {
        Some(json) => Some(
            resolution_from_json(&json)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(ParseError(e))))?,
        ),
        None => None,
    };

    let created_at = parse_timestamp(&created_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let updated_at = parse_timestamp(&updated_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(Contradiction {
        id,
        project,
        subject,
        predicate,
        evidence,
        hypotheses,
        status,
        resolution,
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

    fn sample_contradiction(
        project: ProjectId,
        subject: EntityId,
        hypotheses: Vec<HypothesisId>,
    ) -> Contradiction {
        Contradiction::new(
            project,
            subject,
            NamespacedId::parse("evidence.predicate.function-name").unwrap(),
            vec![],
            hypotheses,
        )
    }

    fn build_resolution(hypotheses: &[HypothesisId]) -> ContradictionResolution {
        ContradictionResolution {
            resolved_at: Timestamp::now(),
            resolution: NamespacedId::parse("core.resolution.chosen-preferred").unwrap(),
            chosen: hypotheses.to_vec(),
            rationale: "preferred".into(),
        }
    }

    #[test]
    fn contradiction_store_insert_and_get() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h1 = insert_hypothesis(&db, pid, entity_id);
        let h2 = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteContradictionStore::new(&db);

        let c = sample_contradiction(pid, entity_id, vec![h1, h2]);
        store.insert(&c).unwrap();

        let fetched = store.get(c.id).unwrap().unwrap();
        assert_eq!(fetched.id, c.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.subject, entity_id);
        assert_eq!(fetched.predicate, c.predicate);
        assert_eq!(fetched.status, ContradictionStatus::Open);
        assert!(fetched.resolution.is_none());
        assert_eq!(fetched.hypotheses.len(), 2);
    }

    #[test]
    fn contradiction_store_get_not_found() {
        let db = test_db();
        let store = SqliteContradictionStore::new(&db);
        assert!(store.get(ContradictionId::new()).unwrap().is_none());
    }

    #[test]
    fn contradiction_store_list_by_project() {
        let db = test_db();
        let pid = insert_project(&db);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteContradictionStore::new(&db);

        store.insert(&sample_contradiction(pid, e1, vec![])).unwrap();
        store.insert(&sample_contradiction(pid, e2, vec![])).unwrap();

        let all = store.list_by_project(pid).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn contradiction_store_list_by_subject() {
        let db = test_db();
        let pid = insert_project(&db);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteContradictionStore::new(&db);

        store.insert(&sample_contradiction(pid, e1, vec![])).unwrap();
        store.insert(&sample_contradiction(pid, e1, vec![])).unwrap();
        store.insert(&sample_contradiction(pid, e2, vec![])).unwrap();

        let for_e1 = store.list_by_subject(e1).unwrap();
        assert_eq!(for_e1.len(), 2);

        let for_e2 = store.list_by_subject(e2).unwrap();
        assert_eq!(for_e2.len(), 1);
    }

    #[test]
    fn contradiction_store_list_by_status() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteContradictionStore::new(&db);

        let c1 = sample_contradiction(pid, entity_id, vec![h]);
        store.insert(&c1).unwrap();

        let c2 = sample_contradiction(pid, entity_id, vec![h]);
        store.insert(&c2).unwrap();
        store.resolve(c2.id, build_resolution(&[h])).unwrap();

        let open = store.list_by_status(pid, "Open").unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, c1.id);

        let resolved = store.list_by_status(pid, "Resolved").unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, c2.id);
        assert!(resolved[0].resolution.is_some());
    }

    #[test]
    fn contradiction_store_resolve_valid_transition() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteContradictionStore::new(&db);

        let c = sample_contradiction(pid, entity_id, vec![h]);
        store.insert(&c).unwrap();

        let resolution = build_resolution(&[h]);
        store.resolve(c.id, resolution).unwrap();

        let fetched = store.get(c.id).unwrap().unwrap();
        assert_eq!(fetched.status, ContradictionStatus::Resolved);
        let r = fetched.resolution.unwrap();
        assert_eq!(r.chosen, vec![h]);
        assert_eq!(r.resolution.to_string(), "core.resolution.chosen-preferred");
    }

    #[test]
    fn contradiction_store_resolve_rejects_from_resolved() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteContradictionStore::new(&db);

        let c = sample_contradiction(pid, entity_id, vec![h]);
        store.insert(&c).unwrap();
        store.resolve(c.id, build_resolution(&[h])).unwrap();

        let result = store.resolve(c.id, build_resolution(&[h]));
        assert!(result.is_err(), "Resolving an already-resolved contradiction must fail");

        let fetched = store.get(c.id).unwrap().unwrap();
        assert_eq!(fetched.status, ContradictionStatus::Resolved);
    }

    #[test]
    fn contradiction_store_resolve_not_found() {
        let db = test_db();
        let store = SqliteContradictionStore::new(&db);
        let result = store.resolve(ContradictionId::new(), build_resolution(&[]));
        assert!(result.is_err());
    }

    #[test]
    fn contradiction_store_deferred_then_reopen_then_resolve() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteContradictionStore::new(&db);

        let c = sample_contradiction(pid, entity_id, vec![h]);
        store.insert(&c).unwrap();

        let conn = db.connection().unwrap();
        conn.execute(
            "UPDATE contradictions SET status = 'Deferred' WHERE id = ?1",
            rusqlite::params![c.id.as_uuid().as_bytes().as_slice()],
        )
        .unwrap();
        drop(conn);

        let result = store.resolve(c.id, build_resolution(&[h]));
        assert!(result.is_err(), "Deferred -> Resolved is invalid; must reopen first");

        let fetched = store.get(c.id).unwrap().unwrap();
        assert_eq!(fetched.status, ContradictionStatus::Deferred);
    }

    #[test]
    fn contradiction_store_fk_project_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteContradictionStore::new(&db);

        let mut c = sample_contradiction(pid, entity_id, vec![]);
        c.project = ProjectId::new();
        let result = store.insert(&c);
        assert!(result.is_err(), "FK violation for non-existent project should fail");
    }

    #[test]
    fn contradiction_store_fk_subject_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteContradictionStore::new(&db);

        let c = sample_contradiction(pid, EntityId::new(), vec![]);
        let result = store.insert(&c);
        assert!(result.is_err(), "FK violation for non-existent entity should fail");
    }

    #[test]
    fn contradiction_store_trait_object() {
        let db = test_db();
        let store = SqliteContradictionStore::new(&db);
        fn _assert(_: &dyn ContradictionStore) {}
        _assert(&store);
    }

    #[test]
    fn contradiction_store_with_evidence_and_hypotheses() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let h1 = insert_hypothesis(&db, pid, entity_id);
        let h2 = insert_hypothesis(&db, pid, entity_id);
        let store = SqliteContradictionStore::new(&db);

        let ev1 = EvidenceRecordId::new();
        let ev2 = EvidenceRecordId::new();
        let mut c = sample_contradiction(pid, entity_id, vec![h1, h2]);
        c.evidence = vec![ev1, ev2];
        store.insert(&c).unwrap();

        let fetched = store.get(c.id).unwrap().unwrap();
        assert_eq!(fetched.evidence.len(), 2);
        assert_eq!(fetched.evidence[0], ev1);
        assert_eq!(fetched.evidence[1], ev2);
        assert_eq!(fetched.hypotheses.len(), 2);
    }
}
