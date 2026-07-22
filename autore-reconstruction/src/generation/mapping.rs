//! Helpers for building `RegisterGeneratedSourceMapping` commands.
//!
//! Each [`GeneratedSourceMappingIntent`] captures the intent to link
//! a canonical entity to its generated declaration (`.hpp`) and
//! definition (`.cpp`) artifacts.

use std::path::PathBuf;

use autore_app::application_service::requests::{
    RegisterGeneratedSourceMappingRequest, RegisterGeneratedSourceMappingResponse,
};
use autore_app::{ApplicationCommand, AutoReClient, CommandResult};
use autore_schema::ids::{EntityId, ProjectId};

/// Describes the intent to register a generated source mapping for
/// one canonical entity.
#[derive(Debug, Clone)]
pub struct GeneratedSourceMappingIntent {
    pub project: ProjectId,
    pub entity_id: EntityId,
    pub declaration_path: PathBuf,
    pub definition_path: PathBuf,
}

impl GeneratedSourceMappingIntent {
    /// Issues a `RegisterGeneratedSourceMapping` command through the
    /// client. The entity ID is used as the work-item identifier
    /// until work-graph wiring provides real work-item IDs.
    pub fn execute(&self, client: &dyn AutoReClient) -> autore_core::Result<String> {
        let req = RegisterGeneratedSourceMappingRequest {
            project: self.project,
            work_item_id: self.entity_id.to_string(),
        };
        let result = client.execute(ApplicationCommand::RegisterGeneratedSourceMapping(req))?;
        match result {
            CommandResult::GeneratedSourceMappingRegistered(
                RegisterGeneratedSourceMappingResponse { mapping_id },
            ) => Ok(mapping_id),
            other => Err(autore_core::Error::Validation(format!(
                "expected GeneratedSourceMappingRegistered, got: {other:?}"
            ))),
        }
    }
}
