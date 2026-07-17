use autore_schema::domain::records::Project;
use autore_schema::domain::{MetadataMap, SchemaVersion, Timestamp};
use autore_schema::ids::ProjectId;

use crate::storage::database::Database;

pub enum ProjectColumn {
    Name,
    CreatedAt,
    UpdatedAt,
}

pub struct Page {
    pub offset: u32,
    pub limit: u32,
    pub order_by: ProjectColumn,
}

pub trait ProjectStore: Send + Sync {
    fn insert_project(&self, p: &Project) -> crate::Result<()>;
    fn get_project(&self, id: ProjectId) -> crate::Result<Option<Project>>;
    fn list_projects(&self, page: Page) -> crate::Result<Vec<Project>>;
}

impl ProjectColumn {
    fn as_sql(&self) -> &'static str {
        match self {
            ProjectColumn::Name => "name",
            ProjectColumn::CreatedAt => "created_at",
            ProjectColumn::UpdatedAt => "updated_at",
        }
    }
}

pub struct SqliteProjectStore<'a> {
    db: &'a Database,
}

impl<'a> SqliteProjectStore<'a> {
    pub fn new(db: &'a Database) -> Self {
        SqliteProjectStore { db }
    }
}

impl ProjectStore for SqliteProjectStore<'_> {
    fn insert_project(&self, p: &Project) -> crate::Result<()> {
        let id_bytes = p.id.as_uuid().as_bytes();
        let schema_version = p.schema_version.to_string();
        let created_at = p.created_at.to_string();
        let updated_at = p.updated_at.to_string();
        let metadata = serde_json::to_string(&p.metadata)
            .map_err(|e| crate::Error::Serialization(e.to_string()))?;

        let conn = self.db.connection()?;
        conn.execute(
            "INSERT INTO projects (id, name, schema_version, created_at, updated_at, metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                id_bytes.as_slice(),
                p.name,
                schema_version,
                created_at,
                updated_at,
                metadata,
            ],
        )
        .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(())
    }

    fn get_project(&self, id: ProjectId) -> crate::Result<Option<Project>> {
        let id_bytes = id.as_uuid().as_bytes();
        let conn = self.db.connection()?;

        let result = conn.query_row(
            "SELECT id, name, schema_version, created_at, updated_at, metadata \
             FROM projects WHERE id = ?1",
            rusqlite::params![id_bytes.as_slice()],
            row_to_project,
        );

        match result {
            Ok(project) => Ok(Some(project)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(crate::Error::Database(e.to_string())),
        }
    }

    fn list_projects(&self, page: Page) -> crate::Result<Vec<Project>> {
        let order_col = page.order_by.as_sql();
        let sql = format!(
            "SELECT id, name, schema_version, created_at, updated_at, metadata \
             FROM projects ORDER BY {order_col} ASC LIMIT ?1 OFFSET ?2"
        );

        let conn = self.db.connection()?;
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        let projects = stmt
            .query_map(rusqlite::params![page.limit, page.offset], row_to_project)
            .map_err(|e| crate::Error::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| crate::Error::Database(e.to_string()))?;

        Ok(projects)
    }
}

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let id_bytes: Vec<u8> = row.get(0)?;
    let name: String = row.get(1)?;
    let schema_version_str: String = row.get(2)?;
    let created_at_str: String = row.get(3)?;
    let updated_at_str: String = row.get(4)?;
    let metadata_str: String = row.get(5)?;

    let uuid = uuid::Uuid::from_slice(&id_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(e))
    })?;
    let id = ProjectId::from_uuid(uuid);

    let schema_version = parse_schema_version(&schema_version_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let created_at = parse_timestamp(&created_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;
    let updated_at = parse_timestamp(&updated_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(ParseError(e)),
        )
    })?;

    let metadata: MetadataMap = serde_json::from_str(&metadata_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(Project {
        id,
        name,
        schema_version,
        created_at,
        updated_at,
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

fn parse_schema_version(s: &str) -> Result<SchemaVersion, String> {
    let mut parts = s.splitn(2, '.');
    let major = parts
        .next()
        .ok_or("missing major")?
        .parse::<u32>()
        .map_err(|e| format!("invalid major: {e}"))?;
    let minor = parts
        .next()
        .ok_or("missing minor")?
        .parse::<u32>()
        .map_err(|e| format!("invalid minor: {e}"))?;
    Ok(SchemaVersion::new(major, minor))
}

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

    #[test]
    fn project_store_insert_and_get() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);

        let project = Project::new("test-project");
        let pid = project.id;
        store.insert_project(&project).unwrap();

        let fetched = store.get_project(pid).unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, pid);
        assert_eq!(fetched.name, "test-project");
        assert_eq!(fetched.schema_version, SchemaVersion::new(2, 0));
        assert!(fetched.metadata.is_empty());
    }

    #[test]
    fn project_store_get_not_found() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);

        let result = store.get_project(ProjectId::new()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn project_store_insert_duplicate_fails() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);

        let project = Project::new("dup-test");
        store.insert_project(&project).unwrap();
        let result = store.insert_project(&project);
        assert!(result.is_err(), "duplicate insert should fail");
    }

    #[test]
    fn project_store_list_projects() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);

        for name in ["alpha", "beta", "gamma"] {
            let p = Project::new(name);
            store.insert_project(&p).unwrap();
        }

        let page = Page {
            offset: 0,
            limit: 10,
            order_by: ProjectColumn::Name,
        };
        let projects = store.list_projects(page).unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].name, "alpha");
        assert_eq!(projects[1].name, "beta");
        assert_eq!(projects[2].name, "gamma");
    }

    #[test]
    fn project_store_list_pagination() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);

        for i in 0..5 {
            let p = Project::new(format!("proj-{i:02}"));
            store.insert_project(&p).unwrap();
        }

        let page = Page {
            offset: 2,
            limit: 2,
            order_by: ProjectColumn::Name,
        };
        let projects = store.list_projects(page).unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "proj-02");
        assert_eq!(projects[1].name, "proj-03");
    }

    #[test]
    fn project_store_roundtrip_with_metadata() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);

        let mut project = Project::new("metadata-test");
        let ns_key = autore_schema::domain::NamespacedId::parse("core.test").unwrap();
        let ext_data = autore_schema::domain::ExtensionData::new(
            ns_key.clone(),
            1,
            serde_json::Value::Bool(true),
        );
        project.metadata.insert(ns_key.clone(), ext_data);

        let pid = project.id;
        store.insert_project(&project).unwrap();

        let fetched = store.get_project(pid).unwrap().unwrap();
        assert!(!fetched.metadata.is_empty());
        assert!(fetched.metadata.contains_key(&ns_key));
    }

    #[test]
    fn project_store_trait_object() {
        let db = test_db();
        let store = SqliteProjectStore::new(&db);
        fn _assert_trait_object(_: &dyn ProjectStore) {}
        _assert_trait_object(&store);
    }
}
