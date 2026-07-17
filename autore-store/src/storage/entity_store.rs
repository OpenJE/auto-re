use autore_schema::domain::records::SemanticEntity;
use autore_schema::domain::{MetadataMap, NamespacedId, StableEntityKey, Timestamp};
use autore_schema::ids::{EntityId, ProjectId};

use crate::storage::database::Database;

pub enum EntityColumn {
    Kind,
    CreatedAt,
}

pub struct EntityPage {
    pub offset: u32,
    pub limit: u32,
    pub order_by: EntityColumn,
}

pub trait EntityStore: Send + Sync {
    fn insert(&self, entity: &SemanticEntity) -> crate::Result<()>;
    fn get(&self, id: EntityId) -> crate::Result<Option<SemanticEntity>>;
    fn list_by_project(
        &self,
        project_id: ProjectId,
        page: EntityPage,
        kind_filter: Option<&NamespacedId>,
    ) -> crate::Result<Vec<SemanticEntity>>;
    fn list_by_stable_key(
        &self,
        project_id: ProjectId,
        stable_key: &StableEntityKey,
    ) -> crate::Result<Vec<SemanticEntity>>;
    fn count_by_project_kind(
        &self,
        project_id: ProjectId,
        kind: &NamespacedId,
    ) -> crate::Result<u64>;
    fn register_kind(&self, _kind: &NamespacedId) {}
}

impl EntityColumn {
    fn as_sql(&self) -> &'static str {
        match self {
            EntityColumn::Kind => "kind",
            EntityColumn::CreatedAt => "created_at",
        }
    }
}

pub struct SqliteEntityStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteEntityStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteEntityStore { db }
    }
}

impl EntityStore for SqliteEntityStore<'_> {
    fn insert(&self, entity: &SemanticEntity) -> crate::Result<()> {
        let id_bytes = entity.id.as_uuid().as_bytes().to_vec();
        let project_bytes = entity.project.as_uuid().as_bytes().to_vec();
        let kind = entity.kind.to_string();
        let stable_key_json = entity
            .stable_key
            .as_ref()
            .map(|k| {
                serde_json::to_string(k).map_err(|e| crate::Error::Serialization(e.to_string()))
            })
            .transpose()?;
        let created_at = entity.created_at.to_string();
        let metadata = serde_json::to_string(&entity.metadata)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO semantic_entities \
             (id, project_id, kind, stable_key, display_name, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                kind,
                stable_key_json,
                entity.display_name,
                created_at,
                metadata,
            ],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE constraint failed")
                || msg.contains("idx_entities_project_stable_key")
            {
                crate::Error::Conflict(format!(
                    "duplicate stable_key in project {}",
                    entity.project
                ))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn get(&self, id: EntityId) -> crate::Result<Option<SemanticEntity>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, kind, stable_key, display_name, created_at, metadata \
             FROM semantic_entities WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_entity,
        );

        match result {
            Ok(entity) => Ok(Some(entity)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_project(
        &self,
        project_id: ProjectId,
        page: EntityPage,
        kind_filter: Option<&NamespacedId>,
    ) -> crate::Result<Vec<SemanticEntity>> {
        let order_col = page.order_by.as_sql();
        let conn = self.db.connection()?;

        let entities = if let Some(kind) = kind_filter {
            let kind_str = kind.to_string();
            let sql = format!(
                "SELECT id, project_id, kind, stable_key, display_name, created_at, metadata \
                 FROM semantic_entities \
                 WHERE project_id = ?1 AND kind = ?2 \
                 ORDER BY {order_col} ASC, id ASC LIMIT ?3 OFFSET ?4"
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| crate::Error::Database(e.to_string()))?;
            stmt.query_map(
                rusqlite::params![
                    project_id.as_uuid().as_bytes().as_slice(),
                    kind_str,
                    page.limit,
                    page.offset,
                ],
                row_to_entity,
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?
        } else {
            let sql = format!(
                "SELECT id, project_id, kind, stable_key, display_name, created_at, metadata \
                 FROM semantic_entities \
                 WHERE project_id = ?1 \
                 ORDER BY {order_col} ASC, id ASC LIMIT ?2 OFFSET ?3"
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| crate::Error::Database(e.to_string()))?;
            stmt.query_map(
                rusqlite::params![
                    project_id.as_uuid().as_bytes().as_slice(),
                    page.limit,
                    page.offset,
                ],
                row_to_entity,
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?
        };

        Ok(entities)
    }

    fn list_by_stable_key(
        &self,
        project_id: ProjectId,
        stable_key: &StableEntityKey,
    ) -> crate::Result<Vec<SemanticEntity>> {
        let key_json = serde_json::to_string(stable_key)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, kind, stable_key, display_name, created_at, metadata \
                 FROM semantic_entities \
                 WHERE project_id = ?1 AND stable_key = ?2 \
                 ORDER BY kind ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let entities = stmt
            .query_map(
                rusqlite::params![project_id.as_uuid().as_bytes().as_slice(), key_json,],
                row_to_entity,
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(entities)
    }

    fn count_by_project_kind(
        &self,
        project_id: ProjectId,
        kind: &NamespacedId,
    ) -> crate::Result<u64> {
        let kind_str = kind.to_string();
        let conn = self.db.connection()?;

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM semantic_entities \
                 WHERE project_id = ?1 AND kind = ?2",
                rusqlite::params![project_id.as_uuid().as_bytes().as_slice(), kind_str,],
                |row| row.get(0),
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(count as u64)
    }
}

fn row_to_entity(row: &rusqlite::Row<'_>) -> rusqlite::Result<SemanticEntity> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let kind_str: String = row.get(2)?;
    let stable_key_json: Option<String> = row.get(3)?;
    let display_name: Option<String> = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let metadata_str: String = row.get(6)?;

    let id_uuid = uuid::Uuid::from_slice(&id_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })?;
    let id = EntityId::from_uuid(id_uuid);

    let project_uuid = uuid::Uuid::from_slice(&project_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e))
    })?;
    let project = ProjectId::from_uuid(project_uuid);

    let kind = NamespacedId::parse(&kind_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let stable_key = stable_key_json
        .map(|json| {
            serde_json::from_str::<StableEntityKey>(&json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()?;

    let created_at = parse_timestamp(&created_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let metadata: MetadataMap = serde_json::from_str(&metadata_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(SemanticEntity {
        id,
        project,
        kind,
        stable_key,
        display_name,
        created_at,
        metadata,
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
    use autore_schema::domain::ContentHash;
    use autore_schema::domain::records::{
        ENTITY_KIND_EXTERNAL_FUNCTION, ENTITY_KIND_FUNCTION, ENTITY_KIND_STRING, ENTITY_KIND_TYPE,
    };
    use autore_schema::domain::values::{BinaryLocation, ModuleIdentity};
    use autore_schema::ids::BinaryArtifactId;

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

    fn test_binary_location() -> BinaryLocation {
        BinaryLocation::new(
            BinaryArtifactId::new(),
            ModuleIdentity::new(
                Some(".text".into()),
                ContentHash::sha256(b"test module"),
                Some(0),
            ),
            0x1000,
        )
    }

    #[test]
    fn entity_store_insert_and_get() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        let entity = SemanticEntity::new(
            pid,
            ENTITY_KIND_FUNCTION.clone(),
            Some(StableEntityKey::BinaryLocation(test_binary_location())),
            Some("main".to_string()),
        );
        store.insert(&entity).unwrap();

        let fetched = store.get(entity.id).unwrap().unwrap();
        assert_eq!(fetched.id, entity.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.kind, ENTITY_KIND_FUNCTION.clone());
        assert_eq!(fetched.display_name, Some("main".to_string()));
        assert!(fetched.stable_key.is_some());
    }

    #[test]
    fn entity_store_get_not_found() {
        let db = test_db();
        let store = SqliteEntityStore::new(&db);

        let result = store.get(EntityId::new()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn entity_store_null_stable_key_allowed_multiple() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        let e1 = SemanticEntity::new(pid, ENTITY_KIND_STRING.clone(), None, Some("str1".into()));
        let e2 = SemanticEntity::new(pid, ENTITY_KIND_STRING.clone(), None, Some("str2".into()));
        store.insert(&e1).unwrap();
        store.insert(&e2).unwrap();

        let count = store
            .count_by_project_kind(pid, &ENTITY_KIND_STRING)
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn entity_store_duplicate_stable_key_conflict() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        let loc = test_binary_location();
        let e1 = SemanticEntity::new(
            pid,
            ENTITY_KIND_FUNCTION.clone(),
            Some(StableEntityKey::BinaryLocation(loc.clone())),
            Some("fn1".into()),
        );
        let e2 = SemanticEntity::new(
            pid,
            ENTITY_KIND_FUNCTION.clone(),
            Some(StableEntityKey::BinaryLocation(loc)),
            Some("fn2".into()),
        );

        store.insert(&e1).unwrap();
        let result = store.insert(&e2);
        assert!(
            result.is_err(),
            "duplicate stable_key should fail with Conflict"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("duplicate") || err_msg.contains("Conflict"),
            "error should mention conflict: {err_msg}"
        );
    }

    #[test]
    fn entity_store_same_stable_key_different_projects_ok() {
        let db = test_db();
        let pid1 = insert_project(&db);
        let pid2 = {
            let pid = ProjectId::new();
            let conn = db.connection().unwrap();
            conn.execute(
                "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    pid.as_uuid().as_bytes().as_slice(),
                    "test-project-2",
                    "2.0",
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z",
                    "{}",
                ],
            )
            .unwrap();
            pid
        };
        let store = SqliteEntityStore::new(&db);

        let loc = test_binary_location();
        let e1 = SemanticEntity::new(
            pid1,
            ENTITY_KIND_FUNCTION.clone(),
            Some(StableEntityKey::BinaryLocation(loc.clone())),
            Some("fn1".into()),
        );
        let e2 = SemanticEntity::new(
            pid2,
            ENTITY_KIND_FUNCTION.clone(),
            Some(StableEntityKey::BinaryLocation(loc)),
            Some("fn2".into()),
        );

        store.insert(&e1).unwrap();
        store.insert(&e2).unwrap();
    }

    #[test]
    fn entity_store_fk_enforced() {
        let db = test_db();
        let store = SqliteEntityStore::new(&db);

        let fake_pid = ProjectId::new();
        let entity = SemanticEntity::new(
            fake_pid,
            ENTITY_KIND_FUNCTION.clone(),
            None,
            Some("orphan".into()),
        );
        let result = store.insert(&entity);
        assert!(result.is_err(), "FK violation should fail");
    }

    #[test]
    fn entity_store_list_by_project_no_filter() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        for name in ["alpha", "beta", "gamma"] {
            let e = SemanticEntity::new(pid, ENTITY_KIND_FUNCTION.clone(), None, Some(name.into()));
            store.insert(&e).unwrap();
        }
        let e = SemanticEntity::new(pid, ENTITY_KIND_TYPE.clone(), None, Some("MyType".into()));
        store.insert(&e).unwrap();

        let page = EntityPage {
            offset: 0,
            limit: 10,
            order_by: EntityColumn::Kind,
        };
        let results = store.list_by_project(pid, page, None).unwrap();
        assert_eq!(results.len(), 4);
    }

    #[test]
    fn entity_store_list_by_project_with_kind_filter() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        for _ in 0..3 {
            let e = SemanticEntity::new(pid, ENTITY_KIND_FUNCTION.clone(), None, None);
            store.insert(&e).unwrap();
        }
        for _ in 0..2 {
            let e = SemanticEntity::new(pid, ENTITY_KIND_TYPE.clone(), None, None);
            store.insert(&e).unwrap();
        }

        let page = EntityPage {
            offset: 0,
            limit: 10,
            order_by: EntityColumn::Kind,
        };
        let fns = store
            .list_by_project(pid, page, Some(&ENTITY_KIND_FUNCTION))
            .unwrap();
        assert_eq!(fns.len(), 3);
        for e in &fns {
            assert_eq!(e.kind, ENTITY_KIND_FUNCTION.clone());
        }
    }

    #[test]
    fn entity_store_pagination_stable() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        for i in 0..5 {
            let e = SemanticEntity::new(
                pid,
                ENTITY_KIND_FUNCTION.clone(),
                None,
                Some(format!("fn-{i:02}")),
            );
            store.insert(&e).unwrap();
        }

        let page1 = EntityPage {
            offset: 0,
            limit: 2,
            order_by: EntityColumn::Kind,
        };
        let page2 = EntityPage {
            offset: 2,
            limit: 2,
            order_by: EntityColumn::Kind,
        };
        let page3 = EntityPage {
            offset: 4,
            limit: 2,
            order_by: EntityColumn::Kind,
        };

        let r1 = store.list_by_project(pid, page1, None).unwrap();
        let r2 = store.list_by_project(pid, page2, None).unwrap();
        let r3 = store.list_by_project(pid, page3, None).unwrap();

        assert_eq!(r1.len(), 2);
        assert_eq!(r2.len(), 2);
        assert_eq!(r3.len(), 1);

        let mut all_ids: Vec<EntityId> = r1.iter().map(|e| e.id).collect();
        all_ids.extend(r2.iter().map(|e| e.id));
        all_ids.extend(r3.iter().map(|e| e.id));
        let unique: std::collections::HashSet<EntityId> = all_ids.iter().copied().collect();
        assert_eq!(unique.len(), 5, "pagination must return no duplicates");

        // Stable ordering: second fetch same order
        let page1_again = EntityPage {
            offset: 0,
            limit: 2,
            order_by: EntityColumn::Kind,
        };
        let r1_again = store.list_by_project(pid, page1_again, None).unwrap();
        assert_eq!(r1[0].id, r1_again[0].id);
        assert_eq!(r1[1].id, r1_again[1].id);
    }

    #[test]
    fn entity_store_list_by_stable_key() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        let loc = test_binary_location();
        let key = StableEntityKey::BinaryLocation(loc.clone());
        let e = SemanticEntity::new(
            pid,
            ENTITY_KIND_FUNCTION.clone(),
            Some(key.clone()),
            Some("findme".into()),
        );
        store.insert(&e).unwrap();

        let other = SemanticEntity::new(pid, ENTITY_KIND_TYPE.clone(), None, Some("other".into()));
        store.insert(&other).unwrap();

        let results = store.list_by_stable_key(pid, &key).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, e.id);
    }

    #[test]
    fn entity_store_count_by_project_kind() {
        let db = test_db();
        let pid = insert_project(&db);
        let store = SqliteEntityStore::new(&db);

        for _ in 0..3 {
            let e = SemanticEntity::new(pid, ENTITY_KIND_FUNCTION.clone(), None, None);
            store.insert(&e).unwrap();
        }
        for _ in 0..2 {
            let e = SemanticEntity::new(pid, ENTITY_KIND_EXTERNAL_FUNCTION.clone(), None, None);
            store.insert(&e).unwrap();
        }

        let fn_count = store
            .count_by_project_kind(pid, &ENTITY_KIND_FUNCTION)
            .unwrap();
        assert_eq!(fn_count, 3);

        let ext_count = store
            .count_by_project_kind(pid, &ENTITY_KIND_EXTERNAL_FUNCTION)
            .unwrap();
        assert_eq!(ext_count, 2);

        let zero_count = store.count_by_project_kind(pid, &ENTITY_KIND_TYPE).unwrap();
        assert_eq!(zero_count, 0);
    }

    #[test]
    fn entity_store_trait_object() {
        let db = test_db();
        let store = SqliteEntityStore::new(&db);
        fn _assert_trait_object(_: &dyn EntityStore) {}
        _assert_trait_object(&store);
    }
}
