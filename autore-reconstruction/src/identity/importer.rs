//! [`ObservationImporter`] — translates provider observations into
//! [`ApplicationCommand`]s routed through an [`AutoReClient`].

use autore_app::application_service::requests::{
    BlockWorkItemRequest, CreateWorkItemsRequest, ImportProviderRunResultRequest,
    RegisterEntityRequest,
};
use autore_app::{
    ApplicationCommand, ApplicationQuery, AutoReClient, EntitiesResponse, QueryResult,
};
use autore_core::Result;
use autore_schema::domain::StableEntityKey;
use autore_schema::ids::{ArtifactId, ProjectId, ProviderRunId};

use super::key::CanonicalEntityKey;
use super::payload::parse_observation_payload;
use super::routing::entity_kind_for_observation_kind;
use super::{Diagnostic, ObservationProduced, STALE_BLOCK_REASON};

/// Counts produced by a single [`ObservationImporter::import`] call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub entities_created: u64,
    pub entities_rematched: u64,
    pub stale_blocked: u64,
    pub investigations_created: u64,
}

/// Translates provider observations into [`ApplicationCommand`]s.
pub struct ObservationImporter<'a> {
    client: &'a dyn AutoReClient,
}

impl<'a> ObservationImporter<'a> {
    pub fn new(client: &'a dyn AutoReClient) -> Self {
        Self { client }
    }

    /// Imports a batch of observations, issuing one [`ApplicationCommand`]
    /// per entity. Entities that already exist by stable key receive an
    /// [`ApplicationCommand::ImportProviderRunResult`] instead of a fresh
    /// `RegisterEntity`.
    pub fn import(
        &self,
        observations: &[ObservationProduced],
        binary_revision_id: ArtifactId,
        _campaign_id: autore_schema::ids::ReconstructionCampaignId,
        project_id: ProjectId,
        run_id: ProviderRunId,
    ) -> Result<ImportSummary> {
        let mut summary = ImportSummary::default();
        for obs in observations {
            let entity_kind = entity_kind_for_observation_kind(&obs.observation_kind);
            let entities = parse_observation_payload(&obs.payload)?;
            for ent in entities {
                let key = CanonicalEntityKey {
                    binary_revision_id,
                    address_space: ent.address_space,
                    entry_address: ent.entry_address,
                    entity_kind: entity_kind.clone(),
                    provider_native_extension: ent.extension,
                };
                let stable = key.stable_key();
                let registered = self.is_entity_registered(project_id, &stable)?;
                if registered {
                    self.client
                        .execute(ApplicationCommand::ImportProviderRunResult(
                            ImportProviderRunResultRequest {
                                project: project_id,
                                run_id,
                            },
                        ))?;
                    summary.entities_rematched += 1;
                } else {
                    self.client.execute(ApplicationCommand::RegisterEntity(
                        RegisterEntityRequest {
                            project: project_id,
                            kind: key.entity_kind.as_str().to_string(),
                            stable_key: Some(stable),
                            display_name: ent.display_name,
                        },
                    ))?;
                    summary.entities_created += 1;
                }
            }
        }
        Ok(summary)
    }

    /// Handles a batch of stale diagnostics — does NOT delete entities;
    /// instead blocks the originating work item and spawns an investigation.
    pub fn import_stale_diagnostics(
        &self,
        diagnostics: &[Diagnostic],
        project_id: ProjectId,
        campaign_id: autore_schema::ids::ReconstructionCampaignId,
    ) -> Result<ImportSummary> {
        let mut summary = ImportSummary::default();
        for d in diagnostics {
            if d.code != "stale" {
                continue;
            }
            self.client
                .execute(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                    project: project_id,
                    work_item_id: d.request_id.clone(),
                    reason: STALE_BLOCK_REASON.to_string(),
                }))?;
            summary.stale_blocked += 1;
            self.client.execute(ApplicationCommand::CreateWorkItems(
                CreateWorkItemsRequest {
                    project: project_id,
                    campaign_id: campaign_id.to_string(),
                    descriptions: vec![format!(
                        "investigate stale observation from {}",
                        d.request_id
                    )],
                },
            ))?;
            summary.investigations_created += 1;
        }
        Ok(summary)
    }

    fn is_entity_registered(&self, project: ProjectId, key: &StableEntityKey) -> Result<bool> {
        let result = self.client.query(ApplicationQuery::ListEntities(
            autore_app::ListEntitiesQuery {
                project,
                offset: 0,
                limit: 1,
                kind_filter: None,
            },
        ))?;
        match result {
            QueryResult::Entities(EntitiesResponse { entities }) => {
                Ok(entities.iter().any(|e| e.stable_key.as_ref() == Some(key)))
            }
            _ => Ok(false),
        }
    }
}
