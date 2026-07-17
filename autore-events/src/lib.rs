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

    /// Returns the event kind string for an operation state transition.
    ///
    /// Format: `core.operation.<state>` where `<state>` is the lowercase
    /// discriminant of the target state.
    pub fn transition_event_kind(target: &OperationState) -> &'static str {
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

    /// Simulates event emission for a state transition.
    ///
    /// Returns the event kind string that would be emitted.
    /// Task 21 will wire this to `ProjectEvent` for actual emission.
    pub fn emit_transition_event(
        current: &OperationState,
        target: &OperationState,
    ) -> autore_core::Result<&'static str> {
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
        assert_eq!(kind, "core.operation.started");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Completed)
            .unwrap();
        assert_eq!(kind, "core.operation.completed");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Paused)
            .unwrap();
        assert_eq!(kind, "core.operation.paused");

        let kind = emit_transition_event(&OperationState::Paused, &OperationState::Running)
            .unwrap();
        assert_eq!(kind, "core.operation.started");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Cancelling)
            .unwrap();
        assert_eq!(kind, "core.operation.cancelling");

        let kind = emit_transition_event(&OperationState::Cancelling, &OperationState::Cancelled)
            .unwrap();
        assert_eq!(kind, "core.operation.cancelled");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Failed)
            .unwrap();
        assert_eq!(kind, "core.operation.failed");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Blocked)
            .unwrap();
        assert_eq!(kind, "core.operation.blocked");

        let kind = emit_transition_event(&OperationState::Running, &OperationState::Inconclusive)
            .unwrap();
        assert_eq!(kind, "core.operation.inconclusive");

        let kind = emit_transition_event(&OperationState::Blocked, &OperationState::Running)
            .unwrap();
        assert_eq!(kind, "core.operation.started");

        let result = emit_transition_event(&OperationState::Completed, &OperationState::Running);
        assert!(result.is_err(), "invalid transition must not emit event");
    }

    #[test]
    fn operation_event_queued_to_running_to_completed() {
        let mut state = OperationState::Queued;

        let k1 = emit_transition_event(&state, &OperationState::Running).unwrap();
        assert_eq!(k1, "core.operation.started");
        state = OperationState::Running;

        let k2 = emit_transition_event(&state, &OperationState::Completed).unwrap();
        assert_eq!(k2, "core.operation.completed");
    }
}

