use autore_schema::domain::records::{
    EnvironmentIdentity, Provider, ProviderRun, ProviderRunStatus,
};
use autore_schema::domain::{ContentHash, NamespacedId, Timestamp};
use autore_schema::ids::{ArtifactId, ProjectId, ProviderId, ProviderRunId};

use crate::storage::database::Database;

pub struct RunQuery {
    pub project_id: ProjectId,
    pub status_filter: Option<ProviderRunStatus>,
    pub provider_filter: Option<ProviderId>,
    pub offset: u32,
    pub limit: u32,
}

pub trait ProviderStore: Send + Sync {
    fn insert_provider(&self, provider: &Provider) -> crate::Result<()>;
    fn get_provider(&self, id: ProviderId) -> crate::Result<Option<Provider>>;
    fn list_providers(&self) -> crate::Result<Vec<Provider>>;

    fn start_run(&self, run: &ProviderRun) -> crate::Result<()>;
    fn complete_run(
        &self,
        run_id: ProviderRunId,
        target: ProviderRunStatus,
    ) -> crate::Result<()>;
    fn get_run(&self, id: ProviderRunId) -> crate::Result<Option<ProviderRun>>;
    fn list_runs(&self, query: RunQuery) -> crate::Result<Vec<ProviderRun>>;
}

pub struct SqliteProviderStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteProviderStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteProviderStore { db }
    }
}

fn status_to_str(s: ProviderRunStatus) -> &'static str {
    match s {
        ProviderRunStatus::Running => "Running",
        ProviderRunStatus::Completed => "Completed",
        ProviderRunStatus::Failed => "Failed",
        ProviderRunStatus::Cancelled => "Cancelled",
        ProviderRunStatus::Inconclusive => "Inconclusive",
    }
}

fn str_to_status(s: &str) -> Result<ProviderRunStatus, String> {
    match s {
        "Running" => Ok(ProviderRunStatus::Running),
        "Completed" => Ok(ProviderRunStatus::Completed),
        "Failed" => Ok(ProviderRunStatus::Failed),
        "Cancelled" => Ok(ProviderRunStatus::Cancelled),
        "Inconclusive" => Ok(ProviderRunStatus::Inconclusive),
        other => Err(format!("unknown provider run status: {other}")),
    }
}

fn parse_timestamp(s: &str) -> Result<Timestamp, String> {
    let dt = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| format!("invalid timestamp: {e}"))?;
    Ok(Timestamp::from_offset_datetime(dt))
}

fn content_hash_to_json(ch: &ContentHash) -> crate::Result<String> {
    serde_json::to_string(ch).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn content_hash_from_json(s: &str) -> Result<ContentHash, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid content hash JSON: {e}"))
}

fn content_hash_to_db(ch: &Option<ContentHash>) -> crate::Result<Option<String>> {
    ch.as_ref()
        .map(content_hash_to_json)
        .transpose()
}

fn artifact_ids_to_json(ids: &[ArtifactId]) -> crate::Result<String> {
    serde_json::to_string(ids).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn artifact_ids_from_json(s: &str) -> Result<Vec<ArtifactId>, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid artifact IDs JSON: {e}"))
}

fn env_to_json(env: &EnvironmentIdentity) -> crate::Result<String> {
    serde_json::to_string(env).map_err(|e| crate::Error::Serialization(e.to_string()))
}

fn env_from_json(s: &str) -> Result<EnvironmentIdentity, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid environment JSON: {e}"))
}

impl ProviderStore for SqliteProviderStore<'_> {
    fn insert_provider(&self, provider: &Provider) -> crate::Result<()> {
        let id_bytes = provider.id.as_uuid().as_bytes().to_vec();
        let package_bytes = provider
            .package_id
            .map(|pid| pid.as_uuid().as_bytes().to_vec());
        let kind = provider.kind.to_string();
        let exec_hash = content_hash_to_db(&provider.executable_hash)?;

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO providers (id, package_id, name, kind, version, executable_hash) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id_bytes,
                package_bytes,
                provider.name,
                kind,
                provider.version,
                exec_hash,
            ],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }

    fn get_provider(&self, id: ProviderId) -> crate::Result<Option<Provider>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, package_id, name, kind, version, executable_hash \
             FROM providers WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_provider,
        );

        match result {
            Ok(p) => Ok(Some(p)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_providers(&self) -> crate::Result<Vec<Provider>> {
        let conn = self.db.connection()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, package_id, name, kind, version, executable_hash \
                 FROM providers ORDER BY name ASC, id ASC",
            )
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let providers = stmt
            .query_map([], row_to_provider)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(providers)
    }

    fn start_run(&self, run: &ProviderRun) -> crate::Result<()> {
        let id_bytes = run.id.as_uuid().as_bytes().to_vec();
        let project_bytes = run.project.as_uuid().as_bytes().to_vec();
        let provider_bytes = run.provider.as_uuid().as_bytes().to_vec();
        let operation = run.operation.to_string();
        let input_json = artifact_ids_to_json(&run.input_artifacts)?;
        let config_artifact_bytes = run
            .configuration_artifact
            .map(|a| a.as_uuid().as_bytes().to_vec());
        let config_hash = content_hash_to_json(&run.configuration_hash)?;
        let env_json = env_to_json(&run.environment)?;
        let started_at = run.started_at.to_string();
        let completed_at = run.completed_at.map(|t| t.to_string());
        let status = status_to_str(run.status);

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO provider_runs \
             (id, project_id, provider_id, operation, input_artifacts, \
              configuration_artifact, configuration_hash, environment, \
              started_at, completed_at, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id_bytes,
                project_bytes,
                provider_bytes,
                operation,
                input_json,
                config_artifact_bytes,
                config_hash,
                env_json,
                started_at,
                completed_at,
                status,
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

    fn complete_run(
        &self,
        run_id: ProviderRunId,
        target: ProviderRunStatus,
    ) -> crate::Result<()> {
        let id_bytes = run_id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let current_status_str: String = conn
            .query_row(
                "SELECT status FROM provider_runs WHERE id = ?1",
                rusqlite::params![id_bytes],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    crate::Error::NotFound(format!("provider run {run_id}"))
                }
                other => crate::Error::Database(other.to_string()),
            })?;

        let current = str_to_status(&current_status_str)
            .map_err(crate::Error::Serialization)?;

        current.transition(target)?;

        let now = Timestamp::now().to_string();
        let target_str = status_to_str(target);
        conn.execute(
            "UPDATE provider_runs SET status = ?1, completed_at = ?2 WHERE id = ?3",
            rusqlite::params![target_str, now, id_bytes],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }

    fn get_run(&self, id: ProviderRunId) -> crate::Result<Option<ProviderRun>> {
        let id_bytes = id.as_uuid().as_bytes().to_vec();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, project_id, provider_id, operation, input_artifacts, \
             configuration_artifact, configuration_hash, environment, \
             started_at, completed_at, status \
             FROM provider_runs WHERE id = ?1",
            rusqlite::params![id_bytes],
            row_to_provider_run,
        );

        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_runs(&self, query: RunQuery) -> crate::Result<Vec<ProviderRun>> {
        let conn = self.db.connection()?;

        let mut conditions = vec!["project_id = ?1".to_string()];
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(query.project_id.as_uuid().as_bytes().to_vec()),
        ];
        let mut idx = 2;

        if let Some(status) = &query.status_filter {
            conditions.push(format!("status = ?{idx}"));
            params.push(Box::new(status_to_str(*status).to_string()));
            idx += 1;
        }

        if let Some(provider_id) = &query.provider_filter {
            conditions.push(format!("provider_id = ?{idx}"));
            params.push(Box::new(provider_id.as_uuid().as_bytes().to_vec()));
            idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            "SELECT id, project_id, provider_id, operation, input_artifacts, \
             configuration_artifact, configuration_hash, environment, \
             started_at, completed_at, status \
             FROM provider_runs \
             WHERE {where_clause} \
             ORDER BY started_at ASC, id ASC LIMIT ?{idx} OFFSET ?{}",
            idx + 1,
        );

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut all_params: Vec<&dyn rusqlite::types::ToSql> = param_refs;
        all_params.push(&query.limit);
        all_params.push(&query.offset);

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let runs = stmt
            .query_map(all_params.as_slice(), row_to_provider_run)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(runs)
    }
}

fn row_to_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<Provider> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let package_bytes: Option<Vec<u8>> = row.get(1)?;
    let name: String = row.get(2)?;
    let kind_str: String = row.get(3)?;
    let version: String = row.get(4)?;
    let exec_hash_str: Option<String> = row.get(5)?;

    let id_uuid = uuid::Uuid::from_slice(&id_bytes)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?;
    let id = ProviderId::from_uuid(id_uuid);

    let package_id = package_bytes
        .map(|bytes| {
            let uuid = uuid::Uuid::from_slice(&bytes)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?;
            Ok::<_, rusqlite::Error>(autore_schema::ids::PackageId::from_uuid(uuid))
        })
        .transpose()?;

    let kind = NamespacedId::parse(&kind_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;

    let executable_hash = exec_hash_str
        .map(|s| content_hash_from_json(&s))
        .transpose()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(Provider {
        id,
        package_id,
        name,
        kind,
        version,
        executable_hash,
    })
}

fn row_to_provider_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderRun> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let project_bytes: Vec<u8> = row.get(1)?;
    let provider_bytes: Vec<u8> = row.get(2)?;
    let operation_str: String = row.get(3)?;
    let input_json: String = row.get(4)?;
    let config_artifact_bytes: Option<Vec<u8>> = row.get(5)?;
    let config_hash_str: String = row.get(6)?;
    let env_json: String = row.get(7)?;
    let started_at_str: String = row.get(8)?;
    let completed_at_str: Option<String> = row.get(9)?;
    let status_str: String = row.get(10)?;

    let id = ProviderRunId::from_uuid(
        uuid::Uuid::from_slice(&id_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e)))?,
    );
    let project = ProjectId::from_uuid(
        uuid::Uuid::from_slice(&project_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Blob, Box::new(e)))?,
    );
    let provider = ProviderId::from_uuid(
        uuid::Uuid::from_slice(&provider_bytes)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Blob, Box::new(e)))?,
    );

    let operation = NamespacedId::parse(&operation_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;

    let input_artifacts = artifact_ids_from_json(&input_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let configuration_artifact = config_artifact_bytes
        .map(|bytes| {
            let uuid = uuid::Uuid::from_slice(&bytes)
                .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Blob, Box::new(e)))?;
            Ok::<_, rusqlite::Error>(ArtifactId::from_uuid(uuid))
        })
        .transpose()?;

    let configuration_hash = content_hash_from_json(&config_hash_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let environment = env_from_json(&env_json)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let started_at = parse_timestamp(&started_at_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let completed_at = completed_at_str
        .map(|s| parse_timestamp(&s))
        .transpose()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    let status = str_to_status(&status_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(ParseError(e))))?;

    Ok(ProviderRun {
        id,
        project,
        provider,
        operation,
        input_artifacts,
        configuration_artifact,
        configuration_hash,
        environment,
        started_at,
        completed_at,
        status,
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
    use autore_schema::domain::records::{
        PROVIDER_KIND_DECOMPILER, PROVIDER_KIND_DISASSEMBLER,
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

    fn test_provider() -> Provider {
        Provider::new("IDA Pro", PROVIDER_KIND_DECOMPILER.clone(), "8.3")
    }

    fn test_environment() -> EnvironmentIdentity {
        EnvironmentIdentity {
            operating_system: NamespacedId::parse("core.linux").unwrap(),
            architecture: NamespacedId::parse("core.x86-64").unwrap(),
            isolation_backend: None,
            image_digest: None,
            extension: None,
        }
    }

    fn test_run(project: ProjectId, provider: ProviderId) -> ProviderRun {
        ProviderRun {
            id: ProviderRunId::new(),
            project,
            provider,
            operation: NamespacedId::parse("core.disassemble").unwrap(),
            input_artifacts: vec![],
            configuration_artifact: None,
            configuration_hash: ContentHash::sha256(b"test-config"),
            environment: test_environment(),
            started_at: Timestamp::now(),
            completed_at: None,
            status: ProviderRunStatus::Running,
        }
    }

    #[test]
    fn provider_store_insert_and_get() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let fetched = store.get_provider(p.id).unwrap().unwrap();
        assert_eq!(fetched.id, p.id);
        assert_eq!(fetched.name, "IDA Pro");
        assert_eq!(fetched.kind, PROVIDER_KIND_DECOMPILER.clone());
        assert_eq!(fetched.version, "8.3");
        assert!(fetched.executable_hash.is_none());
    }

    #[test]
    fn provider_store_get_not_found() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        assert!(store.get_provider(ProviderId::new()).unwrap().is_none());
    }

    #[test]
    fn provider_store_list_providers() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);

        let p1 = Provider::new("Ghidra", PROVIDER_KIND_DECOMPILER.clone(), "11.0");
        let p2 = Provider::new("IDA Pro", PROVIDER_KIND_DECOMPILER.clone(), "8.3");
        store.insert_provider(&p1).unwrap();
        store.insert_provider(&p2).unwrap();

        let all = store.list_providers().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "Ghidra");
        assert_eq!(all[1].name, "IDA Pro");
    }

    #[test]
    fn provider_store_start_and_get_run() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let run = test_run(pid, p.id);
        store.start_run(&run).unwrap();

        let fetched = store.get_run(run.id).unwrap().unwrap();
        assert_eq!(fetched.id, run.id);
        assert_eq!(fetched.project, pid);
        assert_eq!(fetched.provider, p.id);
        assert_eq!(fetched.status, ProviderRunStatus::Running);
        assert!(fetched.completed_at.is_none());
    }

    #[test]
    fn provider_store_complete_run() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let run = test_run(pid, p.id);
        store.start_run(&run).unwrap();

        store.complete_run(run.id, ProviderRunStatus::Completed).unwrap();

        let fetched = store.get_run(run.id).unwrap().unwrap();
        assert_eq!(fetched.status, ProviderRunStatus::Completed);
        assert!(fetched.completed_at.is_some());
    }

    #[test]
    fn provider_run_state_transitions() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let run1 = test_run(pid, p.id);
        store.start_run(&run1).unwrap();
        store.complete_run(run1.id, ProviderRunStatus::Completed).unwrap();

        let run2 = test_run(pid, p.id);
        store.start_run(&run2).unwrap();
        store.complete_run(run2.id, ProviderRunStatus::Failed).unwrap();

        let run3 = test_run(pid, p.id);
        store.start_run(&run3).unwrap();
        store.complete_run(run3.id, ProviderRunStatus::Cancelled).unwrap();

        let run4 = test_run(pid, p.id);
        store.start_run(&run4).unwrap();
        store.complete_run(run4.id, ProviderRunStatus::Inconclusive).unwrap();
    }

    #[test]
    fn provider_run_state_transitions_reject_invalid() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let run = test_run(pid, p.id);
        store.start_run(&run).unwrap();
        store.complete_run(run.id, ProviderRunStatus::Completed).unwrap();

        let result = store.complete_run(run.id, ProviderRunStatus::Failed);
        assert!(result.is_err(), "completed -> failed should be rejected");
    }

    #[test]
    fn provider_store_fk_enforced() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let fake_provider = ProviderId::new();
        let run = test_run(pid, fake_provider);
        let result = store.start_run(&run);
        assert!(result.is_err(), "FK violation for non-existent provider should fail");
    }

    #[test]
    fn provider_store_fk_project_enforced() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let fake_project = ProjectId::new();
        let run = test_run(fake_project, p.id);
        let result = store.start_run(&run);
        assert!(result.is_err(), "FK violation for non-existent project should fail");
    }

    #[test]
    fn provider_store_list_runs_filter_by_status() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        let run1 = test_run(pid, p.id);
        store.start_run(&run1).unwrap();
        store.complete_run(run1.id, ProviderRunStatus::Completed).unwrap();

        let run2 = test_run(pid, p.id);
        store.start_run(&run2).unwrap();

        let query = RunQuery {
            project_id: pid,
            status_filter: Some(ProviderRunStatus::Running),
            provider_filter: None,
            offset: 0,
            limit: 10,
        };
        let running = store.list_runs(query).unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].status, ProviderRunStatus::Running);

        let query = RunQuery {
            project_id: pid,
            status_filter: Some(ProviderRunStatus::Completed),
            provider_filter: None,
            offset: 0,
            limit: 10,
        };
        let completed = store.list_runs(query).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].status, ProviderRunStatus::Completed);
    }

    #[test]
    fn provider_store_list_runs_filter_by_provider() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p1 = Provider::new("IDA", PROVIDER_KIND_DECOMPILER.clone(), "8.3");
        let p2 = Provider::new("Ghidra", PROVIDER_KIND_DISASSEMBLER.clone(), "11.0");
        store.insert_provider(&p1).unwrap();
        store.insert_provider(&p2).unwrap();

        let run1 = test_run(pid, p1.id);
        store.start_run(&run1).unwrap();
        let run2 = test_run(pid, p2.id);
        store.start_run(&run2).unwrap();

        let query = RunQuery {
            project_id: pid,
            status_filter: None,
            provider_filter: Some(p1.id),
            offset: 0,
            limit: 10,
        };
        let results = store.list_runs(query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].provider, p1.id);
    }

    #[test]
    fn provider_store_list_runs_pagination() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        let pid = insert_project(&db);

        let p = test_provider();
        store.insert_provider(&p).unwrap();

        for _ in 0..5 {
            let run = test_run(pid, p.id);
            store.start_run(&run).unwrap();
        }

        let query = RunQuery {
            project_id: pid,
            status_filter: None,
            provider_filter: None,
            offset: 0,
            limit: 2,
        };
        let page1 = store.list_runs(query).unwrap();
        assert_eq!(page1.len(), 2);

        let query = RunQuery {
            project_id: pid,
            status_filter: None,
            provider_filter: None,
            offset: 2,
            limit: 2,
        };
        let page2 = store.list_runs(query).unwrap();
        assert_eq!(page2.len(), 2);

        let mut all_ids: Vec<ProviderRunId> = page1.iter().map(|r| r.id).collect();
        all_ids.extend(page2.iter().map(|r| r.id));
        let unique: std::collections::HashSet<ProviderRunId> = all_ids.iter().copied().collect();
        assert_eq!(unique.len(), 4, "pagination must return no duplicates");
    }

    #[test]
    fn provider_store_complete_run_not_found() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);

        let result = store.complete_run(ProviderRunId::new(), ProviderRunStatus::Completed);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("not found"), "error should mention not found: {msg}");
    }

    #[test]
    fn provider_store_trait_object() {
        let db = test_db();
        let store = SqliteProviderStore::new(&db);
        fn _assert_trait_object(_: &dyn ProviderStore) {}
        _assert_trait_object(&store);
    }
}
