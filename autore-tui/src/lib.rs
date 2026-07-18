pub use autore_core::{Error, Result};
pub use autore_schema::{domain, ids};

pub mod runtime;
pub mod tui;

// Re-export presentation state types for external consumers.
pub use tui::state::{
    DialogState, EventCursor, FilterState, Focus, Navigation, Notification, NotificationLevel,
    OperationViewState, ProjectViewState, TuiState, ValidationStatus,
};
