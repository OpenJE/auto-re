//! Scenario executor and observation backend trait.

use std::sync::Arc;

use async_trait::async_trait;

use autore_app::application_service::requests::{
    ApplicationCommand, ImportDynamicObservationRequest, RecordVerificationComparisonRequest,
};
use autore_app::{AutoReClient, CommandResult};
use autore_core::{Error, Result};
use autore_schema::domain::Timestamp;
use autore_schema::ids::{ArtifactId, ProjectId, VerificationComparisonId};

use crate::dynamic::WineGdbRunner;
use crate::dynamic::import::{DynamicObservation, TimestampRange};
use crate::dynamic::scenario::{SetupOp, StopOp};

use super::compare;
use super::types::{Observation, ObservationSet, Scenario, VerificationComparison};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur while capturing observations.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum ObservationError {
    /// The backend failed to execute the scenario.
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    /// The backend is not supported in this build.
    #[error("unsupported backend")]
    Unsupported,
    /// The observation kind is not valid.
    #[error("invalid observation kind: {0}")]
    InvalidObservationKind(String),
}

impl ObservationError {
    /// Produces an observation set that encodes the execution failure.
    pub fn to_observation_set(&self, scenario_id: &str, artifact_id: ArtifactId) -> ObservationSet {
        let mut set = ObservationSet::new(scenario_id, artifact_id);
        set.execution_failed = true;
        set.execution_failure_diagnostic = Some(super::types::ExecutionDiagnostic::new(
            "execution-failed",
            self.to_string(),
        ));
        set
    }
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Abstraction over the Wave 7 dynamic observation pipeline.
///
/// Production implementations invoke the IDA debugger + GDB backend. Mock
/// implementations are acceptable for tests.
#[async_trait]
pub trait ObservationBackend: Send + Sync {
    /// Executes `scenario` against `target_artifact_id` and returns the captured
    /// observation set.
    async fn capture(
        &self,
        scenario: &Scenario,
        target_artifact_id: ArtifactId,
    ) -> std::result::Result<ObservationSet, ObservationError>;
}

// ---------------------------------------------------------------------------
// Wave 7 observation backend
// ---------------------------------------------------------------------------

/// First implementation of [`ObservationBackend`] that drives the Wave 7
/// `WineGdbRunner` with a typed scenario.
pub struct Wave7ObservationBackend {
    runner_factory: Box<dyn Fn() -> WineGdbRunner + Send + Sync>,
}

impl std::fmt::Debug for Wave7ObservationBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wave7ObservationBackend")
            .field("runner_factory", &"<factory>")
            .finish()
    }
}

impl Wave7ObservationBackend {
    /// Creates a backend using the provided runner factory.
    pub fn new(runner_factory: Box<dyn Fn() -> WineGdbRunner + Send + Sync>) -> Self {
        Self { runner_factory }
    }

    /// Creates a backend that uses the mock `WineGdbRunner`.
    pub fn mock() -> Self {
        Self::new(Box::new(WineGdbRunner::mock))
    }
}

#[async_trait]
impl ObservationBackend for Wave7ObservationBackend {
    async fn capture(
        &self,
        scenario: &Scenario,
        target_artifact_id: ArtifactId,
    ) -> std::result::Result<ObservationSet, ObservationError> {
        let runner = (self.runner_factory)();
        let dynamic_scenario = crate::dynamic::scenario::Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: target_artifact_id,
                env: scenario.initial_state.env.clone(),
                working_dir: scenario.initial_state.working_dir.clone(),
            }],
            scenario.execution_steps.clone(),
            vec![StopOp::TerminateTarget],
        );

        let result =
            crate::dynamic::ida_provider::execute_scenario(&runner, &dynamic_scenario).await;
        match result {
            Ok(scenario_result) => Ok(convert_capture_context(
                scenario.id.clone(),
                target_artifact_id,
                scenario_result.ctx,
            )),
            Err(err) => Err(ObservationError::ExecutionFailed(err.to_string())),
        }
    }
}

fn convert_capture_context(
    scenario_id: String,
    target_artifact_id: ArtifactId,
    ctx: crate::dynamic::runner::CaptureContext,
) -> ObservationSet {
    let mut set = ObservationSet::new(scenario_id, target_artifact_id);
    for debug in ctx.observations {
        let namespaced_kind = debug_kind(&debug.kind);
        let observation = Observation {
            kind: namespaced_kind,
            key: None,
            entity_id: debug.entity,
            address: debug.address,
            timestamp: Some(debug.timestamp_ms),
            data: debug.data,
        };
        set.observations.push(observation);
    }
    set
}

fn debug_kind(raw: &str) -> NamespacedId {
    if raw.contains('.') {
        NamespacedId::parse(raw).unwrap_or_else(|_| NamespacedId::parse("debug.unknown").unwrap())
    } else {
        NamespacedId::parse(&format!("debug.{raw}")).unwrap_or_else(|_| {
            let sanitized = raw.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-");
            NamespacedId::parse(&format!("debug.{sanitized}")).unwrap()
        })
    }
}

// ---------------------------------------------------------------------------
// Scenario executor
// ---------------------------------------------------------------------------

/// Drives original and candidate scenario executions through a backend and
/// records the results through [`ApplicationCommand`].
pub struct ScenarioExecutor {
    project: ProjectId,
    client: Arc<dyn AutoReClient>,
    backend: Arc<dyn ObservationBackend>,
}

impl ScenarioExecutor {
    /// Creates a new executor.
    pub fn new(
        project: ProjectId,
        client: Arc<dyn AutoReClient>,
        backend: Arc<dyn ObservationBackend>,
    ) -> Self {
        Self {
            project,
            client,
            backend,
        }
    }

    /// Executes the scenario against the original executable artifact.
    pub async fn execute_original(&self, scenario: &Scenario) -> Result<ObservationSet> {
        let observation_set = self
            .backend
            .capture(scenario, scenario.executable_artifact_id)
            .await
            .map_err(|e| Error::Operation(e.to_string()))?;
        self.record_observations(scenario, &observation_set)?;
        Ok(observation_set)
    }

    /// Executes the scenario against the generated candidate artifact.
    pub async fn execute_candidate(
        &self,
        scenario: &Scenario,
        generated_binary_artifact: ArtifactId,
    ) -> Result<ObservationSet> {
        let observation_set = self
            .backend
            .capture(scenario, generated_binary_artifact)
            .await
            .map_err(|e| Error::Operation(e.to_string()))?;
        self.record_observations(scenario, &observation_set)?;
        Ok(observation_set)
    }

    /// Compares two observation sets and persists the result.
    pub async fn compare_and_record(
        &self,
        scenario: &Scenario,
        original: &ObservationSet,
        candidate: &ObservationSet,
    ) -> Result<VerificationComparison> {
        let mut comparison = compare(original, candidate, &scenario.normalization_rules);
        let request = RecordVerificationComparisonRequest {
            project: self.project,
            work_item_id: scenario.work_item_id.clone(),
        };
        let result = self
            .client
            .execute(ApplicationCommand::RecordVerificationComparison(request))?;
        if let CommandResult::VerificationComparisonRecorded(response) = result
            && let Ok(uuid) = uuid::Uuid::parse_str(&response.comparison_id)
        {
            comparison.id = VerificationComparisonId::from_uuid(uuid);
        }
        Ok(comparison)
    }

    fn record_observations(
        &self,
        scenario: &Scenario,
        observation_set: &ObservationSet,
    ) -> Result<()> {
        let recorded_at = Timestamp::now();
        let timestamp_range = TimestampRange {
            start: recorded_at,
            end: recorded_at,
        };

        for observation in &observation_set.observations {
            let dynamic = DynamicObservation {
                observation_kind: observation.kind.clone(),
                captured_artifact_id: observation_set.target_artifact_id,
                target_entity_id: scenario.subject_entity,
                scenario_id: scenario.id.clone(),
                timestamp_range,
                recorded_at,
            };
            let payload =
                serde_json::to_string(&dynamic).map_err(|e| Error::Serialization(e.to_string()))?;
            let request = ImportDynamicObservationRequest {
                project: self.project,
                observation: payload,
            };
            self.client
                .execute(ApplicationCommand::ImportDynamicObservation(request))?;
        }

        if let Some(_code) = observation_set.exit_code {
            let dynamic = DynamicObservation {
                observation_kind: NamespacedId::parse("debug.exit")?,
                captured_artifact_id: observation_set.target_artifact_id,
                target_entity_id: scenario.subject_entity,
                scenario_id: scenario.id.clone(),
                timestamp_range,
                recorded_at,
            };
            let payload =
                serde_json::to_string(&dynamic).map_err(|e| Error::Serialization(e.to_string()))?;
            let request = ImportDynamicObservationRequest {
                project: self.project,
                observation: payload,
            };
            self.client
                .execute(ApplicationCommand::ImportDynamicObservation(request))?;
        }

        Ok(())
    }
}

// Re-export types for internal use.
use autore_schema::domain::NamespacedId;

#[cfg(test)]
mod tests {
    use super::super::types::{
        ComparisonLevel, InitialState, Observation, ObservationSet, Scenario,
    };
    use super::super::{
        ObservationBackend, ObservationError, ScenarioExecutor, Wave7ObservationBackend,
    };
    use autore_app::application_service::requests::{
        ApplicationCommand, CommandResult, QueryResult, RecordVerificationComparisonResponse,
    };
    use autore_app::{ApplicationQuery, AutoReClient};
    use autore_core::Result;
    use autore_events::project_event_service::ProjectEventSubscription;
    use autore_schema::domain::NamespacedId;
    use autore_schema::ids::{
        ArtifactId, EntityId, ProjectId, VerificationComparisonId, WorkItemId,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use crate::tests_support::RecordingAutoReClient;

    fn debug_kind(name: &str) -> NamespacedId {
        NamespacedId::parse(&format!("debug.{name}")).unwrap()
    }

    fn base_observation_set() -> ObservationSet {
        ObservationSet::new("scenario-1", ArtifactId::new())
    }

    struct TestClient {
        inner: RecordingAutoReClient,
        commands: Mutex<Vec<ApplicationCommand>>,
    }

    impl TestClient {
        fn new() -> Self {
            Self {
                inner: RecordingAutoReClient::new(),
                commands: Mutex::new(Vec::new()),
            }
        }

        fn commands(&self) -> Vec<ApplicationCommand> {
            self.commands.lock().unwrap().clone()
        }
    }

    impl AutoReClient for TestClient {
        fn execute(&self, command: ApplicationCommand) -> Result<CommandResult> {
            self.commands.lock().unwrap().push(command.clone());
            match &command {
                ApplicationCommand::RecordVerificationComparison(_) => {
                    Ok(CommandResult::VerificationComparisonRecorded(
                        RecordVerificationComparisonResponse {
                            comparison_id: VerificationComparisonId::new().to_string(),
                        },
                    ))
                }
                _ => self.inner.execute(command),
            }
        }

        fn query(&self, query: ApplicationQuery) -> Result<QueryResult> {
            self.inner.query(query)
        }

        fn events_after(
            &self,
            project: ProjectId,
            sequence: u64,
            limit: usize,
        ) -> Result<Vec<autore_schema::domain::records::ProjectEvent>> {
            self.inner.events_after(project, sequence, limit)
        }

        fn subscribe_events(
            &self,
            project: ProjectId,
            after: u64,
        ) -> Result<ProjectEventSubscription> {
            self.inner.subscribe_events(project, after)
        }
    }

    struct MockBackend {
        original: ObservationSet,
        candidate: ObservationSet,
    }

    #[async_trait::async_trait]
    impl ObservationBackend for MockBackend {
        async fn capture(
            &self,
            scenario: &Scenario,
            target_artifact_id: ArtifactId,
        ) -> std::result::Result<ObservationSet, ObservationError> {
            if target_artifact_id == scenario.executable_artifact_id {
                Ok(self.original.clone())
            } else {
                Ok(self.candidate.clone())
            }
        }
    }

    fn make_executor(client: Arc<TestClient>, backend: Arc<MockBackend>) -> ScenarioExecutor {
        ScenarioExecutor::new(ProjectId::new(), client, backend)
    }

    fn make_scenario() -> Scenario {
        Scenario::new(
            "scenario-1",
            WorkItemId::new().to_string(),
            EntityId::new(),
            InitialState::new(HashMap::new(), vec![], PathBuf::from("/tmp")),
            ArtifactId::new(),
            ArtifactId::new(),
            vec![],
            ComparisonLevel::Function,
        )
    }

    #[tokio::test]
    async fn execute_original_imports_dynamic_observations() {
        let original = base_observation_set().add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set();
        let backend = Arc::new(MockBackend {
            original,
            candidate,
        });
        let client = Arc::new(TestClient::new());
        let executor = make_executor(client.clone(), backend);
        let scenario = make_scenario();

        let _obs = executor.execute_original(&scenario).await.unwrap();
        let commands = client.commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            commands[0],
            ApplicationCommand::ImportDynamicObservation(_)
        ));
    }

    #[tokio::test]
    async fn execute_candidate_imports_dynamic_observations() {
        let original = base_observation_set();
        let candidate = base_observation_set().add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));
        let backend = Arc::new(MockBackend {
            original,
            candidate,
        });
        let client = Arc::new(TestClient::new());
        let executor = make_executor(client.clone(), backend);
        let scenario = make_scenario();

        let _obs = executor
            .execute_candidate(&scenario, scenario.candidate_artifact_id)
            .await
            .unwrap();
        let commands = client.commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            commands[0],
            ApplicationCommand::ImportDynamicObservation(_)
        ));
    }

    #[tokio::test]
    async fn compare_and_record_emits_record_verification_comparison() {
        let original = base_observation_set().add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set().add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let backend = Arc::new(MockBackend {
            original,
            candidate,
        });
        let client = Arc::new(TestClient::new());
        let executor = make_executor(client.clone(), backend);
        let scenario = make_scenario();

        let original_obs = executor.execute_original(&scenario).await.unwrap();
        let candidate_obs = executor
            .execute_candidate(&scenario, scenario.candidate_artifact_id)
            .await
            .unwrap();
        let comparison = executor
            .compare_and_record(&scenario, &original_obs, &candidate_obs)
            .await
            .unwrap();

        assert_eq!(comparison.overall, super::super::ComparisonResult::Equal);
        let commands = client.commands();
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, ApplicationCommand::RecordVerificationComparison(_)))
        );
    }

    #[tokio::test]
    async fn wave7_backend_mock_captures_observations() {
        let backend = Arc::new(Wave7ObservationBackend::mock());
        let client = Arc::new(TestClient::new());
        let executor = ScenarioExecutor::new(ProjectId::new(), client, backend);
        let entity = EntityId::new();
        let scenario = Scenario::new(
            "scenario-1",
            WorkItemId::new().to_string(),
            entity,
            InitialState::new(HashMap::new(), vec![], PathBuf::from("/tmp")),
            ArtifactId::new(),
            ArtifactId::new(),
            vec![crate::dynamic::scenario::Step::CaptureRegisters],
            ComparisonLevel::Function,
        );

        let obs = executor.execute_original(&scenario).await.unwrap();
        assert!(
            obs.observations
                .iter()
                .any(|o| o.kind.to_string() == "debug.registers")
        );
    }
}
