use autore_schema::domain::NamespacedId;
use autore_schema::domain::records::{NativeArtifact, ProviderEntityAlias};
use autore_schema::ids::{ArtifactId, EntityId, NativeArtifactId, ProviderRunId};

use crate::storage::database::Database;

pub trait ProviderAliasStore: Send + Sync {
    fn insert_alias(&self, alias: &ProviderEntityAlias) -> crate::Result<()>;
    fn list_aliases_for_run(
        &self,
        run_id: ProviderRunId,
    ) -> crate::Result<Vec<ProviderEntityAlias>>;
    fn find_alias(
        &self,
        run_id: ProviderRunId,
        provider_identifier: &str,
    ) -> crate::Result<Option<ProviderEntityAlias>>;
    fn list_aliases_for_entity(
        &self,
        entity_id: EntityId,
    ) -> crate::Result<Vec<ProviderEntityAlias>>;
}

pub trait NativeArtifactStore: Send + Sync {
    fn insert(&self, artifact: &NativeArtifact) -> crate::Result<()>;
    fn get(&self, id: NativeArtifactId) -> crate::Result<Option<NativeArtifact>>;
    fn list_by_run(&self, run_id: ProviderRunId) -> crate::Result<Vec<NativeArtifact>>;
    fn list_by_subject_entity(&self, entity_id: EntityId) -> crate::Result<Vec<NativeArtifact>>;
}

pub struct SqliteAliasStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteAliasStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteAliasStore { db }
    }
}

fn entity_ids_to_json(ids: &[EntityId]) -> crate::Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn entity_ids_from_json(s: &str) -> Result<Vec<EntityId>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid entity IDs JSON: {e}"))
}

impl ProviderAliasStore for SqliteAliasStore<'_> {
    fn insert_alias(&self, alias: &ProviderEntityAlias) -> crate::Result<()> {
        let run_bytes = alias.provider_run.as_uuid().as_bytes().to_vec();
        let kind = alias.provider_kind.to_string();
        let entity_bytes = alias.entity.as_uuid().as_bytes().to_vec();

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO provider_entity_aliases \
             (provider_run, provider_kind, provider_identifier, entity) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![run_bytes, kind, alias.provider_identifier, entity_bytes,],
        )
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("UNIQUE constraint failed")
                || msg.contains("idx_aliases_provider_identifier")
            {
                crate::Error::Conflict(format!(
                    "duplicate alias for provider_identifier '{}' in run {}",
                    alias.provider_identifier, alias.provider_run
                ))
            } else if msg.contains("FOREIGN KEY constraint failed") {
                crate::Error::Database(format!("foreign key violation: {msg}"))
            } else {
                crate::Error::Database(msg)
            }
        })?;

        Ok(())
    }

    fn list_aliases_for_run(
        &self,
        run_id: ProviderRunId,
    ) -> crate::Result<Vec<ProviderEntityAlias>> {
        let run_bytes = run_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT provider_run, provider_kind, provider_identifier, entity \
                 FROM provider_entity_aliases \
                 WHERE provider_run = ?1 \
                 ORDER BY provider_identifier ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let aliases = stmt
            .query_map(rusqlite::params![run_bytes], row_to_alias)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(aliases)
    }

    fn find_alias(
        &self,
        run_id: ProviderRunId,
        provider_identifier: &str,
    ) -> crate::Result<Option<ProviderEntityAlias>> {
        let run_bytes = run_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT provider_run, provider_kind, provider_identifier, entity \
             FROM provider_entity_aliases \
             WHERE provider_run = ?1 AND provider_identifier = ?2",
            rusqlite::params![run_bytes, provider_identifier],
            row_to_alias,
        );

        match result {
            Ok(alias) => Ok(Some(alias)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_aliases_for_entity(
        &self,
        entity_id: EntityId,
    ) -> crate::Result<Vec<ProviderEntityAlias>> {
        let entity_bytes = entity_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT provider_run, provider_kind, provider_identifier, entity \
                 FROM provider_entity_aliases \
                 WHERE entity = ?1 \
                 ORDER BY provider_identifier ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let aliases = stmt
            .query_map(rusqlite::params![entity_bytes], row_to_alias)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(aliases)
    }
}

impl NativeArtifactStore for SqliteAliasStore<'_> {
    fn insert(&self, artifact: &NativeArtifact) -> crate::Result<()> {
        let id_bytes = artifact.id.as_uuid().as_bytes().to_vec();
        let run_bytes = artifact.provider_run.as_uuid().as_bytes().to_vec();
        let art_bytes = artifact.artifact.as_uuid().as_bytes().to_vec();
        let format = artifact.format.to_string();
        let entities_json = entity_ids_to_json(&artifact.subject_entities)?;

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO native_artifacts \
             (id, provider_run, artifact, format, subject_entities, description) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id_bytes,
                run_bytes,
                art_bytes,
                format,
                entities_json,
                artifact.description,
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

    fn get(&self, id: NativeArtifactId) -> crate::Result<Option<NativeArtifact>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, provider_run, artifact, format, subject_entities, description \
             FROM native_artifacts WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_native_artifact,
        );

        match result {
            Ok(a) => Ok(Some(a)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_by_run(&self, run_id: ProviderRunId) -> crate::Result<Vec<NativeArtifact>> {
        let run_bytes = run_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, provider_run, artifact, format, subject_entities, description \
                 FROM native_artifacts \
                 WHERE provider_run = ?1 \
                 ORDER BY id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let artifacts = stmt
            .query_map(rusqlite::params![run_bytes], row_to_native_artifact)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(artifacts)
    }

    fn list_by_subject_entity(&self, entity_id: EntityId) -> crate::Result<Vec<NativeArtifact>> {
        let conn = self.db.connection()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, provider_run, artifact, format, subject_entities, description \
                 FROM native_artifacts \
                 ORDER BY id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let target_uuid = *entity_id.as_uuid();
        let artifacts = stmt
            .query_map([], |row| {
                let na = row_to_native_artifact(row)?;
                Ok((na, ()))
            })
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .filter_map(|result| match result {
                Ok((na, ())) => {
                    if na
                        .subject_entities
                        .iter()
                        .any(|e| *e.as_uuid() == target_uuid)
                    {
                        Some(Ok(na))
                    } else {
                        None
                    }
                }
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(artifacts)
    }
}

fn row_to_alias(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderEntityAlias> {
    let run_bytes: Vec<u8> = row.get(0)?;
    let kind_str: String = row.get(1)?;
    let identifier: String = row.get(2)?;
    let entity_bytes: Vec<u8> = row.get(3)?;

    let provider_run =
        ProviderRunId::from_uuid(uuid::Uuid::from_slice(&run_bytes).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
        })?);

    let provider_kind = NamespacedId::parse(&kind_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let entity = EntityId::from_uuid(uuid::Uuid::from_slice(&entity_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Blob, Box::new(e))
    })?);

    Ok(ProviderEntityAlias {
        provider_run,
        provider_kind,
        provider_identifier: identifier,
        entity,
    })
}

fn row_to_native_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<NativeArtifact> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let run_bytes: Vec<u8> = row.get(1)?;
    let art_bytes: Vec<u8> = row.get(2)?;
    let format_str: String = row.get(3)?;
    let entities_json: String = row.get(4)?;
    let description: Option<String> = row.get(5)?;

    let id = NativeArtifactId::from_uuid(uuid::Uuid::from_slice(&id_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })?);

    let provider_run =
        ProviderRunId::from_uuid(uuid::Uuid::from_slice(&run_bytes).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e))
        })?);

    let artifact = ArtifactId::from_uuid(uuid::Uuid::from_slice(&art_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Blob, Box::new(e))
    })?);

    let format = NamespacedId::parse(&format_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;

    let subject_entities = entity_ids_from_json(&entities_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    Ok(NativeArtifact {
        id,
        provider_run,
        artifact,
        format,
        subject_entities,
        description,
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

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::ContentHash;
    use autore_schema::domain::records::{
        NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE, NATIVE_FORMAT_IDA_MICROCODE, PROVIDER_KIND_DECOMPILER,
    };
    use autore_schema::ids::{ArtifactId, ProjectId, ProviderId, ProviderRunId};

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

    fn insert_artifact(db: &Database, project: ProjectId) -> ArtifactId {
        let id = ArtifactId::new();
        let conn = db.connection().unwrap();
        let ch = ContentHash::sha256(b"test-artifact-content");
        conn.execute(
            "INSERT INTO stage0_artifacts (id, project_id, kind, hash_algorithm, \
             hash_digest, size, storage_kind, storage_path, created_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '{}')",
            rusqlite::params![
                id.as_uuid().as_bytes().as_slice(),
                project.as_uuid().as_bytes().as_slice(),
                "core.binary",
                "sha256",
                ch.digest.as_slice(),
                100i64,
                "managed-blob",
                "sha256/test",
                "2026-01-01T00:00:00Z",
            ],
        )
        .unwrap();
        id
    }

    // -- ProviderAliasStore tests --

    #[test]
    fn alias_store_insert_and_find() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let alias = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "sub_401000".to_string(),
            entity: entity_id,
        };
        store.insert_alias(&alias).unwrap();

        let found = store.find_alias(run_id, "sub_401000").unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.provider_identifier, "sub_401000");
        assert_eq!(found.entity, entity_id);
        assert_eq!(found.provider_run, run_id);
    }

    #[test]
    fn alias_store_find_not_found() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let store = SqliteAliasStore::new(&db);

        let found = store.find_alias(run_id, "nonexistent").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn alias_store_list_for_run() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        for name in ["sub_401000", "sub_401100", "sub_401200"] {
            let alias = ProviderEntityAlias {
                provider_run: run_id,
                provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
                provider_identifier: name.to_string(),
                entity: entity_id,
            };
            store.insert_alias(&alias).unwrap();
        }

        let aliases = store.list_aliases_for_run(run_id).unwrap();
        assert_eq!(aliases.len(), 3);
    }

    #[test]
    fn alias_store_list_for_entity() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let a1 = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "fn_a".to_string(),
            entity: e1,
        };
        let a2 = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "fn_b".to_string(),
            entity: e2,
        };
        let a3 = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "fn_c".to_string(),
            entity: e1,
        };
        store.insert_alias(&a1).unwrap();
        store.insert_alias(&a2).unwrap();
        store.insert_alias(&a3).unwrap();

        let for_e1 = store.list_aliases_for_entity(e1).unwrap();
        assert_eq!(for_e1.len(), 2);

        let for_e2 = store.list_aliases_for_entity(e2).unwrap();
        assert_eq!(for_e2.len(), 1);
    }

    #[test]
    fn alias_store_unique_constraint() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let a1 = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "dup_name".to_string(),
            entity: e1,
        };
        let a2 = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "dup_name".to_string(),
            entity: e2,
        };
        store.insert_alias(&a1).unwrap();
        let result = store.insert_alias(&a2);
        assert!(result.is_err(), "duplicate (run, identifier) should fail");
    }

    #[test]
    fn alias_store_fk_run_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let alias = ProviderEntityAlias {
            provider_run: ProviderRunId::new(),
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "orphan".to_string(),
            entity: entity_id,
        };
        let result = store.insert_alias(&alias);
        assert!(
            result.is_err(),
            "FK violation for non-existent run should fail"
        );
    }

    #[test]
    fn alias_store_fk_entity_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let store = SqliteAliasStore::new(&db);

        let alias = ProviderEntityAlias {
            provider_run: run_id,
            provider_kind: PROVIDER_KIND_DECOMPILER.clone(),
            provider_identifier: "orphan_entity".to_string(),
            entity: EntityId::new(),
        };
        let result = store.insert_alias(&alias);
        assert!(
            result.is_err(),
            "FK violation for non-existent entity should fail"
        );
    }

    // -- NativeArtifactStore tests --

    #[test]
    fn native_artifact_store_insert_and_get() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let artifact_id = insert_artifact(&db, pid);
        let entity_id = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let na = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: run_id,
            artifact: artifact_id,
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![entity_id],
            description: Some("decompiled main".to_string()),
        };
        store.insert(&na).unwrap();

        let fetched = store.get(na.id).unwrap().unwrap();
        assert_eq!(fetched.id, na.id);
        assert_eq!(fetched.provider_run, run_id);
        assert_eq!(fetched.artifact, artifact_id);
        assert_eq!(fetched.format, NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone());
        assert_eq!(fetched.subject_entities.len(), 1);
        assert_eq!(fetched.subject_entities[0], entity_id);
        assert_eq!(fetched.description, Some("decompiled main".to_string()));
    }

    #[test]
    fn native_artifact_store_get_not_found() {
        let db = test_db();
        let store = SqliteAliasStore::new(&db);
        let result = store.get(NativeArtifactId::new()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn native_artifact_store_list_by_run() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let artifact_id = insert_artifact(&db, pid);
        let store = SqliteAliasStore::new(&db);

        for _ in 0..3 {
            let na = NativeArtifact {
                id: NativeArtifactId::new(),
                provider_run: run_id,
                artifact: artifact_id,
                format: NATIVE_FORMAT_IDA_MICROCODE.clone(),
                subject_entities: vec![],
                description: None,
            };
            store.insert(&na).unwrap();
        }

        let results = store.list_by_run(run_id).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn native_artifact_store_list_by_subject_entity() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let artifact_id = insert_artifact(&db, pid);
        let e1 = insert_entity(&db, pid);
        let e2 = insert_entity(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let na1 = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: run_id,
            artifact: artifact_id,
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![e1],
            description: None,
        };
        let na2 = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: run_id,
            artifact: artifact_id,
            format: NATIVE_FORMAT_IDA_MICROCODE.clone(),
            subject_entities: vec![e1, e2],
            description: None,
        };
        let na3 = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: run_id,
            artifact: artifact_id,
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![],
            description: None,
        };
        store.insert(&na1).unwrap();
        store.insert(&na2).unwrap();
        store.insert(&na3).unwrap();

        let for_e1 = store.list_by_subject_entity(e1).unwrap();
        assert_eq!(for_e1.len(), 2);

        let for_e2 = store.list_by_subject_entity(e2).unwrap();
        assert_eq!(for_e2.len(), 1);
        assert_eq!(for_e2[0].id, na2.id);
    }

    #[test]
    fn native_artifact_store_fk_run_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let artifact_id = insert_artifact(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let na = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: ProviderRunId::new(),
            artifact: artifact_id,
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![],
            description: None,
        };
        let result = store.insert(&na);
        assert!(
            result.is_err(),
            "FK violation for non-existent run should fail"
        );
    }

    #[test]
    fn native_artifact_store_fk_artifact_enforced() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let store = SqliteAliasStore::new(&db);

        let na = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: run_id,
            artifact: ArtifactId::new(),
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![],
            description: None,
        };
        let result = store.insert(&na);
        assert!(
            result.is_err(),
            "FK violation for non-existent artifact should fail"
        );
    }

    #[test]
    fn native_artifact_store_null_description_empty_entities() {
        let db = test_db();
        let pid = insert_project(&db);
        let provider_id = insert_provider(&db);
        let run_id = insert_run(&db, pid, provider_id);
        let artifact_id = insert_artifact(&db, pid);
        let store = SqliteAliasStore::new(&db);

        let na = NativeArtifact {
            id: NativeArtifactId::new(),
            provider_run: run_id,
            artifact: artifact_id,
            format: NATIVE_FORMAT_IDA_HEXRAYS_PSEUDOCODE.clone(),
            subject_entities: vec![],
            description: None,
        };
        store.insert(&na).unwrap();

        let fetched = store.get(na.id).unwrap().unwrap();
        assert!(fetched.description.is_none());
        assert!(fetched.subject_entities.is_empty());
    }

    #[test]
    fn alias_store_trait_object() {
        let db = test_db();
        let store = SqliteAliasStore::new(&db);
        fn _assert_alias(_: &dyn ProviderAliasStore) {}
        fn _assert_native(_: &dyn NativeArtifactStore) {}
        _assert_alias(&store);
        _assert_native(&store);
    }
}
