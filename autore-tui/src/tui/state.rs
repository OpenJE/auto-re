//! Presentation-only TUI state — snapshots loaded via [`AutoReClient`].
//!
//! The TUI never holds a direct storage handle (§3.9 + §23.3). All data
//! access goes through `Box<dyn AutoReClient>`.

use std::collections::HashMap;

use autore_schema::domain::records::{
    Artifact, Contradiction, EvidenceRecord, Hypothesis, Operation, Project, ProjectEvent,
    Provider, ProviderRun, SemanticEntity, VerificationRecord,
};
use autore_schema::domain::{NamespacedId, SchemaVersion, Timestamp};
use autore_schema::ids::{OperationId, ProjectId};

// ---------------------------------------------------------------------------
// TuiState — top-level presentation state
// ---------------------------------------------------------------------------

/// Top-level TUI state: presentation-only snapshots of project data.
///
/// The TUI receives this state populated from queries via `AutoReClient`.
/// It never mutates the database directly.
#[derive(Debug, Clone, Default)]
pub struct TuiState {
    /// Current navigation target.
    pub navigation: Navigation,
    /// Which UI element has keyboard focus.
    pub focus: Focus,
    /// Active text/kind filters.
    pub filters: FilterState,
    /// Modal dialogs stacked on top of the main view.
    pub dialogs: Vec<DialogState>,
    /// Transient notification messages.
    pub notifications: Vec<Notification>,
    /// Per-project data snapshots keyed by project ID.
    pub project_views: HashMap<ProjectId, ProjectViewState>,
    /// Per-operation detail views keyed by operation ID.
    pub operation_views: HashMap<OperationId, OperationViewState>,
    /// Event cursor tracking the last processed sequence.
    pub event_cursor: EventCursor,
    /// Active secondary pane (presentation-only; does not affect authoritative state).
    pub active_pane: Pane,
}

impl TuiState {
    /// Returns the project view for the current navigation target, if any.
    #[must_use]
    pub fn current_project_view(&self) -> Option<&ProjectViewState> {
        match &self.navigation {
            Navigation::Project(id) => self.project_views.get(id),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// Which screen/panel the user is viewing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Navigation {
    /// Project overview / project list.
    #[default]
    Dashboard,
    /// Viewing a specific project.
    Project(ProjectId),
    /// Viewing a specific operation.
    Operation(OperationId),
    /// Raw event stream.
    Events,
    /// Application settings.
    Settings,
}

// ---------------------------------------------------------------------------
// Focus
// ---------------------------------------------------------------------------

/// Which UI element currently has keyboard focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Focus {
    /// First (left) panel — project summary.
    #[default]
    Panel1,
    /// Second (top-right) panel — operations table.
    Panel2,
    /// Third (bottom-right) panel — hypotheses / evidence.
    Panel3,
    /// Sidebar / tab bar.
    Sidebar,
    /// Active modal dialog.
    Dialog,
}

// ---------------------------------------------------------------------------
// Pane — secondary tab selection
// ---------------------------------------------------------------------------

/// Secondary pane/tab shown in the dashboard's right column.
///
/// The dashboard preserves the 4-panel physical layout (Panel 1 left 30%,
/// Panel 2 top-right, Panel 3 bottom-right, with Panel 4 as the active
/// secondary pane overlaid on top or switched in place of Panel 2/3 based
/// on focus). The active pane is a presentation-only cursor and does not
/// affect authoritative state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pane {
    /// Primary dashboard — projects/operations/hypotheses.
    #[default]
    Dashboard,
    /// Providers and provider runs.
    Providers,
    /// Native artifacts (stage-0 native provider outputs).
    NativeArtifacts,
    /// Selected operation's detail view (progress, cancellation).
    OperationsDetail,
    /// Project events log.
    EventsLog,
    /// Migration history.
    MigrationHistory,
    /// External artifact integrity checks.
    ExternalArtifactIntegrity,
}

// ---------------------------------------------------------------------------
// FilterState
// ---------------------------------------------------------------------------

/// Active text and kind filters for list views.
#[derive(Debug, Clone, Default)]
pub struct FilterState {
    /// Free-text search query (empty = no filter).
    pub text_search: String,
    /// Optional kind filter for entities/artifacts.
    pub kind_filter: Option<NamespacedId>,
}

// ---------------------------------------------------------------------------
// DialogState
// ---------------------------------------------------------------------------

/// Modal dialog variants.
#[derive(Debug, Clone)]
pub enum DialogState {
    /// Error dialog with a message.
    Error(String),
    /// Confirmation dialog with a message and pending callback.
    Confirm { message: String },
    /// Text input dialog with a prompt and current buffer.
    Input { prompt: String, buffer: String },
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// Severity level for transient notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
}

/// A transient notification message.
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub level: NotificationLevel,
    pub created_at: Timestamp,
}

// ---------------------------------------------------------------------------
// ProjectViewState
// ---------------------------------------------------------------------------

/// Presentation snapshot for a single project.
///
/// All fields are option/embedded snapshots loaded from queries — never
/// authoritative over the database.
#[derive(Debug, Clone, Default)]
pub struct ProjectViewState {
    /// Project summary (None if not yet loaded).
    pub project_summary: Option<Project>,
    /// Artifacts in the project.
    pub artifacts: Vec<Artifact>,
    /// Semantic entities in the project.
    pub entities: Vec<SemanticEntity>,
    /// Providers registered for the project.
    pub providers: Vec<Provider>,
    /// Provider runs in the project.
    pub runs: Vec<ProviderRun>,
    /// Evidence records in the project.
    pub evidence: Vec<EvidenceRecord>,
    /// Hypotheses in the project.
    pub hypotheses: Vec<Hypothesis>,
    /// Contradictions in the project.
    pub contradictions: Vec<Contradiction>,
    /// Verification records in the project.
    pub verification: Vec<VerificationRecord>,
    /// Operations in the project.
    pub operations: Vec<Operation>,
    /// Recent project events (newest first).
    pub recent_events: Vec<ProjectEvent>,
    /// Schema version of the project (if loaded).
    pub schema_version: Option<SchemaVersion>,
    /// Validation status (if checked).
    pub validation_status: Option<ValidationStatus>,
}

// ---------------------------------------------------------------------------
// EventCursor
// ---------------------------------------------------------------------------

/// Tracks the TUI's position in the project event stream (§23.5).
#[derive(Debug, Clone, Default)]
pub struct EventCursor {
    /// Last processed event sequence number.
    pub last_sequence: u64,
    /// Whether the event stream subscription is active.
    pub connected: bool,
    /// Whether any events were missed (gap detected).
    pub missed_events: bool,
}

// ---------------------------------------------------------------------------
// OperationViewState
// ---------------------------------------------------------------------------

/// Presentation snapshot for a single operation's detail view.
#[derive(Debug, Clone, Default)]
pub struct OperationViewState {
    /// The operation record (None if not yet loaded).
    pub operation: Option<Operation>,
    /// Progress updates for the operation.
    pub progress: Vec<autore_schema::domain::records::ProgressUpdate>,
    /// Cancellation requests for the operation.
    pub cancellation_requests: Vec<autore_schema::domain::records::CancellationRequest>,
}

// ---------------------------------------------------------------------------
// ValidationStatus
// ---------------------------------------------------------------------------

/// Result of a project validation check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationStatus {
    /// Validation passed with no issues.
    Ok,
    /// Validation passed with warnings.
    Warnings(Vec<String>),
    /// Validation failed with errors.
    Failed(Vec<String>),
}
