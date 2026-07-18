pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod runtime;
pub mod tui;

// Re-export presentation state types for external consumers.
pub use tui::state::{
    DialogState, EventCursor, FilterState, Focus, Navigation, Notification, NotificationLevel,
    OperationViewState, Pane, ProjectViewState, TuiState, ValidationStatus,
};

// Re-export event loop types for external consumers.
pub use tui::{InternalTuiEvent, LoopAction, TerminalEvent, TuiEvent, TuiEventLoop};
