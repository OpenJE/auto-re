use crossterm::event::{KeyCode, MouseEventKind};

/// Enum representing different types of events in the TUI system.
#[derive(Debug, Clone)]
pub enum Event {
    Render,
    KeyDown(KeyCode),
    MouseClick(MouseEventKind),
    WidgetStateChanged,
}

impl Event {
    pub fn render() -> Self {
        Self::Render
    }

    pub fn key_down(key: KeyCode) -> Self {
        Self::KeyDown(key)
    }

    pub fn mouse_click(kind: MouseEventKind) -> Self {
        Self::MouseClick(kind)
    }

    pub fn widget_state_changed() -> Self {
        Self::WidgetStateChanged
    }
}

pub mod operation_events {
    use autore_core::operation::OperationState;
    use autore_schema::domain::records::{
        EVENT_KIND_OPERATION_COMPLETED, EVENT_KIND_OPERATION_FAILED,
        EVENT_KIND_OPERATION_PROGRESS, EVENT_KIND_OPERATION_QUEUED,
        EVENT_KIND_OPERATION_STARTED,
    };
    use autore_schema::domain::NamespacedId;

    /// Returns the event kind for an operation state transition.
    ///
    /// Uses the canonical `NamespacedId` constants from `records.rs`.
    pub fn transition_event_kind(target: &OperationState) -> &'static NamespacedId {
        match target {
            OperationState::Queued => &EVENT_KIND_OPERATION_QUEUED,
            OperationState::Running => &EVENT_KIND_OPERATION_STARTED,
            OperationState::Paused => &EVENT_KIND_OPERATION_PROGRESS,
            OperationState::Cancelling => &EVENT_KIND_OPERATION_PROGRESS,
            OperationState::Completed => &EVENT_KIND_OPERATION_COMPLETED,
            OperationState::Failed => &EVENT_KIND_OPERATION_FAILED,
            OperationState::Cancelled => &EVENT_KIND_OPERATION_COMPLETED,
            OperationState::Blocked => &EVENT_KIND_OPERATION_PROGRESS,
            OperationState::Inconclusive => &EVENT_KIND_OPERATION_FAILED,
        }
    }

    /// Returns the event kind string for an operation state transition.
    ///
    /// Format: `core.operation.<state>` where `<state>` is the lowercase
    /// discriminant of the target state.
    pub fn transition_event_kind_str(target: &OperationState) -> &'static str {
        match target {
            OperationState::Queued => "core.operation.queued",
            OperationState::Running => "core.operation.started",
            OperationState::Paused => "core.operation.paused",
            OperationState::Cancelling => "core.operation.cancelling",
            OperationState::Completed => "core.operation.completed",
            OperationState::Failed => "core.operation.failed",
            OperationState::Cancelled => "core.operation.cancelled",
            OperationState::Blocked => "core.operation.blocked",
            OperationState::Inconclusive => "core.operation.inconclusive",
        }
    }

    /// Validates that a state transition is legal and returns the event
    /// kind that would be emitted.
    pub fn emit_transition_event(
        current: &OperationState,
        target: &OperationState,
    ) -> autore_core::Result<&'static NamespacedId> {
        current.transition(target)?;
        Ok(transition_event_kind(target))
    }
}

#[cfg(test)]
mod tests {
    use super::operation_events::*;
    use autore_core::operation::OperationState;

    #[test]
    fn operation_events_emitted_for_transitions() {
        let kind = emit_transition_event(&OperationState::Queued, &OperationState::Running)
            .unwrap();
        assert_eq!(kind.to_string(), "core.operation.started");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Completed)
            .unwrap();
        assert_eq!(kind.to_string(), "core.operation.completed");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Failed)
            .unwrap();
        assert_eq!(kind.to_string(), "core.operation.failed");

        let result = emit_transition_event(&OperationState::Completed, &OperationState::Running);
        assert!(result.is_err(), "invalid transition must not emit event");
    }

    #[test]
    fn operation_event_kind_str_matches_constants() {
        assert_eq!(transition_event_kind_str(&OperationState::Queued), "core.operation.queued");
        assert_eq!(transition_event_kind_str(&OperationState::Running), "core.operation.started");
        assert_eq!(transition_event_kind_str(&OperationState::Completed), "core.operation.completed");
        assert_eq!(transition_event_kind_str(&OperationState::Failed), "core.operation.failed");
    }

    #[test]
    fn operation_event_queued_to_running_to_completed() {
        let mut state = OperationState::Queued;

        let k1 = emit_transition_event(&state, &OperationState::Running).unwrap();
        assert_eq!(k1.to_string(), "core.operation.started");
        state = OperationState::Running;

        let k2 = emit_transition_event(&state, &OperationState::Completed).unwrap();
        assert_eq!(k2.to_string(), "core.operation.completed");
    }
}

