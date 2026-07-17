//! Dashboard state — read-only snapshot of campaigns, tasks, and claims.
//!
//! The TUI receives a `DashboardState` populated from repository traits
//! (or in-memory stubs for M1). The TUI never mutates this state.

use crate::domain::{Campaign, CampaignState, Claim, ClaimState, Task, TaskState};

/// Top-level dashboard state: a snapshot of all displayable data.
#[derive(Debug, Clone, Default)]
pub struct DashboardState {
    /// All campaigns in the system.
    pub campaigns: Vec<Campaign>,
    /// All tasks across all campaigns.
    pub tasks: Vec<Task>,
    /// All claims across all campaigns.
    pub claims: Vec<Claim>,
    /// Index of the currently selected campaign in `campaigns`.
    pub selected_campaign: usize,
}

impl DashboardState {
    /// Returns the currently selected campaign, if any.
    #[must_use]
    pub fn selected(&self) -> Option<&Campaign> {
        self.campaigns.get(self.selected_campaign)
    }

    /// Returns tasks belonging to the selected campaign.
    #[must_use]
    pub fn selected_tasks(&self) -> Vec<&Task> {
        let Some(campaign) = self.selected() else {
            return Vec::new();
        };
        self.tasks
            .iter()
            .filter(|t| t.campaign_id == campaign.id)
            .collect()
    }

    /// Returns claims belonging to tasks in the selected campaign.
    #[must_use]
    pub fn selected_claims(&self) -> Vec<&Claim> {
        let Some(campaign) = self.selected() else {
            return Vec::new();
        };
        let task_ids: Vec<_> = self
            .tasks
            .iter()
            .filter(|t| t.campaign_id == campaign.id)
            .map(|t| t.id)
            .collect();
        // For M1, claims are not directly linked to campaigns via task IDs
        // in the domain model. Return all claims as a flat summary.
        let _ = task_ids;
        self.claims.iter().collect()
    }

    /// Moves selection to the next campaign (wraps around).
    pub fn select_next(&mut self) {
        if !self.campaigns.is_empty() {
            self.selected_campaign = (self.selected_campaign + 1) % self.campaigns.len();
        }
    }

    /// Moves selection to the previous campaign (wraps around).
    pub fn select_previous(&mut self) {
        if !self.campaigns.is_empty() {
            self.selected_campaign = if self.selected_campaign == 0 {
                self.campaigns.len() - 1
            } else {
                self.selected_campaign - 1
            };
        }
    }
}

/// Summary counts for claims by state.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaimSummary {
    pub proposed: usize,
    pub under_review: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub superseded: usize,
    pub invalidated: usize,
}

impl ClaimSummary {
    /// Computes a summary from a slice of claims.
    #[must_use]
    pub fn from_claims(claims: &[&Claim]) -> Self {
        let mut summary = Self::default();
        for claim in claims {
            match claim.state {
                ClaimState::Proposed => summary.proposed += 1,
                ClaimState::UnderReview => summary.under_review += 1,
                ClaimState::Accepted => summary.accepted += 1,
                ClaimState::Rejected => summary.rejected += 1,
                ClaimState::Superseded => summary.superseded += 1,
                ClaimState::Invalidated => summary.invalidated += 1,
            }
        }
        summary
    }

    /// Total number of claims.
    #[must_use]
    pub fn total(&self) -> usize {
        self.proposed
            + self.under_review
            + self.accepted
            + self.rejected
            + self.superseded
            + self.invalidated
    }

    /// Progress as a fraction (accepted / total), in 0.0..=1.0.
    #[must_use]
    pub fn progress(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.accepted as f64 / total as f64
    }
}

/// Summary counts for tasks by state.
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskSummary {
    pub pending: usize,
    pub ready: usize,
    pub leased: usize,
    pub running: usize,
    pub blocked: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub stale: usize,
}

impl TaskSummary {
    /// Computes a summary from a slice of tasks.
    #[must_use]
    pub fn from_tasks(tasks: &[&Task]) -> Self {
        let mut summary = Self::default();
        for task in tasks {
            match task.state {
                TaskState::Pending => summary.pending += 1,
                TaskState::Ready => summary.ready += 1,
                TaskState::Leased => summary.leased += 1,
                TaskState::Running => summary.running += 1,
                TaskState::Blocked => summary.blocked += 1,
                TaskState::Completed => summary.completed += 1,
                TaskState::Failed => summary.failed += 1,
                TaskState::Cancelled => summary.cancelled += 1,
                TaskState::Stale => summary.stale += 1,
            }
        }
        summary
    }

    /// Total number of tasks.
    #[must_use]
    pub fn total(&self) -> usize {
        self.pending
            + self.ready
            + self.leased
            + self.running
            + self.blocked
            + self.completed
            + self.failed
            + self.cancelled
            + self.stale
    }

    /// Progress as a fraction (completed / total), in 0.0..=1.0.
    #[must_use]
    pub fn progress(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            return 0.0;
        }
        self.completed as f64 / total as f64
    }
}

/// Formats a `CampaignState` for display.
#[must_use]
pub fn format_campaign_state(state: CampaignState) -> &'static str {
    match state {
        CampaignState::Pending => "Pending",
        CampaignState::Active => "Active",
        CampaignState::Paused => "Paused",
        CampaignState::Complete => "Complete",
        CampaignState::Blocked => "Blocked",
    }
}

/// An update event sent from the scheduler (or other producers) to the TUI.
///
/// The TUI applies these updates to its `DashboardState` to reflect
/// changes in campaigns, tasks, and claims without polling repositories.
#[derive(Debug, Clone)]
pub enum TuiUpdate {
    /// A campaign was created or its state changed.
    CampaignUpdated(Campaign),
    /// A task was created or its state changed.
    TaskUpdated(Task),
    /// A new claim was produced.
    ClaimAdded(Claim),
    /// Replace the entire dashboard state (e.g., initial load).
    Snapshot(DashboardState),
}

impl DashboardState {
    /// Applies a `TuiUpdate` to this dashboard state.
    ///
    /// - `CampaignUpdated`: upserts the campaign (replace if exists, append otherwise).
    /// - `TaskUpdated`: upserts the task.
    /// - `ClaimAdded`: appends the claim if not already present.
    /// - `Snapshot`: replaces campaigns, tasks, and claims entirely.
    pub fn apply_update(&mut self, update: TuiUpdate) {
        match update {
            TuiUpdate::CampaignUpdated(campaign) => {
                if let Some(existing) = self.campaigns.iter_mut().find(|c| c.id == campaign.id) {
                    *existing = campaign;
                } else {
                    self.campaigns.push(campaign);
                }
            }
            TuiUpdate::TaskUpdated(task) => {
                if let Some(existing) = self.tasks.iter_mut().find(|t| t.id == task.id) {
                    *existing = task;
                } else {
                    self.tasks.push(task);
                }
            }
            TuiUpdate::ClaimAdded(claim) => {
                if !self.claims.iter().any(|c| c.id == claim.id) {
                    self.claims.push(claim);
                }
            }
            TuiUpdate::Snapshot(snapshot) => {
                self.campaigns = snapshot.campaigns;
                self.tasks = snapshot.tasks;
                self.claims = snapshot.claims;
                // Preserve the caller's selection if still in bounds.
                if self.selected_campaign >= self.campaigns.len() && !self.campaigns.is_empty() {
                    self.selected_campaign = self.campaigns.len() - 1;
                }
            }
        }
    }
}

/// Formats a `TaskState` for display.
#[must_use]
pub fn format_task_state(state: TaskState) -> &'static str {
    match state {
        TaskState::Pending => "Pending",
        TaskState::Ready => "Ready",
        TaskState::Leased => "Leased",
        TaskState::Running => "Running",
        TaskState::Blocked => "Blocked",
        TaskState::Completed => "Completed",
        TaskState::Failed => "Failed",
        TaskState::Cancelled => "Cancelled",
        TaskState::Stale => "Stale",
    }
}
