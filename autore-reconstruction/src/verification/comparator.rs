//! Deterministic typed comparison of observation sets.

use autore_schema::domain::EvidenceValue;

use super::types::{
    ComparisonCounts, ComparisonResult, NormalizationRule, Observation, ObservationSet,
    VerificationComparison,
};

/// Compares two observation sets under the supplied normalization rules.
///
/// The comparison is purely structural and typed: no raw byte comparisons are
/// performed. Address observations are relocated using the per-side image bases
/// declared in a [`NormalizationRule::RelocatedAddress`] rule, timestamps and
/// seeds are replaced by placeholders, and environment-specific handles are
/// masked before comparison.
pub fn compare(
    original: &ObservationSet,
    candidate: &ObservationSet,
    normalization_rules: &[NormalizationRule],
) -> VerificationComparison {
    let mut per_observation_results = Vec::new();

    let max_len = original
        .observations
        .len()
        .max(candidate.observations.len());
    for i in 0..max_len {
        match (original.observations.get(i), candidate.observations.get(i)) {
            (Some(a), Some(b)) => {
                per_observation_results.push(compare_pair(a, b, normalization_rules));
            }
            (Some(_), None) | (None, Some(_)) => {
                per_observation_results.push(ComparisonResult::NotObserved);
            }
            (None, None) => unreachable!(),
        }
    }

    if original.execution_failed || candidate.execution_failed {
        per_observation_results.push(ComparisonResult::ExecutionFailed);
    }

    let counts = count_results(&per_observation_results);
    let overall = compute_overall(&per_observation_results);

    let original_output =
        EvidenceValue::String(serde_json::to_string(original).unwrap_or_else(|_| "{}".to_string()));
    let candidate_output = EvidenceValue::String(
        serde_json::to_string(candidate).unwrap_or_else(|_| "{}".to_string()),
    );

    VerificationComparison::new(
        original.scenario_id.clone(),
        original_output,
        candidate_output,
        per_observation_results,
        counts,
        overall,
    )
}

fn compare_pair(
    original: &Observation,
    candidate: &Observation,
    rules: &[NormalizationRule],
) -> ComparisonResult {
    if original == candidate {
        return ComparisonResult::Equal;
    }

    let normalized_original = normalize(original, rules, true);
    let normalized_candidate = normalize(candidate, rules, false);

    if normalized_original.kind != normalized_candidate.kind {
        return ComparisonResult::Inconclusive;
    }

    if normalized_original == normalized_candidate {
        ComparisonResult::EquivalentUnderNormalization
    } else {
        ComparisonResult::Different
    }
}

fn normalize(
    observation: &Observation,
    rules: &[NormalizationRule],
    is_original: bool,
) -> Observation {
    let mut normalized = observation.clone();

    for rule in rules {
        match rule {
            NormalizationRule::RelocatedAddress {
                original_base_address,
                candidate_base_address,
            } => {
                if let Some(address) = normalized.address {
                    let base = if is_original {
                        *original_base_address
                    } else {
                        *candidate_base_address
                    };
                    normalized.address = Some(address.saturating_sub(base));
                    normalized.data =
                        normalize_address_in_value(&normalized.data, rule, is_original);
                }
            }
            NormalizationRule::Timestamp { placeholder } => {
                if normalized.kind.to_string() == "debug.timestamp"
                    || normalized.timestamp.is_some()
                {
                    normalized.timestamp = Some(*placeholder);
                    if normalized.kind.to_string() == "debug.timestamp" {
                        normalized.data = serde_json::json!(*placeholder);
                    }
                }
            }
            NormalizationRule::RandomSeed { placeholder } => {
                if normalized.kind.to_string() == "debug.seed" {
                    normalized.data = serde_json::json!(*placeholder);
                }
            }
            NormalizationRule::EnvSpecificHandle { placeholder } => {
                if normalized.kind.to_string() == "debug.handle" {
                    normalized.data = serde_json::json!(placeholder);
                }
            }
        }
    }

    normalized
}

fn normalize_address_in_value(
    value: &serde_json::Value,
    rule: &NormalizationRule,
    is_original: bool,
) -> serde_json::Value {
    let NormalizationRule::RelocatedAddress {
        original_base_address,
        candidate_base_address,
    } = rule
    else {
        return value.clone();
    };

    match value {
        serde_json::Value::Number(n) if n.is_u64() => {
            if let Some(address) = n.as_u64() {
                let base = if is_original {
                    *original_base_address
                } else {
                    *candidate_base_address
                };
                serde_json::json!(address.saturating_sub(base as u64))
            } else {
                value.clone()
            }
        }
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                out.insert(k.clone(), normalize_address_in_value(v, rule, is_original));
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| normalize_address_in_value(v, rule, is_original))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn count_results(results: &[ComparisonResult]) -> ComparisonCounts {
    let mut counts = ComparisonCounts::zero();
    for result in results {
        match result {
            ComparisonResult::Equal => counts.equal_count += 1,
            ComparisonResult::EquivalentUnderNormalization => counts.equivalent_count += 1,
            ComparisonResult::Different => counts.different_count += 1,
            ComparisonResult::Inconclusive => counts.inconclusive_count += 1,
            ComparisonResult::NotObserved => counts.not_observed_count += 1,
            ComparisonResult::ExecutionFailed => counts.execution_failed_count += 1,
        }
    }
    counts
}

fn compute_overall(results: &[ComparisonResult]) -> ComparisonResult {
    if results.contains(&ComparisonResult::ExecutionFailed) {
        return ComparisonResult::ExecutionFailed;
    }
    if results.contains(&ComparisonResult::Different) {
        return ComparisonResult::Different;
    }
    if results.contains(&ComparisonResult::Inconclusive) {
        return ComparisonResult::Inconclusive;
    }
    if results.contains(&ComparisonResult::NotObserved) {
        return ComparisonResult::NotObserved;
    }
    if results.contains(&ComparisonResult::EquivalentUnderNormalization) {
        return ComparisonResult::EquivalentUnderNormalization;
    }
    ComparisonResult::Equal
}

#[cfg(test)]
mod tests {
    use super::super::types::{
        ComparisonLevel, ComparisonResult, ExecutionDiagnostic, InitialState, NormalizationRule,
        Observation, ObservationSet, Scenario,
    };
    use super::super::{ObservationBackend, ScenarioExecutor, compare};
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

    #[test]
    fn scenario_normalize_relocated_addresses() {
        let original = base_observation_set()
            .with_image_base(0x400000)
            .add_observation(
                Observation::new(
                    debug_kind("address"),
                    serde_json::json!({"address": 0x401000}),
                )
                .with_address(0x401000),
            );

        let candidate = base_observation_set()
            .with_image_base(0x410000)
            .add_observation(
                Observation::new(
                    debug_kind("address"),
                    serde_json::json!({"address": 0x411000}),
                )
                .with_address(0x411000),
            );

        let rules = vec![NormalizationRule::RelocatedAddress {
            original_base_address: 0x400000,
            candidate_base_address: 0x410000,
        }];

        let comparison = compare(&original, &candidate, &rules);
        assert_eq!(
            comparison.overall,
            ComparisonResult::EquivalentUnderNormalization
        );
        assert_eq!(comparison.counts.equivalent_count, 1);
    }

    #[test]
    fn scenario_normalize_timestamps() {
        let original = base_observation_set().add_observation(
            Observation::new(
                debug_kind("timestamp"),
                serde_json::json!(1_000_000_000_u64),
            )
            .with_timestamp(1_000_000_000),
        );
        let candidate = base_observation_set().add_observation(
            Observation::new(
                debug_kind("timestamp"),
                serde_json::json!(1_500_000_000_u64),
            )
            .with_timestamp(1_500_000_000),
        );

        let rules = vec![NormalizationRule::Timestamp { placeholder: 0 }];

        let comparison = compare(&original, &candidate, &rules);
        assert_eq!(
            comparison.overall,
            ComparisonResult::EquivalentUnderNormalization
        );
        assert_eq!(comparison.counts.equal_count, 0);
        assert_eq!(comparison.counts.equivalent_count, 1);
    }

    #[test]
    fn scenario_normalize_random_seeds_before_comparison() {
        let original = base_observation_set()
            .add_observation(Observation::new(debug_kind("seed"), serde_json::json!(42)));
        let candidate = base_observation_set()
            .add_observation(Observation::new(debug_kind("seed"), serde_json::json!(99)));

        let rules = vec![NormalizationRule::RandomSeed { placeholder: 0 }];

        let comparison = compare(&original, &candidate, &rules);
        assert_eq!(
            comparison.overall,
            ComparisonResult::EquivalentUnderNormalization
        );
        assert_eq!(comparison.counts.equivalent_count, 1);
    }

    #[test]
    fn equal_under_normalization_stored_as_equivalent_under_normalization() {
        let original = base_observation_set()
            .with_image_base(0x400000)
            .add_observation(
                Observation::new(debug_kind("rip"), serde_json::json!({"rip": 0x401234}))
                    .with_address(0x401234),
            );
        let candidate = base_observation_set()
            .with_image_base(0x410000)
            .add_observation(
                Observation::new(debug_kind("rip"), serde_json::json!({"rip": 0x411234}))
                    .with_address(0x411234),
            );

        let rules = vec![NormalizationRule::RelocatedAddress {
            original_base_address: 0x400000,
            candidate_base_address: 0x410000,
        }];

        let comparison = compare(&original, &candidate, &rules);
        assert_eq!(
            comparison.overall,
            ComparisonResult::EquivalentUnderNormalization
        );
        assert!(comparison.matches);
        assert!(!comparison.requires_repair);
    }

    #[test]
    fn different_diff_boosted_to_repair() {
        let original = base_observation_set().add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 1}),
        ));
        let candidate = base_observation_set().add_observation(Observation::new(
            debug_kind("register"),
            serde_json::json!({"rax": 2}),
        ));

        let comparison = compare(&original, &candidate, &[]);
        assert_eq!(comparison.overall, ComparisonResult::Different);
        assert_eq!(comparison.counts.different_count, 1);
        assert!(comparison.requires_repair);
    }

    #[test]
    fn execution_failed_exited_with_code_139_classified_execution_failed_with_diagnostic() {
        let original = base_observation_set()
            .with_exit_code(139)
            .with_execution_failure(ExecutionDiagnostic::new(
                "exit-code-139",
                "process exited with signal 11 (SIGSEGV)",
            ));
        let candidate = base_observation_set();

        let comparison = compare(&original, &candidate, &[]);
        assert_eq!(comparison.overall, ComparisonResult::ExecutionFailed);
        assert_eq!(comparison.counts.execution_failed_count, 1);
        assert!(comparison.requires_repair);
        assert!(comparison.requires_repair);
    }

    // Helper client that records commands and handles RecordVerificationComparison.
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

    // Mock backend returning canned observation sets.
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
        ) -> std::result::Result<ObservationSet, super::super::ObservationError> {
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

        assert_eq!(comparison.overall, ComparisonResult::Equal);
        let commands = client.commands();
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, ApplicationCommand::RecordVerificationComparison(_)))
        );
    }
}
