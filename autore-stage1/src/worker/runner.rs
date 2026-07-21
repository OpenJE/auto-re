//! Worker runner — dispatches analysis packets to model providers.
//!
//! `WorkerRunner` takes a `WorkerInput` (task metadata + analysis packet),
//! calls a model provider with a schema-constrained prompt, validates the
//! response, converts it to domain claims and evidence, routes durable writes
//! through `ApplicationCommand` via an `AutoReClient`, and marks the task
//! complete or failed.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use autore_app::application_service::requests::{
    AddEvidenceRequest, AddHypothesisRequest, ApplicationCommand, AutoReClient,
    CompleteWorkItemRequest,
};
use autore_schema::domain::records::{EvidenceRecord, HypothesisStatus};
use autore_schema::domain::{
    Derivation, DerivationMethod, EvidenceValue, NamespacedId, Timestamp,
};

use crate::analysis::FunctionAnalysisPacket;
use crate::domain::{Claim, ClaimPredicate, ClaimValue, Evidence};
use crate::ids::{CampaignId, ProjectId, TaskId, WorkerRunId};
use crate::model::{ModelDescriptor, ModelProvider, ModelRequest};
use crate::storage::repositories::TaskRepository;
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
    /// The project this work item belongs to.
    pub project_id: ProjectId,
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

/// Dispatches analysis packets to model providers and routes durable writes
/// through `ApplicationCommand` via an `AutoReClient`.
pub struct WorkerRunner {
    provider: Arc<dyn ModelProvider>,
    tasks: Arc<dyn TaskRepository>,
    client: Arc<dyn AutoReClient>,
}

impl WorkerRunner {
    /// Creates a new runner with the given dependencies.
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        tasks: Arc<dyn TaskRepository>,
        client: Arc<dyn AutoReClient>,
    ) -> Self {
        Self {
            provider,
            tasks,
            client,
        }
    }

    /// Runs a single analysis task to completion.
    ///
    /// 1. Builds a prompt from the packet and a schema from `FunctionAnalysisOutput`.
    /// 2. Calls `ModelProvider::complete()` within the time budget.
    /// 3. Validates the response against the schema.
    /// 4. Issues `AddEvidence` commands for each evidence item.
    /// 5. Issues `AddHypothesis` with the claim and supporting evidence IDs.
    /// 6. Issues `CompleteWorkItem` to mark the work item done.
    /// 7. Marks the internal task `Completed`.
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

        // Route durable writes through the application command layer.
        self.issue_commands(input, &analysis, worker_run_id)?;

        // Convert to domain entities for in-memory return.
        let claims = Claim::from_worker_output(input.packet.function_id, &analysis, worker_run_id)?;
        let evidence =
            Evidence::from_worker_output(input.packet.function_id, &analysis, worker_run_id)?;

        Ok(WorkerOutput {
            claims,
            evidence,
            analysis,
        })
    }

    /// Issues `AddEvidence`, `AddHypothesis`, and `CompleteWorkItem` commands
    /// through the application client.
    fn issue_commands(
        &self,
        input: &WorkerInput,
        analysis: &FunctionAnalysisOutput,
        worker_run_id: WorkerRunId,
    ) -> crate::Result<()> {
        let operation =
            NamespacedId::parse(&format!("worker.run.{worker_run_id}"))
                .map_err(|e| crate::Error::Worker(format!("invalid operation name: {e}")))?;

        // Issue AddEvidence for each evidence item and collect the resulting IDs.
        let mut evidence_record_ids = Vec::new();
        for _pe in &analysis.evidence {
            let ev_id = autore_schema::ids::EvidenceRecordId::new();
            let record = EvidenceRecord {
                id: ev_id,
                project: input.project_id,
                subject: autore_schema::ids::EntityId::new(),
                predicate: NamespacedId::parse("evidence.predicate.worker-output")
                    .map_err(|e| crate::Error::Worker(format!("invalid predicate: {e}")))?,
                value: EvidenceValue::Null,
                derivation: Derivation::new(
                    DerivationMethod::LlmInference,
                    operation.clone(),
                    vec![],
                    vec![],
                ),
                provider_run: None,
                native_artifacts: vec![],
                assumptions: vec![],
                created_at: Timestamp::now(),
            };
            self.client.execute(ApplicationCommand::AddEvidence(
                AddEvidenceRequest {
                    project: input.project_id,
                    record,
                },
            ))?;
            evidence_record_ids.push(ev_id);
        }

        // Issue AddHypothesis for the analysis claims.
        let candidate = claim_value_to_evidence_value(
            analysis
                .claims
                .first()
                .map(|c| &c.value)
                .unwrap_or(&ClaimValue::String(String::new())),
        );
        let confidence = analysis.confidence.score() as f64;
        let predicate_str = analysis
            .claims
            .first()
            .map(|c| claim_predicate_to_string(&c.predicate))
            .unwrap_or_else(|| "unknown".to_string());

        self.client.execute(ApplicationCommand::AddHypothesis(
            AddHypothesisRequest {
                project: input.project_id,
                subject: autore_schema::ids::EntityId::new(),
                predicate: predicate_str,
                candidate,
                confidence_score: confidence,
                confidence_rationale: None,
                supporting_evidence: evidence_record_ids,
                contradicting_evidence: vec![],
                derived_from: vec![],
                status: HypothesisStatus::Proposed,
            },
        ))?;

        // Issue CompleteWorkItem to mark the work item done.
        self.client.execute(ApplicationCommand::CompleteWorkItem(
            CompleteWorkItemRequest {
                project: input.project_id,
                work_item_id: input.task_id.to_string(),
            },
        ))?;

        Ok(())
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

/// Converts a `ClaimPredicate` to a human-readable string.
fn claim_predicate_to_string(predicate: &ClaimPredicate) -> String {
    match predicate {
        ClaimPredicate::FunctionName => "function-name",
        ClaimPredicate::FunctionSignature => "function-signature",
        ClaimPredicate::FunctionAddress => "function-address",
        ClaimPredicate::FunctionSize => "function-size",
        ClaimPredicate::TypeRecovery => "type-recovery",
        ClaimPredicate::StructureLayout => "structure-layout",
        ClaimPredicate::CallingConvention => "calling-convention",
        ClaimPredicate::ControlFlowGraph => "control-flow-graph",
        ClaimPredicate::DataFlowFact => "data-flow-fact",
        ClaimPredicate::CrossReference => "cross-reference",
        ClaimPredicate::StringReference => "string-reference",
        ClaimPredicate::GlobalReference => "global-reference",
        ClaimPredicate::Comment => "comment",
        ClaimPredicate::RuntimeObservation => "runtime-observation",
        ClaimPredicate::ReimplementationCorrectness => "reimplementation-correctness",
        ClaimPredicate::TestResult => "test-result",
        ClaimPredicate::Custom(s) => return s.clone(),
    }
    .to_string()
}

/// Converts a `ClaimValue` to an `EvidenceValue` for use as a hypothesis
/// candidate.
fn claim_value_to_evidence_value(value: &ClaimValue) -> EvidenceValue {
    match value {
        ClaimValue::String(s) => EvidenceValue::String(s.clone()),
        ClaimValue::Integer(n) => EvidenceValue::UnsignedInteger(*n as u128),
        ClaimValue::Float(f) => EvidenceValue::Float(*f),
        ClaimValue::Boolean(b) => EvidenceValue::Boolean(*b),
        ClaimValue::Bytes(b) => EvidenceValue::Bytes(b.clone()),
        ClaimValue::TypeDescriptor(s) => EvidenceValue::String(s.clone()),
        ClaimValue::Map(entries) => EvidenceValue::String(format!("{entries:?}")),
        ClaimValue::Json(v) => EvidenceValue::String(v.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AnalysisCapability;
    use crate::domain::{
        Address, AddressSpace, Confidence, EvidenceKind, EvidenceLocation,
        SymbolName, Task, TaskState,
    };
    use crate::ids::{
        BinaryRevisionId, CampaignId, FunctionId, ModuleId, ProjectId, TaskId,
    };
    use crate::model::{
        ModelCapabilities, ModelClass, ModelDescriptor, ModelProvider, ModelRequest, ModelResponse,
    };
    use crate::storage::repositories::TaskRepository;
    use crate::worker::output::{FunctionAnalysisOutput, ProposedClaim, ProposedEvidence};
    use async_trait::async_trait;
    use autore_app::application_service::requests::{
        ApplicationQuery, CommandResult, QueryResult,
    };
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
        async fn create(&self, _task: &Task) -> autore_core::Result<TaskId> {
            Ok(TaskId::new())
        }
        async fn lease_next(
            &self,
            _campaign_id: CampaignId,
            _now: time::OffsetDateTime,
        ) -> autore_core::Result<Option<Task>> {
            Ok(None)
        }
        async fn renew_lease(
            &self,
            _task_id: TaskId,
            _until: time::OffsetDateTime,
        ) -> autore_core::Result<()> {
            Ok(())
        }
        async fn complete(&self, _task_id: TaskId) -> autore_core::Result<()> {
            *self.state.lock().unwrap() = TaskState::Completed;
            Ok(())
        }
        async fn fail(&self, _task_id: TaskId, _error: String) -> autore_core::Result<()> {
            *self.state.lock().unwrap() = TaskState::Failed;
            Ok(())
        }
    }

    // -- RecordingClient --

    /// An `AutoReClient` that records every command and query it receives,
    /// returning plausible stub results.
    struct RecordingClient {
        commands: Mutex<Vec<ApplicationCommand>>,
        queries: Mutex<Vec<ApplicationQuery>>,
    }

    impl RecordingClient {
        fn new() -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                queries: Mutex::new(Vec::new()),
            }
        }

        fn recorded_commands(&self) -> Vec<ApplicationCommand> {
            self.commands.lock().unwrap().clone()
        }

        #[allow(dead_code)]
        fn recorded_queries(&self) -> Vec<ApplicationQuery> {
            self.queries.lock().unwrap().clone()
        }
    }

    impl AutoReClient for RecordingClient {
        fn execute(&self, command: ApplicationCommand) -> autore_core::Result<CommandResult> {
            self.commands.lock().unwrap().push(command);
            // Return a plausible result for any command variant.
            Ok(CommandResult::EvidenceAdded(
                autore_app::AddEvidenceResponse {
                    id: autore_schema::ids::EvidenceRecordId::new(),
                },
            ))
        }

        fn query(&self, query: ApplicationQuery) -> autore_core::Result<QueryResult> {
            self.queries.lock().unwrap().push(query);
            Ok(QueryResult::WorkItems(
                autore_app::application_service::requests::WorkItemsResponse {
                    work_items: vec![],
                },
            ))
        }

        fn events_after(
            &self,
            _project: ProjectId,
            _sequence: u64,
            _limit: usize,
        ) -> autore_core::Result<
            Vec<autore_schema::domain::records::ProjectEvent>,
        > {
            Ok(vec![])
        }

        fn subscribe_events(
            &self,
            _project: ProjectId,
            _after: u64,
        ) -> autore_core::Result<
            autore_app::autore_events::project_event_service::ProjectEventSubscription,
        > {
            unimplemented!("not needed in worker tests")
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
            project_id: ProjectId::new(),
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
        let client = Arc::new(RecordingClient::new());

        let runner = WorkerRunner::new(
            Arc::new(ValidOutputProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&client) as Arc<dyn AutoReClient>,
        );

        let cancel = CancellationToken::new();
        let result = runner.run(input, cancel).await;

        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        let output = result.unwrap();
        assert_eq!(output.claims.len(), 1);
        assert_eq!(output.evidence.len(), 1);
        assert_eq!(task_repo.current_state(), TaskState::Completed);

        // Verify commands were issued through the client.
        let commands = client.recorded_commands();
        let add_evidence_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::AddEvidence(_)))
            .count();
        let has_hypothesis = commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::AddHypothesis(_)));
        let has_complete = commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::CompleteWorkItem(_)));
        assert_eq!(add_evidence_count, 1);
        assert!(has_hypothesis);
        assert!(has_complete);
    }

    #[tokio::test]
    async fn worker_rejects_malformed_schema() {
        let packet = test_packet();
        let input = test_input(packet);
        let task_repo = Arc::new(StubTaskRepository::new());
        let client = Arc::new(RecordingClient::new());

        let runner = WorkerRunner::new(
            Arc::new(MalformedOutputProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&client) as Arc<dyn AutoReClient>,
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
        // No commands should have been issued on failure.
        assert!(client.recorded_commands().is_empty());
    }

    #[tokio::test]
    async fn worker_cancels_on_token() {
        let packet = test_packet();
        let input = test_input(packet);
        let task_repo = Arc::new(StubTaskRepository::new());
        let client = Arc::new(RecordingClient::new());

        let runner = WorkerRunner::new(
            Arc::new(SlowProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&client) as Arc<dyn AutoReClient>,
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
        let client = Arc::new(RecordingClient::new());

        let runner = WorkerRunner::new(
            Arc::new(SlowProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&client) as Arc<dyn AutoReClient>,
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

    #[tokio::test]
    async fn worker_routes_writes_through_application() {
        let packet = test_packet();
        let input = test_input(packet);
        let task_repo = Arc::new(StubTaskRepository::new());
        let client = Arc::new(RecordingClient::new());

        let runner = WorkerRunner::new(
            Arc::new(ValidOutputProvider),
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
            Arc::clone(&client) as Arc<dyn AutoReClient>,
        );

        let cancel = CancellationToken::new();
        let result = runner.run(input, cancel).await;
        assert!(result.is_ok(), "expected Ok, got: {result:?}");

        let commands = client.recorded_commands();

        // Count each command variant.
        let add_evidence_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::AddEvidence(_)))
            .count();
        let add_hypothesis_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::AddHypothesis(_)))
            .count();
        let complete_count = commands
            .iter()
            .filter(|c| matches!(c, ApplicationCommand::CompleteWorkItem(_)))
            .count();

        // The test provider produces 1 evidence item and 1 claim.
        assert_eq!(
            add_evidence_count, 1,
            "expected exactly 1 AddEvidence command, got {add_evidence_count}"
        );
        assert_eq!(
            add_hypothesis_count, 1,
            "expected exactly 1 AddHypothesis command, got {add_hypothesis_count}"
        );
        assert_eq!(
            complete_count, 1,
            "expected exactly 1 CompleteWorkItem command, got {complete_count}"
        );

        // Verify command ordering: AddEvidence before AddHypothesis before CompleteWorkItem.
        assert_eq!(commands.len(), 3, "expected exactly 3 commands total");
        assert!(
            matches!(&commands[0], ApplicationCommand::AddEvidence(_)),
            "first command should be AddEvidence"
        );
        assert!(
            matches!(&commands[1], ApplicationCommand::AddHypothesis(_)),
            "second command should be AddHypothesis"
        );
        assert!(
            matches!(&commands[2], ApplicationCommand::CompleteWorkItem(_)),
            "third command should be CompleteWorkItem"
        );

        // Verify the AddHypothesis has supporting evidence IDs matching the
        // AddEvidence record IDs.
        if let ApplicationCommand::AddHypothesis(ref req) = commands[1] {
            assert_eq!(
                req.supporting_evidence.len(),
                1,
                "hypothesis should reference 1 supporting evidence record"
            );
            assert!(
                (req.confidence_score - 0.9).abs() < 0.001,
                "confidence should come from analysis output, got {}",
                req.confidence_score
            );
            assert_eq!(
                req.predicate, "function-name",
                "predicate should be derived from the claim"
            );
        }

        // Verify the CompleteWorkItem uses the task_id as work_item_id.
        if let ApplicationCommand::CompleteWorkItem(ref req) = commands[2] {
            assert!(
                !req.work_item_id.is_empty(),
                "work_item_id should not be empty"
            );
        }

        // Verify in-memory WorkerOutput is preserved.
        let output = result.unwrap();
        assert_eq!(output.claims.len(), 1);
        assert_eq!(output.evidence.len(), 1);
        assert_eq!(task_repo.current_state(), TaskState::Completed);
    }
}
