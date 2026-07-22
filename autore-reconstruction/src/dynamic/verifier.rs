//! Scenario validation: ensures a proposed debugger scenario is safe
//! to execute before the provider receives it.
//!
//! The [`ScenarioVerifier`] enforces four invariants:
//!
//! 1. Every [`EntityId`] referenced in steps exists in the known entity map.
//! 2. Every [`Step::CaptureMemoryRegion`] address falls within a known
//!    mapped segment.
//! 3. Every [`Step::CaptureExternalCall`] API is in the operator's
//!    allowlist.
//! 4. Every [`Step::CaptureMemoryDelta`] size does not exceed 64 KiB.
//!
//! A scenario that fails any check produces a
//! [`ScenarioValidationError`] and is **never executed**.

use std::collections::{HashMap, HashSet};
use std::fmt;

use autore_schema::domain::{NamespacedId, SemanticEntity};
use autore_schema::ids::EntityId;

use super::scenario::{AddressRange, Scenario, Step};

// ---------------------------------------------------------------------------
// Validation error
// ---------------------------------------------------------------------------

/// Maximum allowed size for a `CaptureMemoryDelta` in bytes (64 KiB).
const MAX_MEMORY_DELTA_BYTES: usize = 64 * 1024;

/// Errors produced by [`ScenarioVerifier::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioValidationError {
    /// The scenario has no setup operations (launch or attach).
    MissingSetup,
    /// The scenario body contains no steps.
    EmptyScenario,
    /// A step references an [`EntityId`] not present in the known entity map.
    UnknownEntity(EntityId),
    /// A `CaptureMemoryRegion` address is outside every known mapped segment.
    UnmappedAddress(u128),
    /// A `CaptureExternalCall` references an API not in the allowlist.
    DisallowedApi(NamespacedId),
    /// A `CaptureMemoryDelta` exceeds the 64 KiB limit.
    MemoryDeltaTooLarge {
        /// The requested size in bytes.
        size: usize,
        /// The maximum allowed size in bytes (always 65536).
        max: usize,
    },
}

impl fmt::Display for ScenarioValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScenarioValidationError::MissingSetup => {
                write!(f, "scenario has no setup operations (launch or attach)")
            }
            ScenarioValidationError::EmptyScenario => {
                write!(f, "scenario body contains no steps")
            }
            ScenarioValidationError::UnknownEntity(id) => {
                write!(f, "unknown entity referenced in scenario step: {id}")
            }
            ScenarioValidationError::UnmappedAddress(addr) => {
                write!(f, "address {addr:#x} is outside every known mapped segment")
            }
            ScenarioValidationError::DisallowedApi(api) => {
                write!(f, "external API '{api}' is not in the operator allowlist")
            }
            ScenarioValidationError::MemoryDeltaTooLarge { size, max } => {
                write!(
                    f,
                    "capture-memory-delta size {size} bytes exceeds maximum of {max} bytes"
                )
            }
        }
    }
}

impl std::error::Error for ScenarioValidationError {}

// ---------------------------------------------------------------------------
// Verifier
// ---------------------------------------------------------------------------

/// Validates a [`Scenario`] against known entities, mapped segments, and
/// an API allowlist.
///
/// The verifier is a pure function with no side effects — it reads its
/// inputs and returns `Ok(())` or the first validation error encountered.
pub struct ScenarioVerifier;

impl ScenarioVerifier {
    /// Validates `scenario` against the given constraints.
    ///
    /// # Arguments
    ///
    /// * `scenario` — the proposed debugger scenario.
    /// * `entities_by_id` — map of known entities keyed by [`EntityId`].
    /// * `mapped_segments` — contiguous address ranges that are mapped
    ///   (e.g., from IDA's segment snapshot).
    /// * `allowed_external_apis` — the set of [`NamespacedId`]s the
    ///   operator permits for `CaptureExternalCall`.
    pub fn validate(
        scenario: &Scenario,
        entities_by_id: &HashMap<EntityId, SemanticEntity>,
        mapped_segments: &[AddressRange],
        allowed_external_apis: &HashSet<NamespacedId>,
    ) -> Result<(), ScenarioValidationError> {
        // 1. Setup must be non-empty.
        if scenario.setup.is_empty() {
            return Err(ScenarioValidationError::MissingSetup);
        }

        // 2. Body must be non-empty.
        if scenario.body.is_empty() {
            return Err(ScenarioValidationError::EmptyScenario);
        }

        // 3. Walk each step and enforce constraints.
        for step in &scenario.body {
            match step {
                Step::SetBreakpoint { entity }
                | Step::RemoveBreakpoint { entity }
                | Step::CaptureArguments { entity }
                | Step::CaptureReturnValue { entity }
                | Step::CaptureGlobalValue { entity } => {
                    if !entities_by_id.contains_key(entity) {
                        return Err(ScenarioValidationError::UnknownEntity(*entity));
                    }
                }

                Step::CaptureMemoryRegion { addr, .. } => {
                    if !mapped_segments.iter().any(|seg| seg.contains(*addr)) {
                        return Err(ScenarioValidationError::UnmappedAddress(*addr));
                    }
                }

                Step::CaptureMemoryDelta { addr, size } => {
                    if *size > MAX_MEMORY_DELTA_BYTES {
                        return Err(ScenarioValidationError::MemoryDeltaTooLarge {
                            size: *size,
                            max: MAX_MEMORY_DELTA_BYTES,
                        });
                    }
                    if !mapped_segments.iter().any(|seg| seg.contains(*addr)) {
                        return Err(ScenarioValidationError::UnmappedAddress(*addr));
                    }
                }

                Step::CaptureExternalCall { api } => {
                    if !allowed_external_apis.contains(api) {
                        return Err(ScenarioValidationError::DisallowedApi(api.clone()));
                    }
                }

                // Steps with no entity/address/api constraint.
                Step::Continue
                | Step::Step
                | Step::Finish
                | Step::CaptureRegisters
                | Step::CaptureCallTarget
                | Step::CaptureException => {}
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic::scenario::{AddressRange, Scenario, SetupOp, Step, StopOp};

    use autore_schema::domain::{NamespacedId, SemanticEntity, Timestamp};
    use autore_schema::ids::{ArtifactId, EntityId, ProjectId};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn make_entity() -> (EntityId, SemanticEntity) {
        let id = EntityId::new();
        let entity = SemanticEntity::new(
            ProjectId::new(),
            NamespacedId::parse("core.function").unwrap(),
            None,
            Some("test_func".into()),
        );
        // Override the auto-generated id with our chosen one.
        let entity = SemanticEntity {
            id,
            project: entity.project,
            kind: entity.kind,
            stable_key: entity.stable_key,
            display_name: entity.display_name,
            created_at: Timestamp::now(),
            metadata: entity.metadata,
        };
        (id, entity)
    }

    fn make_scenario(entity: EntityId) -> Scenario {
        Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: ArtifactId::new(),
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

    fn make_segments() -> Vec<AddressRange> {
        vec![AddressRange::new(0x400000, 0x500000)]
    }

    fn make_allowlist() -> HashSet<NamespacedId> {
        let mut set = HashSet::new();
        set.insert(NamespacedId::parse("win32.kernel32.create-file").unwrap());
        set
    }

    fn make_entity_map(
        entities: &[(EntityId, SemanticEntity)],
    ) -> HashMap<EntityId, SemanticEntity> {
        entities.iter().cloned().collect()
    }

    // -----------------------------------------------------------------------
    // Required acceptance tests
    // -----------------------------------------------------------------------

    #[test]
    fn valid_scenario_passes() {
        let (eid, entity) = make_entity();
        let scenario = make_scenario(eid);
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(result.is_ok(), "valid scenario should pass: {result:?}");
    }

    #[test]
    fn scenario_rejects_unknown_entity_id() {
        let fake_id = EntityId::new();
        let scenario = make_scenario(fake_id);
        let map = make_entity_map(&[]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(matches!(
            result,
            Err(ScenarioValidationError::UnknownEntity(id)) if id == fake_id
        ));
    }

    #[test]
    fn scenario_rejects_unmapped_address() {
        let (eid, entity) = make_entity();
        let mut scenario = make_scenario(eid);
        scenario.body.push(Step::CaptureMemoryRegion {
            addr: 0xDEAD_0000, // outside [0x400000, 0x500000)
            size: 64,
        });
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(matches!(
            result,
            Err(ScenarioValidationError::UnmappedAddress(0xDEAD_0000))
        ));
    }

    #[test]
    fn scenario_rejects_disallowed_external_api() {
        let (eid, entity) = make_entity();
        let mut scenario = make_scenario(eid);
        let bad_api = NamespacedId::parse("evil.hook.something").unwrap();
        scenario.body.push(Step::CaptureExternalCall {
            api: bad_api.clone(),
        });
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(matches!(
            result,
            Err(ScenarioValidationError::DisallowedApi(ref a)) if a == &bad_api
        ));
    }

    #[test]
    fn scenario_rejects_too_large_memory_delta() {
        let (eid, entity) = make_entity();
        let mut scenario = make_scenario(eid);
        scenario.body.push(Step::CaptureMemoryDelta {
            addr: 0x401000,
            size: 128 * 1024, // 128 KiB > 64 KiB limit
        });
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(matches!(
            result,
            Err(ScenarioValidationError::MemoryDeltaTooLarge {
                size: 131072,
                max: 65536
            })
        ));
    }

    // -----------------------------------------------------------------------
    // Edge case tests
    // -----------------------------------------------------------------------

    #[test]
    fn scenario_rejects_missing_setup() {
        let (eid, entity) = make_entity();
        let scenario = Scenario::new(
            vec![], // empty setup
            vec![Step::SetBreakpoint { entity: eid }],
            vec![StopOp::TerminateTarget],
        );
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(matches!(result, Err(ScenarioValidationError::MissingSetup)));
    }

    #[test]
    fn scenario_rejects_empty_body() {
        let scenario = Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: ArtifactId::new(),
                env: HashMap::new(),
                working_dir: PathBuf::from("/tmp"),
            }],
            vec![], // empty body
            vec![StopOp::TerminateTarget],
        );
        let map = make_entity_map(&[]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(matches!(
            result,
            Err(ScenarioValidationError::EmptyScenario)
        ));
    }

    #[test]
    fn memory_delta_at_exact_limit_passes() {
        let (eid, entity) = make_entity();
        let scenario = Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: ArtifactId::new(),
                env: HashMap::new(),
                working_dir: PathBuf::from("/tmp"),
            }],
            vec![Step::CaptureMemoryDelta {
                addr: 0x401000,
                size: 64 * 1024, // exactly 64 KiB — should pass
            }],
            vec![StopOp::TerminateTarget],
        );
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(
            result.is_ok(),
            "exactly 64 KiB delta should pass: {result:?}"
        );
    }

    #[test]
    fn allowed_external_api_passes() {
        let (eid, entity) = make_entity();
        let allowed = NamespacedId::parse("win32.kernel32.create-file").unwrap();
        let scenario = Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: ArtifactId::new(),
                env: HashMap::new(),
                working_dir: PathBuf::from("/tmp"),
            }],
            vec![Step::CaptureExternalCall { api: allowed }],
            vec![StopOp::TerminateTarget],
        );
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        assert!(result.is_ok(), "allowlisted API should pass: {result:?}");
    }

    #[test]
    fn memory_delta_also_validates_address() {
        let (eid, entity) = make_entity();
        let scenario = Scenario::new(
            vec![SetupOp::LaunchTarget {
                exe_artifact: ArtifactId::new(),
                env: HashMap::new(),
                working_dir: PathBuf::from("/tmp"),
            }],
            vec![Step::CaptureMemoryDelta {
                addr: 0xDEAD_0000, // unmapped
                size: 256,         // within limit
            }],
            vec![StopOp::TerminateTarget],
        );
        let map = make_entity_map(&[(eid, entity)]);
        let result =
            ScenarioVerifier::validate(&scenario, &map, &make_segments(), &make_allowlist());
        // Address check comes after size check, but since size is OK,
        // the address check should trigger.
        assert!(matches!(
            result,
            Err(ScenarioValidationError::UnmappedAddress(0xDEAD_0000))
        ));
    }

    #[test]
    fn validation_error_display_messages() {
        let e = ScenarioValidationError::MissingSetup;
        assert!(e.to_string().contains("setup"));

        let e = ScenarioValidationError::EmptyScenario;
        assert!(e.to_string().contains("no steps"));

        let e = ScenarioValidationError::UnknownEntity(EntityId::new());
        assert!(e.to_string().contains("unknown entity"));

        let e = ScenarioValidationError::UnmappedAddress(0x4000);
        assert!(e.to_string().contains("0x4000"));

        let e = ScenarioValidationError::DisallowedApi(NamespacedId::parse("test.api").unwrap());
        assert!(e.to_string().contains("test.api"));

        let e = ScenarioValidationError::MemoryDeltaTooLarge {
            size: 200_000,
            max: 65536,
        };
        assert!(e.to_string().contains("200000"));
        assert!(e.to_string().contains("65536"));
    }
}
