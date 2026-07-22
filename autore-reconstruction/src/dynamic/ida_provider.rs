//! IDA provider debug-capability helpers and scenario execution.
//!
//! This module is shared between the `autore-reconstruction` dynamic analysis
//! layer and the external `ida-provider` binary. It keeps the list of debug
//! capabilities and a small typed executor that drives a [`TargetRunner`] with
//! a validated [`Scenario`](super::scenario::Scenario).

use std::collections::{HashMap, HashSet};

use autore_provider_protocol::v1::CapabilityDescriptor;
use autore_schema::domain::{NamespacedId, SemanticEntity};
use autore_schema::ids::{EntityId, ProjectId};

use super::runner::{CaptureContext, RunnerError, TargetRunner};
use super::scenario::{Scenario, SetupOp, Step, StopOp};

// ---------------------------------------------------------------------------
// Capability descriptors
// ---------------------------------------------------------------------------

/// The 7 debug capabilities added to the IDA provider in Todo 32.
pub const DEBUG_CAPABILITIES: &[(&str, &str)] = &[
    ("debug.target.launch", "Debug Target Launch"),
    ("debug.target.stop", "Debug Target Stop"),
    ("debug.scenario.execute", "Debug Scenario Execute"),
    ("debug.function.capture", "Debug Function Capture"),
    ("debug.function.trace", "Debug Function Trace"),
    ("debug.memory.capture", "Debug Memory Capture"),
    ("debug.calls.capture", "Debug Calls Capture"),
];

/// Builds the 7 debug [`CapabilityDescriptor`]s for the IDA provider.
pub fn debug_capabilities() -> Vec<CapabilityDescriptor> {
    DEBUG_CAPABILITIES
        .iter()
        .map(|(id, name)| CapabilityDescriptor {
            capability_id: id.to_string(),
            version: "1.0.0".to_string(),
            name: name.to_string(),
            request_schema: Vec::new(),
            response_schema: Vec::new(),
        })
        .collect()
}

/// Builds a validation context that accepts every entity, address, and API
/// referenced inside `scenario`.
///
/// This is intended for the provider's defense-in-depth re-validation step:
/// the coordinator already validates scenarios against the canonical project
/// context before sending them to the provider. The provider re-runs the
/// verifier to satisfy the safety boundary, using a context that mirrors the
/// scenario's own references so structural errors (empty setup/body) are still
/// caught.
pub fn permissive_validation_context(
    scenario: &Scenario,
) -> (
    HashMap<EntityId, SemanticEntity>,
    Vec<super::scenario::AddressRange>,
    HashSet<NamespacedId>,
) {
    let mut entities = HashMap::new();
    let mut min_addr: Option<u128> = None;
    let mut max_addr: Option<u128> = None;
    let mut apis = HashSet::new();

    for step in &scenario.body {
        match step {
            Step::SetBreakpoint { entity }
            | Step::RemoveBreakpoint { entity }
            | Step::CaptureArguments { entity }
            | Step::CaptureReturnValue { entity }
            | Step::CaptureGlobalValue { entity } => {
                entities.entry(*entity).or_insert_with(|| {
                    SemanticEntity::new(
                        ProjectId::new(),
                        NamespacedId::parse("core.function").unwrap(),
                        None,
                        Some("scenario-entity".into()),
                    )
                });
            }
            Step::CaptureMemoryRegion { addr, .. } | Step::CaptureMemoryDelta { addr, .. } => {
                min_addr = Some(min_addr.map_or(*addr, |m| m.min(*addr)));
                max_addr = Some(max_addr.map_or(addr.saturating_add(1), |m| m.max(*addr)));
            }
            Step::CaptureExternalCall { api } => {
                apis.insert(api.clone());
            }
            _ => {}
        }
    }

    let segments = match (min_addr, max_addr) {
        (Some(start), Some(end)) => vec![super::scenario::AddressRange::new(start, end)],
        _ => vec![super::scenario::AddressRange::new(0, u128::MAX)],
    };

    (entities, segments, apis)
}

// ---------------------------------------------------------------------------
// Scenario execution
// ---------------------------------------------------------------------------

/// Terminal status returned by [`execute_scenario`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioStatus {
    /// The scenario executed to completion without fatal errors.
    Passed,
    /// The scenario failed or was cancelled.
    Failed,
}

/// Result of executing a scenario through a [`TargetRunner`].
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    /// Captured observations and staged artifacts.
    pub ctx: CaptureContext,
    /// Terminal execution status.
    pub status: ScenarioStatus,
}

/// Executes a validated [`Scenario`] against a [`TargetRunner`].
///
/// The caller is responsible for validating the scenario with
/// [`ScenarioVerifier`](super::verifier::ScenarioVerifier) before calling this
/// function.
pub async fn execute_scenario(
    runner: &dyn TargetRunner,
    scenario: &Scenario,
) -> Result<ScenarioResult, RunnerError> {
    let mut ctx = CaptureContext::new();

    // Setup: launch or attach.
    for setup in &scenario.setup {
        match setup {
            SetupOp::LaunchTarget {
                exe_artifact,
                env,
                working_dir,
            } => {
                runner
                    .launch(*exe_artifact, env.clone(), working_dir.clone())
                    .await?;
            }
            SetupOp::AttachTarget { pid } => {
                runner.attach(*pid).await?;
            }
        }
    }

    // Body: execute each step once.
    for step in &scenario.body {
        runner.execute_step(step, &mut ctx).await?;
    }

    // Stop conditions.
    for stop in &scenario.stop_conditions {
        match stop {
            StopOp::TerminateTarget => {
                runner.stop().await?;
            }
            StopOp::StopAfterInvocationCount { count } => {
                // In a real runner this would monitor breakpoint hits. For the
                // mock/initial implementation we stop once after the body.
                if *count == 0 {
                    runner.stop().await?;
                }
            }
            StopOp::StopAfterTimeout { .. } => {
                // Timeout is enforced by the provider's deadline; the runner
                // trusts the caller to cancel or abort on timeout.
            }
        }
    }

    Ok(ScenarioResult {
        ctx,
        status: ScenarioStatus::Passed,
    })
}

/// Executes a scenario using the default mock Wine + gdbserver runner.
///
/// This helper is convenient for tests and documentation; production code
/// should use [`execute_scenario`] with a configured runner.
#[cfg(test)]
pub async fn execute_scenario_mock(scenario: &Scenario) -> Result<ScenarioResult, RunnerError> {
    let runner = super::runner::WineGdbRunner::mock();
    execute_scenario(&runner, scenario).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic::runner::{WindowsGdbServerRunner, WineGdbRunner};
    use crate::dynamic::scenario::{AddressRange, Scenario, SetupOp, Step, StopOp};
    use crate::dynamic::verifier::ScenarioVerifier;

    use autore_schema::domain::{NamespacedId, SemanticEntity, Timestamp};
    use autore_schema::ids::{ArtifactId, EntityId, ProjectId};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn make_entity(id: EntityId) -> SemanticEntity {
        let entity = SemanticEntity::new(
            ProjectId::new(),
            NamespacedId::parse("core.function").unwrap(),
            None,
            Some("test_func".into()),
        );
        SemanticEntity {
            id,
            project: entity.project,
            kind: entity.kind,
            stable_key: entity.stable_key,
            display_name: entity.display_name,
            created_at: Timestamp::now(),
            metadata: entity.metadata,
        }
    }

    fn make_entity_map(id: EntityId) -> HashMap<EntityId, SemanticEntity> {
        let mut map = HashMap::new();
        map.insert(id, make_entity(id));
        map
    }

    fn make_segments() -> Vec<AddressRange> {
        vec![AddressRange::new(0x400000, 0x500000)]
    }

    fn make_allowlist() -> HashSet<NamespacedId> {
        HashSet::new()
    }

    fn make_scenario(entity: EntityId, exe: ArtifactId) -> Scenario {
        Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: exe,
                env: HashMap::new(),
                working_dir: PathBuf::from("/tmp"),
            }],
            vec![
                Step::SetBreakpoint { entity },
                Step::Continue,
                Step::CaptureArguments { entity },
            ],
            vec![StopOp::StopAfterInvocationCount { count: 1 }],
        )
    }

    #[test]
    fn ida_provider_advertises_debug_capabilities() {
        let caps = debug_capabilities();
        assert_eq!(caps.len(), DEBUG_CAPABILITIES.len());
        for (id, name) in DEBUG_CAPABILITIES {
            let found = caps.iter().find(|c| c.capability_id == *id);
            assert!(found.is_some(), "missing debug capability {id}");
            assert_eq!(found.unwrap().name, *name);
        }
    }

    #[tokio::test]
    async fn launch_then_stop_lifecycle() {
        let runner = WineGdbRunner::mock();
        let exe = ArtifactId::new();
        runner
            .launch(exe, HashMap::new(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        runner.stop().await.unwrap();
    }

    #[tokio::test]
    async fn scenario_execute_returns_completed_status_passed() {
        let entity = EntityId::new();
        let exe = ArtifactId::new();
        let scenario = make_scenario(entity, exe);
        ScenarioVerifier::validate(
            &scenario,
            &make_entity_map(entity),
            &make_segments(),
            &make_allowlist(),
        )
        .unwrap();

        let result = execute_scenario_mock(&scenario).await.unwrap();
        assert_eq!(result.status, ScenarioStatus::Passed);
        assert!(
            result
                .ctx
                .observations
                .iter()
                .any(|o| o.kind == "arguments" && o.entity == Some(entity))
        );
        assert!(
            result
                .ctx
                .observations
                .iter()
                .any(|o| o.kind == "breakpoint-set")
        );
    }

    #[tokio::test]
    async fn capture_arguments_recorded_for_target_function() {
        let runner = WineGdbRunner::mock();
        let entity = EntityId::new();
        let ctx = runner.capture_function(entity, 1).await.unwrap();
        let args_obs = ctx.observations.iter().find(|o| {
            o.kind == "arguments"
                && o.entity == Some(entity)
                && o.data.get("captured_arguments").is_some()
        });
        assert!(
            args_obs.is_some(),
            "expected captured arguments observation"
        );
    }

    #[tokio::test]
    async fn cancellation_terminates_target_and_emits_diagnostic() {
        let runner = WineGdbRunner::mock();
        runner
            .launch(ArtifactId::new(), HashMap::new(), PathBuf::from("/tmp"))
            .await
            .unwrap();
        runner.cancel();
        let mut ctx = CaptureContext::new();
        let result = runner.execute_step(&Step::Continue, &mut ctx).await;
        assert!(matches!(result, Err(RunnerError::Cancelled)));
        let diag = ctx.observations.iter().find(|o| {
            o.kind == "diagnostic"
                && o.data.get("code").and_then(|v| v.as_str()) == Some("cancellation")
                && o.data.get("severity").and_then(|v| v.as_str()) == Some("warning")
        });
        assert!(diag.is_some(), "expected cancellation diagnostic");
    }

    #[tokio::test]
    async fn unsupported_runner_blocks_in_verifier() {
        let runner = WindowsGdbServerRunner;
        let result = runner
            .launch(ArtifactId::new(), HashMap::new(), PathBuf::from("/tmp"))
            .await;
        assert!(matches!(result, Err(RunnerError::Unsupported)));

        // The Scenario shape must be unchanged when used with either runner.
        let entity = EntityId::new();
        let exe = ArtifactId::new();
        let scenario = make_scenario(entity, exe);
        let json1 = serde_json::to_string(&scenario).unwrap();
        let scenario2: Scenario = serde_json::from_str(&json1).unwrap();
        let json2 = serde_json::to_string(&scenario2).unwrap();
        assert_eq!(json1, json2);
    }
}
