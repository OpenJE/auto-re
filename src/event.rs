use crossterm::event::{KeyCode, MouseEventKind};

/// Enum representing different types of events in the TUI system.
#[derive(Debug, Clone)]
pub enum Event {
    /// Trigger a full render of the UI.
    Render,

    /// A key was pressed.
    KeyDown(KeyCode),

    /// A mouse click occurred.
    MouseClick(MouseEventKind),

    /// The state of a widget has changed (e.g., value, visibility, focus).
    WidgetStateChanged,
}

impl Event {
    /// Creates a new Render event.
    pub fn render() -> Self {
        Self::Render
    }

    /// Creates a new KeyDown event with the given key code.
    pub fn key_down(key: KeyCode) -> Self {
        Self::KeyDown(key)
    }

    /// Creates a new MouseClick event with the given mouse event kind.
    pub fn mouse_click(kind: MouseEventKind) -> Self {
        Self::MouseClick(kind)
    }

    /// Creates a new WidgetStateChanged event.
    pub fn widget_state_changed() -> Self {
        Self::WidgetStateChanged
    }
}
