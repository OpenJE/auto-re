//! Model router — selects the appropriate model descriptor for a task.
//!
//! The `ModelRouter` maps `TaskKind` variants to `ModelClass` and selects
//! the first matching `ModelDescriptor` from its registry.

use crate::domain::task::TaskKind;
use crate::model::{ModelClass, ModelDescriptor};

// ---------------------------------------------------------------------------
// Default concurrency limit
// ---------------------------------------------------------------------------

/// Default concurrency limit when a model descriptor does not specify one.
const DEFAULT_CONCURRENCY_LIMIT: u64 = 4;

// ---------------------------------------------------------------------------
// ModelRouter
// ---------------------------------------------------------------------------

/// Routes tasks to appropriate model descriptors based on task kind and
/// model class.
#[derive(Debug, Clone)]
pub struct ModelRouter {
    models: Vec<ModelDescriptor>,
}

impl ModelRouter {
    /// Creates a new router with the given model descriptors.
    pub fn new(models: Vec<ModelDescriptor>) -> Self {
        Self { models }
    }

    /// Selects a model for the given task based on its `TaskKind`.
    ///
    /// Mapping:
    /// - Analysis/decompilation/type-recovery → `ModelClass::Analyzer`
    /// - Verification/validation → `ModelClass::Verifier`
    /// - Report generation → `ModelClass::Summarizer`
    /// - Everything else → `ModelClass::Generalist`
    pub fn select(&self, task: &crate::domain::task::Task) -> crate::Result<&ModelDescriptor> {
        let class = class_for_kind(&task.kind);
        self.select_by_class(class)
    }

    /// Selects the first model matching the given class.
    pub fn select_by_class(&self, class: ModelClass) -> crate::Result<&ModelDescriptor> {
        self.models
            .iter()
            .find(|m| m.class == class)
            .ok_or_else(|| {
                crate::Error::ModelProvider(format!("no model available for class {:?}", class))
            })
    }

    /// Returns the concurrency limit for a model by ID.
    ///
    /// Currently returns `DEFAULT_CONCURRENCY_LIMIT` (4) for all known models,
    /// or 0 for unknown model IDs.
    pub fn concurrency_limit(&self, model_id: &str) -> u64 {
        if self.models.iter().any(|m| m.id == model_id) {
            DEFAULT_CONCURRENCY_LIMIT
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// TaskKind → ModelClass mapping
// ---------------------------------------------------------------------------

/// Maps a `TaskKind` to the `ModelClass` best suited for it.
fn class_for_kind(kind: &TaskKind) -> ModelClass {
    match kind {
        // Analysis and decompilation → Analyzer
        TaskKind::AnalyzeFunction
        | TaskKind::AnalyzeModule
        | TaskKind::AnalyzeCallGraph
        | TaskKind::AnalyzeCrossReferences
        | TaskKind::DecompileFunction
        | TaskKind::DecompileModule
        | TaskKind::RecoverTypes
        | TaskKind::RecoverStructures
        | TaskKind::RecoverCallingConventions => ModelClass::Analyzer,

        // Verification and validation → Verifier
        TaskKind::VerifyClaim
        | TaskKind::VerifyClaimSet
        | TaskKind::GenerateImplementationContract
        | TaskKind::ValidateImplementationContract
        | TaskKind::ValidateReimplementation => ModelClass::Verifier,

        // Reporting → Summarizer
        TaskKind::GenerateReport | TaskKind::GenerateDiffReport => ModelClass::Summarizer,

        // Everything else → Generalist
        _ => ModelClass::Generalist,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::task::{RequiredCapabilities, TaskPriority, TaskSubject};
    use crate::ids::{CampaignId, TaskId};
    use crate::model::provider::ModelCapabilities;

    fn make_models() -> Vec<ModelDescriptor> {
        vec![
            ModelDescriptor {
                id: "analyzer-1".into(),
                name: "Analyzer".into(),
                class: ModelClass::Analyzer,
                capabilities: ModelCapabilities {
                    json_mode: true,
                    tool_use: false,
                    analysis: true,
                    verification: false,
                },
                max_context_tokens: 8192,
            },
            ModelDescriptor {
                id: "verifier-1".into(),
                name: "Verifier".into(),
                class: ModelClass::Verifier,
                capabilities: ModelCapabilities {
                    json_mode: true,
                    tool_use: true,
                    analysis: false,
                    verification: true,
                },
                max_context_tokens: 4096,
            },
            ModelDescriptor {
                id: "summarizer-1".into(),
                name: "Summarizer".into(),
                class: ModelClass::Summarizer,
                capabilities: ModelCapabilities {
                    json_mode: true,
                    tool_use: false,
                    analysis: false,
                    verification: false,
                },
                max_context_tokens: 16384,
            },
        ]
    }

    fn make_task(kind: TaskKind) -> crate::domain::task::Task {
        crate::domain::task::Task::new(
            TaskId::new(),
            CampaignId::new(),
            kind,
            TaskSubject::Binary,
            TaskPriority::new(100),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        )
    }

    #[test]
    fn model_router_selects_by_class() {
        let router = ModelRouter::new(make_models());

        // Analysis task → Analyzer
        let task = make_task(TaskKind::AnalyzeFunction);
        let model = router.select(&task).unwrap();
        assert_eq!(model.class, ModelClass::Analyzer);
        assert_eq!(model.id, "analyzer-1");

        // Verification task → Verifier
        let task = make_task(TaskKind::VerifyClaim);
        let model = router.select(&task).unwrap();
        assert_eq!(model.class, ModelClass::Verifier);
        assert_eq!(model.id, "verifier-1");

        // Report task → Summarizer
        let task = make_task(TaskKind::GenerateReport);
        let model = router.select(&task).unwrap();
        assert_eq!(model.class, ModelClass::Summarizer);
        assert_eq!(model.id, "summarizer-1");

        // Direct class selection
        let model = router.select_by_class(ModelClass::Analyzer).unwrap();
        assert_eq!(model.id, "analyzer-1");
    }

    #[test]
    fn model_router_enforces_concurrency_limits() {
        let router = ModelRouter::new(make_models());

        // Known models get the default limit.
        assert_eq!(
            router.concurrency_limit("analyzer-1"),
            DEFAULT_CONCURRENCY_LIMIT
        );
        assert_eq!(
            router.concurrency_limit("verifier-1"),
            DEFAULT_CONCURRENCY_LIMIT
        );
        assert_eq!(
            router.concurrency_limit("summarizer-1"),
            DEFAULT_CONCURRENCY_LIMIT
        );

        // Unknown models get 0.
        assert_eq!(router.concurrency_limit("nonexistent"), 0);
    }

    #[test]
    fn model_router_errors_on_missing_class() {
        // Only an analyzer — no generalist available.
        let router = ModelRouter::new(vec![ModelDescriptor {
            id: "analyzer-only".into(),
            name: "Analyzer Only".into(),
            class: ModelClass::Analyzer,
            capabilities: ModelCapabilities {
                json_mode: true,
                tool_use: false,
                analysis: true,
                verification: false,
            },
            max_context_tokens: 8192,
        }]);

        // A campaign-management task maps to Generalist, which is missing.
        let task = make_task(TaskKind::EvaluateCampaign);
        let err = router.select(&task).unwrap_err();
        assert!(err.to_string().contains("Generalist"));
    }
}
