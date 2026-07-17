//! Operation state machine — unit-only, no schema dependency.
//!
//! `OperationState` is the lifecycle status of a long-running operation.
//! Transitions are validated at the domain layer; the persistence layer
//! calls `OperationState::transition(target)` before updating the DB.

/// The lifecycle status of an operation (§16).
///
/// Valid transitions:
/// - Queued → Running
/// - Running ↔ Paused
/// - Running → Cancelling, Completed, Failed, Blocked, Inconclusive
/// - Paused → Running
/// - Cancelling → Cancelled
/// - Blocked → Running
///
/// Terminal states: Completed, Failed, Cancelled, Inconclusive.
/// All other transitions are invalid and return `Error::InvalidStateTransition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OperationState {
    Queued,
    Running,
    Paused,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    Blocked,
    Inconclusive,
}

impl OperationState {
    /// Validates a state transition from `self` to `target`.
    ///
    /// Only the documented transitions are allowed; all others
    /// return `Error::InvalidStateTransition`.
    pub fn transition(&self, target: &OperationState) -> crate::Result<()> {
        match (self, target) {
            // Queued → Running
            (OperationState::Queued, OperationState::Running) => Ok(()),
            // Running ↔ Paused
            (OperationState::Running, OperationState::Paused) => Ok(()),
            (OperationState::Paused, OperationState::Running) => Ok(()),
            // Running → Cancelling
            (OperationState::Running, OperationState::Cancelling) => Ok(()),
            // Cancelling → Cancelled
            (OperationState::Cancelling, OperationState::Cancelled) => Ok(()),
            // Running → Completed, Failed, Blocked, Inconclusive
            (OperationState::Running, OperationState::Completed) => Ok(()),
            (OperationState::Running, OperationState::Failed) => Ok(()),
            (OperationState::Running, OperationState::Blocked) => Ok(()),
            (OperationState::Running, OperationState::Inconclusive) => Ok(()),
            // Blocked → Running
            (OperationState::Blocked, OperationState::Running) => Ok(()),
            // Everything else is invalid
            _ => Err(crate::Error::InvalidStateTransition(format!(
                "{self} -> {target}"
            ))),
        }
    }

    /// Returns the discriminant string for database storage and filtering.
    pub fn kind(&self) -> &'static str {
        match self {
            OperationState::Queued => "Queued",
            OperationState::Running => "Running",
            OperationState::Paused => "Paused",
            OperationState::Cancelling => "Cancelling",
            OperationState::Completed => "Completed",
            OperationState::Failed => "Failed",
            OperationState::Cancelled => "Cancelled",
            OperationState::Blocked => "Blocked",
            OperationState::Inconclusive => "Inconclusive",
        }
    }

    /// Returns `true` if this state is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OperationState::Completed
                | OperationState::Failed
                | OperationState::Cancelled
                | OperationState::Inconclusive
        )
    }
}

impl std::fmt::Display for OperationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.kind())
    }
}

/// Parses a status string from the database into an `OperationState`.
pub fn operation_state_from_str(s: &str) -> Result<OperationState, String> {
    match s {
        "Queued" => Ok(OperationState::Queued),
        "Running" => Ok(OperationState::Running),
        "Paused" => Ok(OperationState::Paused),
        "Cancelling" => Ok(OperationState::Cancelling),
        "Completed" => Ok(OperationState::Completed),
        "Failed" => Ok(OperationState::Failed),
        "Cancelled" => Ok(OperationState::Cancelled),
        "Blocked" => Ok(OperationState::Blocked),
        "Inconclusive" => Ok(OperationState::Inconclusive),
        other => Err(format!("unknown operation state: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_state_transitions_valid() {
        // Queued → Running
        assert!(
            OperationState::Queued
                .transition(&OperationState::Running)
                .is_ok()
        );
        // Running ↔ Paused
        assert!(
            OperationState::Running
                .transition(&OperationState::Paused)
                .is_ok()
        );
        assert!(
            OperationState::Paused
                .transition(&OperationState::Running)
                .is_ok()
        );
        // Running → Cancelling
        assert!(
            OperationState::Running
                .transition(&OperationState::Cancelling)
                .is_ok()
        );
        // Cancelling → Cancelled
        assert!(
            OperationState::Cancelling
                .transition(&OperationState::Cancelled)
                .is_ok()
        );
        // Running → Completed
        assert!(
            OperationState::Running
                .transition(&OperationState::Completed)
                .is_ok()
        );
        // Running → Failed
        assert!(
            OperationState::Running
                .transition(&OperationState::Failed)
                .is_ok()
        );
        // Running → Blocked
        assert!(
            OperationState::Running
                .transition(&OperationState::Blocked)
                .is_ok()
        );
        // Running → Inconclusive
        assert!(
            OperationState::Running
                .transition(&OperationState::Inconclusive)
                .is_ok()
        );
        // Blocked → Running
        assert!(
            OperationState::Blocked
                .transition(&OperationState::Running)
                .is_ok()
        );
    }

    #[test]
    fn operation_state_transitions_reject_invalid() {
        // Terminal states cannot transition
        for terminal in [
            OperationState::Completed,
            OperationState::Failed,
            OperationState::Cancelled,
            OperationState::Inconclusive,
        ] {
            for target in [
                OperationState::Queued,
                OperationState::Running,
                OperationState::Paused,
                OperationState::Cancelling,
                OperationState::Completed,
                OperationState::Failed,
                OperationState::Cancelled,
                OperationState::Blocked,
                OperationState::Inconclusive,
            ] {
                assert!(
                    terminal.transition(&target).is_err(),
                    "terminal {terminal} -> {target} must be rejected"
                );
            }
        }
        // Queued cannot go to anything except Running
        assert!(
            OperationState::Queued
                .transition(&OperationState::Completed)
                .is_err()
        );
        assert!(
            OperationState::Queued
                .transition(&OperationState::Failed)
                .is_err()
        );
        assert!(
            OperationState::Queued
                .transition(&OperationState::Paused)
                .is_err()
        );
        assert!(
            OperationState::Queued
                .transition(&OperationState::Cancelling)
                .is_err()
        );
        // Paused cannot go to terminal directly
        assert!(
            OperationState::Paused
                .transition(&OperationState::Completed)
                .is_err()
        );
        assert!(
            OperationState::Paused
                .transition(&OperationState::Failed)
                .is_err()
        );
        // Cancelling can only go to Cancelled
        assert!(
            OperationState::Cancelling
                .transition(&OperationState::Running)
                .is_err()
        );
        assert!(
            OperationState::Cancelling
                .transition(&OperationState::Completed)
                .is_err()
        );
        // Self-transitions are invalid
        for state in [
            OperationState::Queued,
            OperationState::Running,
            OperationState::Paused,
            OperationState::Cancelling,
            OperationState::Completed,
            OperationState::Failed,
            OperationState::Cancelled,
            OperationState::Blocked,
            OperationState::Inconclusive,
        ] {
            assert!(
                state.transition(&state).is_err(),
                "self-transition {state} -> {state} must be rejected"
            );
        }
    }

    #[test]
    fn operation_state_terminal_classification() {
        assert!(!OperationState::Queued.is_terminal());
        assert!(!OperationState::Running.is_terminal());
        assert!(!OperationState::Paused.is_terminal());
        assert!(!OperationState::Cancelling.is_terminal());
        assert!(OperationState::Completed.is_terminal());
        assert!(OperationState::Failed.is_terminal());
        assert!(OperationState::Cancelled.is_terminal());
        assert!(!OperationState::Blocked.is_terminal());
        assert!(OperationState::Inconclusive.is_terminal());
    }

    #[test]
    fn operation_state_kind_strings() {
        assert_eq!(OperationState::Queued.kind(), "Queued");
        assert_eq!(OperationState::Running.kind(), "Running");
        assert_eq!(OperationState::Paused.kind(), "Paused");
        assert_eq!(OperationState::Cancelling.kind(), "Cancelling");
        assert_eq!(OperationState::Completed.kind(), "Completed");
        assert_eq!(OperationState::Failed.kind(), "Failed");
        assert_eq!(OperationState::Cancelled.kind(), "Cancelled");
        assert_eq!(OperationState::Blocked.kind(), "Blocked");
        assert_eq!(OperationState::Inconclusive.kind(), "Inconclusive");
    }

    #[test]
    fn operation_state_serialize_round_trip() {
        for state in [
            OperationState::Queued,
            OperationState::Running,
            OperationState::Paused,
            OperationState::Cancelling,
            OperationState::Completed,
            OperationState::Failed,
            OperationState::Cancelled,
            OperationState::Blocked,
            OperationState::Inconclusive,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: OperationState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }

    #[test]
    fn operation_state_from_str_round_trip() {
        for state in [
            OperationState::Queued,
            OperationState::Running,
            OperationState::Paused,
            OperationState::Cancelling,
            OperationState::Completed,
            OperationState::Failed,
            OperationState::Cancelled,
            OperationState::Blocked,
            OperationState::Inconclusive,
        ] {
            let s = state.kind();
            let parsed = operation_state_from_str(s).unwrap();
            assert_eq!(state, parsed);
        }
        assert!(operation_state_from_str("Unknown").is_err());
    }

    #[test]
    fn operation_state_display() {
        assert_eq!(format!("{}", OperationState::Queued), "Queued");
        assert_eq!(format!("{}", OperationState::Running), "Running");
        assert_eq!(format!("{}", OperationState::Completed), "Completed");
    }

    #[test]
    fn operation_parent_cycle_rejected() {
        // Test that parent-child chains cannot form cycles.
        // Uses validate_no_cycle from the validation module.
        use crate::validation::validate_no_cycle;

        let ids = vec!["op-a", "op-b", "op-c"];
        // a → b → c (no cycle)
        let edges = vec![(0, 1), (1, 2)];
        assert!(validate_no_cycle(&ids, &edges).is_ok());

        // a → b → c → a (cycle)
        let cyclic_edges = vec![(0, 1), (1, 2), (2, 0)];
        assert!(validate_no_cycle(&ids, &cyclic_edges).is_err());
    }
}
