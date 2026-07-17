//! Worker runner — dispatches analysis packets to model providers.
//!
//! `WorkerRunner` takes a `WorkerInput` (task metadata + analysis packet),
//! calls a model provider with a schema-constrained prompt, validates the
//! response, converts it to domain claims and evidence, stores them via
//! repositories, and marks the task complete or failed.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::analysis::FunctionAnalysisPacket;
use crate::domain::{Claim, Evidence};
use crate::ids::{CampaignId, TaskId, WorkerRunId};
use crate::model::{ModelDescriptor, ModelProvider, ModelRequest};
use crate::storage::repositories::{ClaimRepository, EvidenceRepository, TaskRepository};
use crate::worker::output::{FunctionAnalysisOutput, validate_output};

// ---------------------------------------------------------------------------
// WorkerInput
// ---------------------------------------------------------------------------

/// Input to a single worker run.
pub struct WorkerInput {
    /// The task this run fulfils.
    pub task_id: TaskId,
    /// The campaign the task belongs to.
    pub campaign_id: CampaignId,
    /// The analysis packet describing the function to analyze.
    pub packet: FunctionAnalysisPacket,
    /// The model to use for completion.
    pub model_descriptor: ModelDescriptor,
    /// Maximum wall-clock time for the entire run.
    pub time_budget: Duration,
}

// ---------------------------------------------------------------------------
// WorkerOutput
// ---------------------------------------------------------------------------

/// Output produced by a successful worker run.
#[derive(Debug)]
pub struct WorkerOutput {
    /// Claims generated from the model response.
    pub claims: Vec<Claim>,
    /// Evidence generated from the model response.
    pub evidence: Vec<Evidence>,
    /// The validated model output.
    pub analysis: FunctionAnalysisOutput,
}

// ---------------------------------------------------------------------------
// WorkerRunner
// ---------------------------------------------------------------------------

/// Dispatches analysis packets to model providers and persists results.
pub struct WorkerRunner {
    provider: Arc<dyn ModelProvider>,
    tasks: Arc<dyn TaskRepository>,
    claims: Arc<dyn ClaimRepository>,
    evidence: Arc<dyn EvidenceRepository>,
}

impl WorkerRunner {
    /// Creates a new runner with the given dependencies.
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        tasks: Arc<dyn TaskRepository>,
        claims: Arc<dyn ClaimRepository>,
        evidence: Arc<dyn EvidenceRepository>,
    ) -> Self {
        Self {
            provider,
            tasks,
            claims,
            evidence,
        }
    }

    /// Runs a single analysis task to completion.
    ///
    /// 1. Builds a prompt from the packet and a schema from `FunctionAnalysisOutput`.
    /// 2. Calls `ModelProvider::complete()` within the time budget.
    /// 3. Validates the response against the schema.
    /// 4. Converts to claims and evidence.
    /// 5. Stores claims and evidence via repositories.
    /// 6. Marks the task `Completed`.
    ///
    /// On timeout, cancellation, or validation failure, marks the task `Failed`.
    pub async fn run(
        &self,
        input: WorkerInput,
        cancel: CancellationToken,
    ) -> crate::Result<WorkerOutput> {
        let worker_run_id = WorkerRunId::new();
        let result = self.run_inner(&input, &cancel, worker_run_id).await;

        match &result {
            Ok(_) => {
                self.tasks.complete(input.task_id).await?;
            }
            Err(e) => {
                let msg = e.to_string();
                // Best-effort fail; if this also errors, propagate the original.
                let _ = self.tasks.fail(input.task_id, msg.clone()).await;
            }
        }

        result
    }

    async fn run_inner(
        &self,
        input: &WorkerInput,
        cancel: &CancellationToken,
        worker_run_id: WorkerRunId,
    ) -> crate::Result<WorkerOutput> {
        let prompt = build_prompt(&input.packet);
        let schema = schema_for_output();

        let request = ModelRequest {
            model_id: input.model_descriptor.id.clone(),
            prompt,
            schema: Some(schema),
        };

        // Enforce time budget with cooperative cancellation.
        let response = tokio::select! {
            result = timeout(input.time_budget, self.provider.complete(request, cancel.clone())) => {
                match result {
                    Ok(inner) => inner?,
                    Err(_) => {
                        return Err(crate::Error::Worker(format!(
                            "timed out after {:?}",
                            input.time_budget
                        )));
                    }
                }
            }
            _ = cancel.cancelled() => {
                return Err(crate::Error::Worker("cancelled".into()));
            }
        };

        // Validate the response against the schema.
        let analysis = validate_output(&response.content)?;

        // Convert to domain entities.
        let claims = Claim::from_worker_output(input.packet.function_id, &analysis, worker_run_id)?;
        let evidence =
            Evidence::from_worker_output(input.packet.function_id, &analysis, worker_run_id)?;

        // Store claims.
        for claim in &claims {
            self.claims.create(claim).await?;
        }

        // Store evidence.
        for ev in &evidence {
            self.evidence.create(ev).await?;
        }

        Ok(WorkerOutput {
            claims,
            evidence,
            analysis,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Builds a prompt string from an analysis packet.
fn build_prompt(packet: &FunctionAnalysisPacket) -> String {
    let mut prompt = format!(
        "Analyze function {} at address {}.",
        packet.function_id, packet.address
    );
    if let Some(ref name) = packet.symbol_name {
        prompt.push_str(&format!(" Symbol name: {name}."));
    }
    if !packet.callers.is_empty() {
        prompt.push_str(&format!(" Callers: {}.", packet.callers.len()));
    }
    if !packet.callees.is_empty() {
        prompt.push_str(&format!(" Callees: {}.", packet.callees.len()));
    }
    prompt
}

/// Generates the JSON Schema for `FunctionAnalysisOutput`.
fn schema_for_output() -> serde_json::Value {
    let schema = schemars::schema_for!(FunctionAnalysisOutput);
    serde_json::to_value(schema).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AnalysisCapability;
    use crate::domain::{
        Address, AddressSpace, Claim, Confidence, Evidence, EvidenceKind, EvidenceLocation,
        SymbolName, Task, TaskState,
    };
    use crate::ids::{
        BinaryRevisionId, CampaignId, ClaimId, EvidenceId, FunctionId, ModuleId, TaskId,
    };
    use crate::model::{
        ModelCapabilities, ModelClass, ModelDescriptor, ModelProvider, ModelRequest, ModelResponse,
    };
    use crate::storage::repositories::TaskRepository;
    use crate::worker::output::{FunctionAnalysisOutput, ProposedClaim, ProposedEvidence};
    use async_trait::async_trait;
    use std::sync::Mutex;

    // -- In-memory stubs --

    struct StubTaskRepository {
        state: Mutex<TaskState>,
    }

    impl StubTaskRepository {
        fn new() -> Self {
            Self {
                state: Mutex::new(TaskState::Running),
            }
        }

        fn current_state(&self) -> TaskState {
            *self.state.lock().unwrap()
        }
    }

    #[async_trait]
    impl TaskRepository for StubTaskRepository {
        async fn create(&self, _task: &Task) -> crate::Result<TaskId> {
            Ok(TaskId::new())
        }
        async fn lease_next(
            &self,
            _campaign_id: CampaignId,
            _now: time::OffsetDateTime,
        ) -> crate::Result<Option<Task>> {
            Ok(None)
        }
        async fn renew_lease(
            &self,
            _task_id: TaskId,
            _until: time::OffsetDateTime,
        ) -> crate::Result<()> {
            Ok(())
        }
        async fn complete(&self, _task_id: TaskId) -> crate::Result<()> {
            *self.state.lock().unwrap() = TaskState::Completed;
            Ok(())
        }
        async fn fail(&self, _task_id: TaskId, _error: String) -> crate::Result<()> {
            *self.state.lock().unwrap() = TaskState::Failed;
            Ok(())
        }
    }

    struct StubClaimRepository {
        claims: Mutex<Vec<Claim>>,
    }

    impl StubClaimRepository {
        fn new() -> Self {
            Self {
                claims: Mutex::new(Vec::new()),
            }
        }

        fn stored_claims(&self) -> Vec<Claim> {
            self.claims.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ClaimRepository for StubClaimRepository {
        async fn create(&self, claim: &Claim) -> crate::Result<ClaimId> {
            self.claims.lock().unwrap().push(claim.clone());
            Ok(claim.id)
        }
        async fn find_by_id(&self, _id: ClaimId) -> crate::Result<Option<Claim>> {
            Ok(None)
        }
    }

    struct StubEvidenceRepository {
        evidence: Mutex<Vec<Evidence>>,
    }

    impl StubEvidenceRepository {
        fn new() -> Self {
            Self {
                evidence: Mutex::new(Vec::new()),
            }
        }

        fn stored_evidence(&self) -> Vec<Evidence> {
            self.evidence.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl EvidenceRepository for StubEvidenceRepository {
        async fn create(&self, evidence: &Evidence) -> crate::Result<EvidenceId> {
            self.evidence.lock().unwrap().push(evidence.clone());
            Ok(evidence.id)
        }
        async fn find_by_id(&self, _id: EvidenceId) -> crate::Result<Option<Evidence>> {
            Ok(None)
        }
    }

    // -- Valid-output mock provider --

    /// A mock provider that returns schema-valid `FunctionAnalysisOutput` JSON.
    struct ValidOutputProvider;

    fn valid_analysis_json(function_id: FunctionId) -> String {
        let output = FunctionAnalysisOutput {
            function_id,
            symbol_name: Some(SymbolName::new("test_func")),
            address: Address::new(AddressSpace::Virtual, 0x1000),
            confidence: Confidence::new(0.9).unwrap(),
            claims: vec![ProposedClaim {
                predicate: crate::domain::ClaimPredicate::FunctionName,
                value: crate::domain::ClaimValue::String("test_func".into()),
                confidence: Confidence::new(0.95).unwrap(),
                dependencies: vec![],
            }],
            evidence: vec![ProposedEvidence {
                kind: EvidenceKind::Disassembly,
                location: Some(EvidenceLocation::new(
                    Some(Address::new(AddressSpace::Virtual, 0x1000)),
                    None,
                )),
                description: "push rbp; mov rbp, rsp".into(),
                confidence: Confidence::new(0.85).unwrap(),
            }],
            metadata: serde_json::json!({}),
        };
        serde_json::to_string(&output).unwrap()
    }

    #[async_trait]
    impl ModelProvider for ValidOutputProvider {
        async fn list_models(&self) -> crate::Result<Vec<ModelDescriptor>> {
            Ok(vec![])
        }
        async fn complete(
            &self,
            _request: ModelRequest,
            cancel: CancellationToken,
        ) -> crate::Result<ModelResponse> {
            if cancel.is_cancelled() {
                return Err(crate::Error::ModelProvider("cancelled".into()));
            }
            Ok(ModelResponse {
                content: valid_analysis_json(FunctionId::new()),
                tokens_used: 100,
            })
        }
    }

    /// Provider that returns malformed JSON (fails schema validation).
    struct MalformedOutputProvider;

    #[async_trait]
    impl ModelProvider for MalformedOutputProvider {
        async fn list_models(&self) -> crate::Result<Vec<ModelDescriptor>> {
            Ok(vec![])
        }
        async fn complete(
            &self,
            _request: ModelRequest,
            _cancel: CancellationToken,
        ) -> crate::Result<ModelResponse> {
            Ok(ModelResponse {
                content: r#"{"broken": true}"#.into(),
                tokens_used: 10,
            })
        }
    }

    /// Provider that sleeps longer than the time budget.
    struct SlowProvider;

    #[async_trait]
    impl ModelProvider for SlowProvider {
        async fn list_models(&self) -> crate::Result<Vec<ModelDescriptor>> {
            Ok(vec![])
        }
        async fn complete(
            &self,
            _request: ModelRequest,
            cancel: CancellationToken,
        ) -> crate::Result<ModelResponse> {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    Ok(ModelResponse {
                        content: "{}".into(),
                        tokens_used: 0,
                    })
                }
                _ = cancel.cancelled() => {
                    Err(crate::Error::ModelProvider("cancelled".into()))
                }
            }
        }
    }

    // -- Helpers --

    fn test_packet() -> FunctionAnalysisPacket {
        FunctionAnalysisPacket {
            function_id: FunctionId::new(),
            binary_revision_id: BinaryRevisionId::new(),
            module_id: ModuleId::new(),
            address: Address::new(AddressSpace::Virtual, 0x1000),
            symbol_name: Some(SymbolName::new("test_func")),
            control_flow_hash: None,
            callers: vec![],
            callees: vec![],
            requested_capabilities: vec![AnalysisCapability::Decompile],
        }
    }

    fn test_model_descriptor() -> ModelDescriptor {
        ModelDescriptor {
            id: "test-model".into(),
            name: "Test Model".into(),
            class: ModelClass::Analyzer,
            capabilities: ModelCapabilities {
                json_mode: true,
                tool_use: false,
                analysis: true,
                verification: false,
            },
            max_context_tokens: 8192,
        }
    }

    fn test_input(packet: FunctionAnalysisPacket) -> WorkerInput {
        WorkerInput {
            task_id: TaskId::new(),
            campaign_id: CampaignId::new(),
            packet,
            model_descriptor: test_model_descriptor(),
            time_budget: Duration::from_secs(30),
        }
    }

    // -- Tests --

    #[tokio::test]
    async fn worker_runs_valid_output_to_claims() {
        let packet = test_packet();
        let input = test_input(packet);
        let task_repo = Arc::new(StubTaskRepository::new());
        let claim_repo = Arc::new(StubClaimRepository::new());
        let evidence_repo = Arc::new(StubEvidenceRepository::new());

        let runner = WorkerRunner::new(
            Arc::new(ValidOutputProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
            Arc::clone(&evidence_repo) as Arc<dyn EvidenceRepository>,
        );

        let cancel = CancellationToken::new();
        let result = runner.run(input, cancel).await;

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        let output = result.unwrap();
        assert_eq!(output.claims.len(), 1);
        assert_eq!(output.evidence.len(), 1);
        assert_eq!(task_repo.current_state(), TaskState::Completed);
        assert_eq!(claim_repo.stored_claims().len(), 1);
        assert_eq!(evidence_repo.stored_evidence().len(), 1);
    }

    #[tokio::test]
    async fn worker_rejects_malformed_schema() {
        let packet = test_packet();
        let input = test_input(packet);
        let task_repo = Arc::new(StubTaskRepository::new());
        let claim_repo = Arc::new(StubClaimRepository::new());
        let evidence_repo = Arc::new(StubEvidenceRepository::new());

        let runner = WorkerRunner::new(
            Arc::new(MalformedOutputProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
            Arc::clone(&evidence_repo) as Arc<dyn EvidenceRepository>,
        );

        let cancel = CancellationToken::new();
        let result = runner.run(input, cancel).await;

        assert!(result.is_err(), "expected validation error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("validation error"),
            "expected validation error, got: {err}"
        );
        assert_eq!(task_repo.current_state(), TaskState::Failed);
        assert!(claim_repo.stored_claims().is_empty());
    }

    #[tokio::test]
    async fn worker_cancels_on_token() {
        let packet = test_packet();
        let input = test_input(packet);
        let task_repo = Arc::new(StubTaskRepository::new());
        let claim_repo = Arc::new(StubClaimRepository::new());
        let evidence_repo = Arc::new(StubEvidenceRepository::new());

        let runner = WorkerRunner::new(
            Arc::new(SlowProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
            Arc::clone(&evidence_repo) as Arc<dyn EvidenceRepository>,
        );

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let handle = tokio::spawn(async move { runner.run(input, cancel_clone).await });

        // Cancel after a short delay.
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();

        let result = handle.await.unwrap();
        assert!(result.is_err(), "expected cancellation error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("cancelled") || err.to_string().contains("worker error"),
            "expected cancellation error, got: {err}"
        );
        assert_eq!(task_repo.current_state(), TaskState::Failed);
    }

    #[tokio::test]
    async fn worker_times_out() {
        let packet = test_packet();
        let mut input = test_input(packet);
        input.time_budget = Duration::from_millis(50);

        let task_repo = Arc::new(StubTaskRepository::new());
        let claim_repo = Arc::new(StubClaimRepository::new());
        let evidence_repo = Arc::new(StubEvidenceRepository::new());

        let runner = WorkerRunner::new(
            Arc::new(SlowProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&claim_repo) as Arc<dyn ClaimRepository>,
            Arc::clone(&evidence_repo) as Arc<dyn EvidenceRepository>,
        );

        let cancel = CancellationToken::new();
        let result = runner.run(input, cancel).await;

        assert!(result.is_err(), "expected timeout error");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("timed out"),
            "expected timeout error, got: {err}"
        );
        assert_eq!(task_repo.current_state(), TaskState::Failed);
    }
}
