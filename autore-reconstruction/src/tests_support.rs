//! Test-only [`AutoReClient`] that records every command issued through
//! it and answers [`ApplicationQuery::ListEntities`] from the registered
//! set. Used by tests to verify that every canonical mutation goes
//! through a command — no direct SQL is issued.

use std::collections::HashMap;
use std::sync::Mutex;

use autore_app::application_service::requests::{
    BlockWorkItemResponse, CreateWorkItemsResponse, EntitiesResponse,
    ImportProviderRunResultResponse, InvalidateWorkItemResponse, RecordWorkDependencyResponse,
    RegisterEntityResponse,
};
use autore_app::{ApplicationCommand, ApplicationQuery, AutoReClient, CommandResult, QueryResult};
use autore_core::{Error, Result};
use autore_events::project_event_service::ProjectEventSubscription;
use autore_schema::domain::records::{ProjectEvent, SemanticEntity};
use autore_schema::domain::{MetadataMap, NamespacedId, StableEntityKey, Timestamp};
use autore_schema::ids::{EntityId, ProjectId, WorkItemId};

/// Records every [`ApplicationCommand`] issued through it.
#[derive(Debug, Default)]
pub struct RecordingAutoReClient {
    commands: Mutex<Vec<ApplicationCommand>>,
    /// `(project_id, stable_key_json) → entity` for rematch detection.
    registered: Mutex<HashMap<(ProjectId, String), SemanticEntity>>,
}

impl RecordingAutoReClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn commands(&self) -> Vec<ApplicationCommand> {
        self.commands.lock().expect("lock").clone()
    }

    pub fn count<F: Fn(&ApplicationCommand) -> bool>(&self, pred: F) -> usize {
        self.commands
            .lock()
            .expect("lock")
            .iter()
            .filter(|c| pred(c))
            .count()
    }

    pub fn find_by_stable_key(
        &self,
        project: ProjectId,
        key: &StableEntityKey,
    ) -> Option<SemanticEntity> {
        let json = serde_json::to_string(key).ok()?;
        self.registered
            .lock()
            .expect("lock")
            .get(&(project, json))
            .cloned()
    }
}

impl AutoReClient for RecordingAutoReClient {
    fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
        let result = match &command {
            ApplicationCommand::RegisterEntity(req) => {
                let entity = SemanticEntity {
                    id: EntityId::new(),
                    project: req.project,
                    kind: NamespacedId::parse(&req.kind).map_err(|e| Error::Validation(e.0))?,
                    stable_key: req.stable_key.clone(),
                    display_name: req.display_name.clone(),
                    created_at: Timestamp::now(),
                    metadata: MetadataMap::new(),
                };
                if let Some(key) = req.stable_key.as_ref() {
                    let json = serde_json::to_string(key)
                        .map_err(|e| Error::Serialization(e.to_string()))?;
                    self.registered
                        .lock()
                        .expect("lock")
                        .insert((req.project, json), entity.clone());
                }
                CommandResult::EntityRegistered(RegisterEntityResponse { entity })
            }
            ApplicationCommand::ImportProviderRunResult(req) => {
                CommandResult::ProviderRunResultImported(ImportProviderRunResultResponse {
                    run_id: req.run_id,
                })
            }
            ApplicationCommand::BlockWorkItem(req) => {
                CommandResult::WorkItemBlocked(BlockWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            ApplicationCommand::CreateWorkItems(req) => {
                let ids: Vec<WorkItemId> =
                    req.descriptions.iter().map(|_| WorkItemId::new()).collect();
                CommandResult::WorkItemsCreated(CreateWorkItemsResponse {
                    work_item_ids: ids.iter().map(|id| id.to_string()).collect(),
                })
            }
            ApplicationCommand::RecordWorkDependency(req) => {
                CommandResult::WorkDependencyRecorded(RecordWorkDependencyResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            ApplicationCommand::InvalidateWorkItem(req) => {
                CommandResult::WorkItemInvalidated(InvalidateWorkItemResponse {
                    work_item_id: req.work_item_id.clone(),
                })
            }
            _ => {
                return Err(Error::Unsupported(format!(
                    "recording client does not handle {:?}",
                    std::mem::discriminant(&command)
                )));
            }
        };
        self.commands.lock().expect("lock").push(command);
        Ok(result)
    }

    fn query(&self, query: ApplicationQuery) -> Result<QueryResult> {
        match query {
            ApplicationQuery::ListEntities(q) => {
                let guard = self.registered.lock().expect("lock");
                let entities: Vec<SemanticEntity> = guard
                    .iter()
                    .filter(|((pid, _), _)| *pid == q.project)
                    .map(|(_, e)| e.clone())
                    .collect();
                Ok(QueryResult::Entities(EntitiesResponse { entities }))
            }
            _ => Err(Error::Unsupported(format!(
                "recording client does not answer {:?}",
                std::mem::discriminant(&query)
            ))),
        }
    }

    fn events_after(
        &self,
        _project: ProjectId,
        _sequence: u64,
        _limit: usize,
    ) -> Result<Vec<ProjectEvent>> {
        Ok(Vec::new())
    }

    fn subscribe_events(
        &self,
        _project: ProjectId,
        _after: u64,
    ) -> Result<ProjectEventSubscription> {
        Err(Error::Unsupported(
            "recording client has no event stream".into(),
        ))
    }
}
