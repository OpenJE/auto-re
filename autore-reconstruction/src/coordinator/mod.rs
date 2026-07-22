//! Durable coordinator loop implementing spec §14.1.
//!
//! A [`Coordinator`] performs one deterministic tick at a time. Each tick runs
//! reconciliation, health refresh, structure refresh, dependency update,
//! invalidation, promotion, selection, and handler dispatch in that order. All
//! durable side effects issue [`ApplicationCommand`]s through the configured
//! [`AutoReClient`]; no tick writes directly to project storage.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use autore_app::AutoReClient;
use autore_app::application_service::requests::{
    ApplicationCommand, BlockWorkItemRequest, ImportProviderRunResultRequest,
    InvalidateWorkItemRequest, PromoteWorkItemRequest, RecordWorkDependencyRequest,
    RequeueWorkItemRequest,
};
use autore_core::Result;
use autore_schema::domain::records::WorkItemState;
use autore_schema::ids::ProjectId;

use crate::coordinator::handlers::{classify_work_item, dispatch};

pub mod handlers;
pub mod policy;
pub mod state;

pub use handlers::{DispatchKind, HandlerOutput, WorkKindHandlers};
pub use policy::{CompletionPolicy, NoProgressDetector, NoProgressKind};
pub use state::{CoordinatorState, CoordinatorWorkItem, ProviderHealth};

/// Result returned by a single coordinator tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TickResult {
    /// A work item was processed; includes its id.
    Processed(String),
    /// No dispatchable work was available.
    #[default]
    NoWork,
    /// All required work items are terminal.
    Complete,
    /// A work item was blocked this tick.
    Blocked(String),
    /// The coordinator was cancelled before or during the tick.
    Cancelled,
}

/// Coordinator configuration.
#[derive(Debug, Clone, Copy)]
pub struct CoordinatorConfig {
    /// Number of identical raw-response hashes that trigger a block.
    pub no_progress_threshold: usize,
    /// Max promotions per tick.
    pub max_promotions_per_tick: usize,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            no_progress_threshold: 3,
            max_promotions_per_tick: 100,
        }
    }
}

/// Durable coordinator implementing spec §14.1.
pub struct Coordinator<H: WorkKindHandlers> {
    pub project_id: ProjectId,
    pub campaign_id: String,
    pub client: Arc<dyn AutoReClient>,
    pub config: CoordinatorConfig,
    pub state: CoordinatorState,
    pub handlers: H,
    pub cancel: CancellationToken,
    detector: NoProgressDetector,
}

impl<H: WorkKindHandlers> Coordinator<H> {
    /// Creates a new coordinator.
    pub fn new(
        project_id: ProjectId,
        campaign_id: String,
        client: Arc<dyn AutoReClient>,
        handlers: H,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            project_id,
            campaign_id,
            client,
            config: CoordinatorConfig::default(),
            state: CoordinatorState::default(),
            handlers,
            cancel,
            detector: NoProgressDetector::default(),
        }
    }

    /// Creates a coordinator with explicit configuration.
    pub fn with_config(
        project_id: ProjectId,
        campaign_id: String,
        client: Arc<dyn AutoReClient>,
        config: CoordinatorConfig,
        handlers: H,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            project_id,
            campaign_id,
            client,
            config,
            state: CoordinatorState::default(),
            handlers,
            cancel,
            detector: NoProgressDetector::with_threshold(config.no_progress_threshold),
        }
    }

    /// Performs one coordinator tick.
    ///
    /// The tick aborts early and returns [`TickResult::Cancelled`] if the
    /// cancellation token is cancelled before dispatch begins.
    pub async fn tick(&mut self) -> Result<TickResult> {
        if self.cancel.is_cancelled() {
            return Ok(TickResult::Cancelled);
        }

        self.reconcile_interrupted_operations()?;
        self.refresh_provider_health();
        self.refresh_program_structure_if_requested()?;
        self.update_work_dependencies()?;
        self.invalidate_stale_work()?;
        self.promote_ready_work()?;

        if CompletionPolicy::is_complete(&self.state) {
            return Ok(TickResult::Complete);
        }

        let selected = self.select_ready_work();
        let (item, kind) = match selected {
            Some(s) => s,
            None => return Ok(TickResult::NoWork),
        };

        let output = dispatch(&self.handlers, kind, &item).await?;

        if let Some(hash) = output.raw_response_hash {
            let key = item.entity_key();
            self.state.record_hash(&key, hash);
            if self.detector.is_stuck(&self.state, &key) {
                self.client
                    .execute(ApplicationCommand::BlockWorkItem(BlockWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                        reason: format!(
                            "{}:{:?}",
                            NoProgressKind::RepeatedIdenticalModelOutput.as_reason(),
                            kind
                        ),
                    }))?;
                return Ok(TickResult::Blocked(item.work_item_id));
            }
        }

        for cmd in output.commands {
            self.client.execute(cmd)?;
        }

        self.state.tick_count += 1;
        Ok(TickResult::Processed(item.work_item_id))
    }

    /// §17 restart: requeue work items left in `Leased` or `Running` state.
    fn reconcile_interrupted_operations(&self) -> Result<()> {
        for item in &self.state.work_items {
            if matches!(item.state, WorkItemState::Leased | WorkItemState::Running) {
                self.client.execute(ApplicationCommand::RequeueWorkItem(
                    RequeueWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                    },
                ))?;
            }
        }
        Ok(())
    }

    /// Refresh provider health snapshots.
    fn refresh_provider_health(&mut self) {
        // Stub: health would be updated from provider-runtime status queries.
        // We touch the map so the phase is exercised.
        for health in self.state.provider_health.values_mut() {
            if *health == ProviderHealth::Unknown {
                *health = ProviderHealth::Healthy;
            }
        }
    }

    /// If requested, import the provider run that refreshes program structure.
    fn refresh_program_structure_if_requested(&self) -> Result<()> {
        if let Some(run_id) = self.state.program_structure_refresh_run_id {
            self.client
                .execute(ApplicationCommand::ImportProviderRunResult(
                    ImportProviderRunResultRequest {
                        project: self.project_id,
                        run_id,
                    },
                ))?;
        }
        Ok(())
    }

    /// Emit `RecordWorkDependency` commands for dependencies not yet recorded.
    fn update_work_dependencies(&self) -> Result<()> {
        for (successor, predecessor) in &self.state.dependencies {
            self.client
                .execute(ApplicationCommand::RecordWorkDependency(
                    RecordWorkDependencyRequest {
                        project: self.project_id,
                        work_item_id: successor.clone(),
                        depends_on: predecessor.clone(),
                    },
                ))?;
        }
        Ok(())
    }

    /// Issue `InvalidateWorkItem` for items flagged stale in state.
    fn invalidate_stale_work(&self) -> Result<()> {
        for item in &self.state.work_items {
            if item.state == WorkItemState::Stale {
                self.client.execute(ApplicationCommand::InvalidateWorkItem(
                    InvalidateWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                        reason: "stale input fingerprint".to_string(),
                    },
                ))?;
            }
        }
        Ok(())
    }

    /// Promote `Pending` items whose dependencies are terminal.
    fn promote_ready_work(&self) -> Result<()> {
        let mut promoted = 0;
        for item in &self.state.work_items {
            if item.state != WorkItemState::Pending {
                continue;
            }
            if policy::dependencies_satisfied(item, &self.state) {
                self.client.execute(ApplicationCommand::PromoteWorkItem(
                    PromoteWorkItemRequest {
                        project: self.project_id,
                        work_item_id: item.work_item_id.clone(),
                    },
                ))?;
                promoted += 1;
                if promoted >= self.config.max_promotions_per_tick {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Select the highest-priority dispatchable work item.
    fn select_ready_work(&self) -> Option<(CoordinatorWorkItem, DispatchKind)> {
        let mut ready: Vec<(u64, &CoordinatorWorkItem, DispatchKind)> = self
            .state
            .work_items
            .iter()
            .filter(|w| policy::is_dispatchable(w.state))
            .filter_map(|w| classify_work_item(w).map(|k| (priority(w), w, k)))
            .collect();
        ready.sort_by_key(|b| std::cmp::Reverse(b.0));
        ready.first().map(|(_, w, k)| ((*w).clone(), *k))
    }
}

fn priority(item: &CoordinatorWorkItem) -> u64 {
    use autore_schema::domain::records::WorkItemKind;
    let kind_score = match item.kind {
        WorkItemKind::ProgramSkeleton => 100,
        WorkItemKind::ExternalDependency => 95,
        WorkItemKind::Global => 90,
        WorkItemKind::Enum
        | WorkItemKind::Structure
        | WorkItemKind::Class
        | WorkItemKind::Vtable => 80,
        WorkItemKind::ConflictResolution => 75,
        WorkItemKind::BuildFailure | WorkItemKind::LinkFailure => 70,
        WorkItemKind::VerificationFailure => 65,
        WorkItemKind::Investigation => 60,
        WorkItemKind::FunctionCluster => 55,
        WorkItemKind::Function => 50,
        WorkItemKind::StaticInitializer => 40,
        WorkItemKind::Subsystem => 30,
        WorkItemKind::Entrypoint => 20,
        WorkItemKind::Generation => 10,
    };
    kind_score as u64
}

#[cfg(test)]
mod tests;
