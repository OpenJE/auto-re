//! Handler traits for the seven conceptual work-kind dispatch targets.
//!
//! Each handler is an async boundary. The coordinator does not care whether the
//! implementation is a real provider, an orchestrator, or a mock; it only cares
//! that the handler returns a [`HandlerOutput`] containing the commands to be
//! atomically executed this tick.

use async_trait::async_trait;

use autore_app::ApplicationCommand;
use autore_core::Result;

use crate::coordinator::state::CoordinatorWorkItem;

/// Output returned by a work-kind handler.
///
/// All durable side effects are represented as [`ApplicationCommand`]s. The
/// coordinator executes them in order, preserving the atomic-import-per-iteration
/// invariant.
#[derive(Debug, Clone, Default)]
pub struct HandlerOutput {
    pub commands: Vec<ApplicationCommand>,
    /// Raw-response hash for no-progress detection. `None` when the handler
    /// produced no model output (e.g. a pure deterministic step).
    pub raw_response_hash: Option<u64>,
}

impl HandlerOutput {
    /// Creates output with a single command and no hash.
    pub fn command(cmd: ApplicationCommand) -> Self {
        Self {
            commands: vec![cmd],
            raw_response_hash: None,
        }
    }

    /// Creates output carrying a raw-response hash.
    pub fn with_hash(mut self, hash: u64) -> Self {
        self.raw_response_hash = Some(hash);
        self
    }
}

/// Conceptual work kinds used for dispatch inside the coordinator.
///
/// These do not map 1:1 to [`autore_schema::domain::records::WorkItemKind`];
/// the coordinator classifies work items via [`classify_work_item`] before
/// dispatching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchKind {
    StaticInvestigation,
    DynamicInvestigation,
    SemanticAnalysis,
    ConflictResolution,
    Generation,
    BuildFailure,
    Verification,
}

/// Classifies a work item into the conceptual dispatch kind.
///
/// The `Investigation` kind is disambiguated by description prefix because the
/// schema-level enum does not yet contain the fine-grained static/dynamic/
/// semantic variants.
pub fn classify_work_item(item: &CoordinatorWorkItem) -> Option<DispatchKind> {
    use autore_schema::domain::records::WorkItemKind;
    match item.kind {
        WorkItemKind::Investigation => {
            if item.description.starts_with("dynamic:") {
                Some(DispatchKind::DynamicInvestigation)
            } else if item.description.starts_with("semantic:") {
                Some(DispatchKind::SemanticAnalysis)
            } else {
                Some(DispatchKind::StaticInvestigation)
            }
        }
        WorkItemKind::ConflictResolution => Some(DispatchKind::ConflictResolution),
        WorkItemKind::Generation
        | WorkItemKind::ProgramSkeleton
        | WorkItemKind::ExternalDependency
        | WorkItemKind::Global
        | WorkItemKind::Enum
        | WorkItemKind::Structure
        | WorkItemKind::Class
        | WorkItemKind::Vtable
        | WorkItemKind::Function
        | WorkItemKind::FunctionCluster
        | WorkItemKind::StaticInitializer
        | WorkItemKind::Subsystem
        | WorkItemKind::Entrypoint => Some(DispatchKind::Generation),
        WorkItemKind::BuildFailure | WorkItemKind::LinkFailure => Some(DispatchKind::BuildFailure),
        WorkItemKind::VerificationFailure => Some(DispatchKind::Verification),
    }
}

/// Single trait exposing all work-kind handlers.
///
/// Production implementations delegate to the Wave-3 IDA provider, Wave-7
/// dynamic backend, Wave-5 LLM importer, Wave-8 reconciler, Wave-9 generation
/// orchestrator, Wave-9 repair path, and Wave-10 verification pipeline
/// respectively.
#[async_trait]
pub trait WorkKindHandlers: Send + Sync {
    /// Wave-3 IDA provider path.
    async fn handle_static_investigation(
        &self,
        item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput>;

    /// Wave-7 IDA-GDB scenario path.
    async fn handle_dynamic_investigation(
        &self,
        item: &CoordinatorWorkItem,
    ) -> Result<HandlerOutput>;

    /// Wave-5 LLM analysis.
    async fn handle_semantic_analysis(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput>;

    /// Wave-8 conflict-arbitration pipeline.
    async fn handle_conflict_resolution(&self, item: &CoordinatorWorkItem)
    -> Result<HandlerOutput>;

    /// Wave-9 generation orchestrator.
    async fn handle_generation(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput>;

    /// Wave-9 repair orchestration.
    async fn handle_build_failure(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput>;

    /// Wave-10 differential verification pipeline.
    async fn handle_verification(&self, item: &CoordinatorWorkItem) -> Result<HandlerOutput>;
}

/// Dispatches a work item to the appropriate handler method.
pub async fn dispatch<H: WorkKindHandlers>(
    handlers: &H,
    kind: DispatchKind,
    item: &CoordinatorWorkItem,
) -> Result<HandlerOutput> {
    match kind {
        DispatchKind::StaticInvestigation => handlers.handle_static_investigation(item).await,
        DispatchKind::DynamicInvestigation => handlers.handle_dynamic_investigation(item).await,
        DispatchKind::SemanticAnalysis => handlers.handle_semantic_analysis(item).await,
        DispatchKind::ConflictResolution => handlers.handle_conflict_resolution(item).await,
        DispatchKind::Generation => handlers.handle_generation(item).await,
        DispatchKind::BuildFailure => handlers.handle_build_failure(item).await,
        DispatchKind::Verification => handlers.handle_verification(item).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::records::WorkItemKind;
    use autore_schema::ids::WorkItemId;

    fn item(kind: WorkItemKind, description: &str) -> CoordinatorWorkItem {
        CoordinatorWorkItem {
            work_item_id: WorkItemId::new().to_string(),
            kind,
            description: description.to_string(),
            state: autore_schema::domain::records::WorkItemState::Ready,
            subject_entity: None,
            dependencies: Vec::new(),
            required: true,
        }
    }

    #[test]
    fn classify_investigation_by_description() {
        assert_eq!(
            classify_work_item(&item(WorkItemKind::Investigation, "static: foo")),
            Some(DispatchKind::StaticInvestigation)
        );
        assert_eq!(
            classify_work_item(&item(WorkItemKind::Investigation, "dynamic: foo")),
            Some(DispatchKind::DynamicInvestigation)
        );
        assert_eq!(
            classify_work_item(&item(WorkItemKind::Investigation, "semantic: foo")),
            Some(DispatchKind::SemanticAnalysis)
        );
    }

    #[test]
    fn classify_generation_and_failure_kinds() {
        assert_eq!(
            classify_work_item(&item(WorkItemKind::Function, "")),
            Some(DispatchKind::Generation)
        );
        assert_eq!(
            classify_work_item(&item(WorkItemKind::BuildFailure, "")),
            Some(DispatchKind::BuildFailure)
        );
        assert_eq!(
            classify_work_item(&item(WorkItemKind::VerificationFailure, "")),
            Some(DispatchKind::Verification)
        );
        assert_eq!(
            classify_work_item(&item(WorkItemKind::ConflictResolution, "")),
            Some(DispatchKind::ConflictResolution)
        );
    }
}
