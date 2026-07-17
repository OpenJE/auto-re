//! Runtime orchestration — scheduler and TUI as concurrent tokio tasks.
//!
//! The runtime creates a bounded channel for `TuiUpdate` events, spawns
//! the scheduler loop and TUI as separate tokio tasks, and coordinates
//! graceful shutdown when the TUI exits.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::domain::{Campaign, CampaignState, Task, TaskKind, TaskState};
use crate::ids::{CampaignId, TaskId};
use crate::tui::state::{DashboardState, TuiUpdate};

/// Default capacity for the TUI update channel.
const CHANNEL_CAPACITY: usize = 256;

/// Default scheduler tick interval.
const DEFAULT_TICK_INTERVAL: Duration = Duration::from_millis(100);

/// Runs the application: scheduler loop + TUI as concurrent tasks.
///
/// Creates a bounded channel for `TuiUpdate` events, spawns the scheduler
/// and TUI as separate tokio tasks, and waits for the TUI to exit.
pub async fn run() -> crate::Result<()> {
    run_with_tick_interval(DEFAULT_TICK_INTERVAL).await
}

/// Runs the application with a configurable scheduler tick interval.
///
/// Exposed for testing — production code calls `run()`.
pub async fn run_with_tick_interval(tick_interval: Duration) -> crate::Result<()> {
    let (sender, receiver) = mpsc::channel::<TuiUpdate>(CHANNEL_CAPACITY);

    let scheduler_handle = tokio::spawn(scheduler_loop(sender, tick_interval));
    let tui_handle = tokio::spawn(crate::tui::run_tui(Some(receiver)));

    let tui_result = tui_handle
        .await
        .map_err(|e| crate::Error::Operation(format!("TUI task panicked: {e}")))?;

    scheduler_handle.abort();
    tui_result
}

/// Scheduler loop that ticks periodically and sends updates to the TUI.
///
/// For M1, this creates a mock campaign with tasks and simulates progress.
/// Future iterations will use real repositories and the scheduler's
/// `run_campaign` method.
async fn scheduler_loop(sender: mpsc::Sender<TuiUpdate>, tick_interval: Duration) {
    let campaign_id = CampaignId::new();
    let mut campaign = Campaign::new(campaign_id, "M1 Smoke Test");
    campaign.state = CampaignState::Active;

    let mut tasks: Vec<Task> = (0..3)
        .map(|i| {
            let mut task = Task::new(
                TaskId::new(),
                campaign_id,
                TaskKind::AnalyzeFunction,
                crate::domain::TaskSubject::Binary,
                crate::domain::TaskPriority::new(100),
                crate::domain::RequiredCapabilities::new(false, true, false, false),
                None,
                None,
                3,
            );
            task.state = if i == 0 {
                TaskState::Ready
            } else {
                TaskState::Pending
            };
            task
        })
        .collect();

    let initial_state = DashboardState {
        campaigns: vec![campaign.clone()],
        tasks: tasks.clone(),
        claims: vec![],
        selected_campaign: 0,
    };

    if sender
        .send(TuiUpdate::Snapshot(initial_state))
        .await
        .is_err()
    {
        return;
    }

    let mut tick_count = 0u32;
    let mut interval = tokio::time::interval(tick_interval);

    loop {
        interval.tick().await;
        tick_count += 1;

        if let Some(task) = tasks.iter_mut().find(|t| t.state == TaskState::Ready) {
            task.state = TaskState::Running;
            if sender
                .send(TuiUpdate::TaskUpdated(task.clone()))
                .await
                .is_err()
            {
                return;
            }
        } else if let Some(task) = tasks.iter_mut().find(|t| t.state == TaskState::Running) {
            task.state = TaskState::Completed;
            if sender
                .send(TuiUpdate::TaskUpdated(task.clone()))
                .await
                .is_err()
            {
                return;
            }

            if let Some(next) = tasks.iter_mut().find(|t| t.state == TaskState::Pending) {
                next.state = TaskState::Ready;
                if sender
                    .send(TuiUpdate::TaskUpdated(next.clone()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }

        if tick_count % 10 == 0 {
            campaign.state = if tasks.iter().all(|t| t.state == TaskState::Completed) {
                CampaignState::Complete
            } else {
                CampaignState::Active
            };
            if sender
                .send(TuiUpdate::CampaignUpdated(campaign.clone()))
                .await
                .is_err()
            {
                return;
            }
        }

        if tasks.iter().all(|t| t.state == TaskState::Completed) {
            campaign.state = CampaignState::Complete;
            let _ = sender
                .send(TuiUpdate::CampaignUpdated(campaign.clone()))
                .await;
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Claim, ClaimPredicate, ClaimState, ClaimValue, Confidence, EntityId, Provenance,
    };
    use crate::ids::{ClaimId, FunctionId};

    #[tokio::test]
    async fn tui_updates_on_task_state_change() {
        let (sender, mut receiver) = mpsc::channel::<TuiUpdate>(16);
        let mut state = DashboardState::default();

        let campaign_id = CampaignId::new();
        let campaign = Campaign::new(campaign_id, "test");
        state.apply_update(TuiUpdate::CampaignUpdated(campaign));

        let mut task = Task::new(
            TaskId::new(),
            campaign_id,
            TaskKind::AnalyzeFunction,
            crate::domain::TaskSubject::Binary,
            crate::domain::TaskPriority::new(100),
            crate::domain::RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        task.state = TaskState::Ready;
        state.apply_update(TuiUpdate::TaskUpdated(task.clone()));

        assert_eq!(state.tasks.len(), 1);
        assert_eq!(state.tasks[0].state, TaskState::Ready);

        task.state = TaskState::Running;
        sender
            .send(TuiUpdate::TaskUpdated(task.clone()))
            .await
            .unwrap();

        if let Some(update) = receiver.recv().await {
            state.apply_update(update);
        }

        assert_eq!(state.tasks[0].state, TaskState::Running);
    }

    #[tokio::test]
    async fn tui_updates_on_new_claim() {
        let (sender, mut receiver) = mpsc::channel::<TuiUpdate>(16);
        let mut state = DashboardState::default();

        assert_eq!(state.claims.len(), 0);

        let claim = Claim::new(
            ClaimId::new(),
            EntityId::Function(FunctionId::new()),
            ClaimPredicate::FunctionName,
            ClaimValue::String("test_fn".into()),
            Confidence::new(0.9).unwrap(),
            Provenance::StaticAnalysis,
        );
        sender
            .send(TuiUpdate::ClaimAdded(claim.clone()))
            .await
            .unwrap();

        if let Some(update) = receiver.recv().await {
            state.apply_update(update);
        }

        assert_eq!(state.claims.len(), 1);
        assert_eq!(state.claims[0].state, ClaimState::Proposed);
    }

    #[tokio::test]
    async fn tui_does_not_block_scheduler() {
        let (sender, receiver) = mpsc::channel::<TuiUpdate>(2);
        let mut task = Task::new(
            TaskId::new(),
            CampaignId::new(),
            TaskKind::AnalyzeFunction,
            crate::domain::TaskSubject::Binary,
            crate::domain::TaskPriority::new(100),
            crate::domain::RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        task.state = TaskState::Ready;

        let mut task_clone = task.clone();
        let scheduler_handle = tokio::spawn(async move {
            for i in 0..100 {
                task_clone.state = if i % 2 == 0 {
                    TaskState::Running
                } else {
                    TaskState::Ready
                };
                if sender
                    .send(TuiUpdate::TaskUpdated(task_clone.clone()))
                    .await
                    .is_err()
                {
                    return i;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            100
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(receiver);

        let iterations_completed = scheduler_handle.await.unwrap();
        assert!(
            iterations_completed < 100,
            "scheduler should stop when receiver is dropped, but completed {iterations_completed} iterations"
        );
    }
}
