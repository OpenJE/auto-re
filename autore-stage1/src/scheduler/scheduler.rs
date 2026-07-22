//! Scheduler — deterministic priority scoring and model-routed dispatch.
//!
//! The scheduler computes a stable, deterministic priority score for each
//! task using configurable `PriorityFactors` and a caller-supplied
//! `PriorityContext`. The same inputs always produce the same score — no
//! randomness, no platform-dependent hashing.
//!
//! The `run_campaign` method performs one evaluation tick via
//! `ApplicationCommand` / `ApplicationQuery` through an `AutoReClient`:
//! recovering expired leases, promoting pending tasks, dispatching ready
//! tasks, and evaluating the campaign state.

use std::sync::Arc;

use time::OffsetDateTime;

use autore_app::application_service::requests::{
    ApplicationCommand, ApplicationQuery, AutoReClient, FailWorkItemRequest, LeaseWorkItemRequest,
    ListExpiredLeasesQuery, PromoteWorkItemRequest, QueryResult, RequeueWorkItemRequest,
};

use crate::domain::task::{Task, TaskKind, TaskState};
use crate::ids::{CampaignId, ProjectId, TaskId};
use crate::model::router::ModelRouter;

// ---------------------------------------------------------------------------
// PriorityFactors
// ---------------------------------------------------------------------------

/// Configurable weights for the priority score formula.
///
/// Each weight is applied to a specific task attribute or context indicator.
/// The final score is a deterministic `u64` derived from integer-scaled
/// floating-point arithmetic.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PriorityFactors {
    /// Base priority added to every task.
    pub base_priority: u32,
    /// Weight multiplied by `task.attempt_count` — higher attempts boost priority.
    pub attempt_count_weight: f64,
    /// Weight multiplied by `task.dependencies.len()` — deeper dependency chains
    /// get a scheduling boost.
    pub dependency_depth_weight: f64,
    /// Weight applied to the task's intrinsic `TaskPriority` score.
    pub deadline_weight: f64,
    /// Weight applied to a binary verification indicator (1.0 for verification
    /// tasks, 0.0 otherwise).
    pub verification_weight: f64,
    /// Weight multiplied by `ctx.dependents_unblocked` — tasks that unblock
    /// many downstream dependents get a scheduling bonus (§7.4).
    pub dependents_unblocked_weight: f64,
    /// Weight applied to a binary `ctx.high_impact_conflict` indicator.
    pub high_impact_conflict_weight: f64,
    /// Weight applied to a binary `ctx.removes_build_blocker` indicator.
    pub removes_build_blocker_weight: f64,
    /// Weight multiplied by `ctx.verified_coverage` (0.0–1.0).
    pub verified_coverage_weight: f64,
    /// Weight multiplied by `ctx.evidence_strength` (0.0–1.0).
    pub evidence_strength_weight: f64,
}

impl Default for PriorityFactors {
    fn default() -> Self {
        Self {
            base_priority: 100,
            attempt_count_weight: 10.0,
            dependency_depth_weight: 5.0,
            deadline_weight: 1.0,
            verification_weight: 20.0,
            dependents_unblocked_weight: 15.0,
            high_impact_conflict_weight: 25.0,
            removes_build_blocker_weight: 30.0,
            verified_coverage_weight: 10.0,
            evidence_strength_weight: 5.0,
        }
    }
}

// ---------------------------------------------------------------------------
// PriorityContext
// ---------------------------------------------------------------------------

/// Caller-supplied scoring inputs for the expanded priority factors (§7.4).
///
/// All fields default to zero/false, which makes the new bonus terms
/// contribute nothing — preserving backward-compatible scores when the
/// caller does not supply context.
#[derive(Debug, Clone, PartialEq)]
pub struct PriorityContext {
    /// Number of downstream dependents unblocked if this task completes.
    pub dependents_unblocked: u32,
    /// Whether this task resolves a high-impact conflict record.
    pub high_impact_conflict: bool,
    /// Whether this task removes a build blocker.
    pub removes_build_blocker: bool,
    /// Verified coverage contribution (0.0–1.0).
    pub verified_coverage: f64,
    /// Evidence strength indicator (0.0–1.0).
    pub evidence_strength: f64,
}

impl Default for PriorityContext {
    fn default() -> Self {
        Self {
            dependents_unblocked: 0,
            high_impact_conflict: false,
            removes_build_blocker: false,
            verified_coverage: 0.0,
            evidence_strength: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// CampaignEvaluation
// ---------------------------------------------------------------------------

/// Result of evaluating a campaign's current state after one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CampaignEvaluation {
    /// All tasks are in terminal states (Completed, Cancelled, Failed).
    Complete,
    /// Non-terminal tasks exist but none can proceed.
    Blocked,
    /// No ready tasks to dispatch; work is in progress (leased/running).
    Idle,
    /// Ready tasks available or tasks were dispatched this tick.
    Active,
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// Computes deterministic priority scores and routes tasks to models.
pub struct Scheduler {
    router: ModelRouter,
    max_dispatch_per_tick: usize,
}

impl Scheduler {
    /// Creates a new scheduler with the given model router.
    pub fn new(router: ModelRouter) -> Self {
        Self {
            router,
            max_dispatch_per_tick: 4,
        }
    }

    /// Sets the maximum number of tasks to dispatch per evaluation tick.
    pub fn with_max_dispatch(mut self, max: usize) -> Self {
        self.max_dispatch_per_tick = max;
        self
    }

    /// Returns a reference to the internal model router.
    pub fn router(&self) -> &ModelRouter {
        &self.router
    }

    /// Computes a deterministic priority score for a task.
    ///
    /// The formula is:
    /// ```text
    /// score = base_priority
    ///       + attempt_count * attempt_count_weight
    ///       + dependency_count * dependency_depth_weight
    ///       + task.priority.score() * deadline_weight
    ///       + verification_indicator * verification_weight
    ///       + ctx.dependents_unblocked * dependents_unblocked_weight
    ///       + ctx.high_impact_conflict * high_impact_conflict_weight
    ///       + ctx.removes_build_blocker * removes_build_blocker_weight
    ///       + ctx.verified_coverage * verified_coverage_weight
    ///       + ctx.evidence_strength * evidence_strength_weight
    /// ```
    ///
    /// The result is truncated to `u64`. The same inputs always produce the
    /// same output — no randomness or platform-dependent behavior.
    pub fn priority_score(
        task: &Task,
        factors: &PriorityFactors,
        ctx: &PriorityContext,
        _now: OffsetDateTime,
    ) -> u64 {
        let base = factors.base_priority as f64;
        let attempt_bonus = task.attempt_count as f64 * factors.attempt_count_weight;
        let dep_bonus = task.dependencies.len() as f64 * factors.dependency_depth_weight;
        let priority_bonus = task.priority.score() as f64 * factors.deadline_weight;
        let verification_indicator = if is_verification_task(&task.kind) {
            1.0
        } else {
            0.0
        };
        let verification_bonus = verification_indicator * factors.verification_weight;

        let unblocked_bonus = ctx.dependents_unblocked as f64 * factors.dependents_unblocked_weight;
        let conflict_indicator = if ctx.high_impact_conflict { 1.0 } else { 0.0 };
        let conflict_bonus = conflict_indicator * factors.high_impact_conflict_weight;
        let blocker_indicator = if ctx.removes_build_blocker { 1.0 } else { 0.0 };
        let blocker_bonus = blocker_indicator * factors.removes_build_blocker_weight;
        let coverage_bonus = ctx.verified_coverage * factors.verified_coverage_weight;
        let evidence_bonus = ctx.evidence_strength * factors.evidence_strength_weight;

        let total = base
            + attempt_bonus
            + dep_bonus
            + priority_bonus
            + verification_bonus
            + unblocked_bonus
            + conflict_bonus
            + blocker_bonus
            + coverage_bonus
            + evidence_bonus;
        total.max(0.0) as u64
    }

    /// Computes priority scores for a slice of tasks, returning them in the
    /// same order.
    pub fn score_tasks(
        &self,
        tasks: &[Task],
        factors: &PriorityFactors,
        ctx: &PriorityContext,
        now: OffsetDateTime,
    ) -> Vec<u64> {
        tasks
            .iter()
            .map(|t| Self::priority_score(t, factors, ctx, now))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Campaign evaluation loop
    // -----------------------------------------------------------------------

    /// Performs one campaign evaluation tick:
    ///
    /// 1. Recover expired leases via `FailWorkItem` / `RequeueWorkItem`.
    /// 2. Promote Pending tasks with satisfied dependencies via `PromoteWorkItem`.
    /// 3. Invalidate stale work (M1: no-op).
    /// 4. Lease and dispatch up to `max_dispatch_per_tick` ready tasks via `LeaseWorkItem`.
    /// 5. Evaluate state and set `CampaignEvaluation`.
    ///
    /// All mutations route through `AutoReClient`; no direct storage access.
    pub fn run_campaign(
        &self,
        _campaign_id: CampaignId,
        evaluation: &mut CampaignEvaluation,
        client: Arc<dyn AutoReClient>,
        project_id: ProjectId,
        tasks: &[Task],
        now: OffsetDateTime,
    ) -> crate::Result<()> {
        self.recover_expired_leases(&*client, project_id, tasks)?;
        self.promote_ready_tasks(&*client, project_id, tasks)?;
        // Step 3: invalidation is a no-op for M1.
        let dispatched = self.dispatch_tasks(&*client, project_id, tasks, now)?;
        *evaluation = Self::evaluate_state(tasks, dispatched);
        Ok(())
    }

    /// Convenience method: runs one tick and returns the evaluation.
    pub fn evaluate(
        &self,
        campaign_id: CampaignId,
        client: Arc<dyn AutoReClient>,
        project_id: ProjectId,
        tasks: &[Task],
        now: OffsetDateTime,
    ) -> crate::Result<CampaignEvaluation> {
        let mut eval = CampaignEvaluation::Idle;
        self.run_campaign(campaign_id, &mut eval, client, project_id, tasks, now)?;
        Ok(eval)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Recovers expired leases: issues `FailWorkItem` for exhausted tasks or
    /// `RequeueWorkItem` for retryable ones.
    fn recover_expired_leases(
        &self,
        client: &dyn AutoReClient,
        project_id: ProjectId,
        tasks: &[Task],
    ) -> crate::Result<()> {
        let result = client.query(ApplicationQuery::ListExpiredLeases(
            ListExpiredLeasesQuery {
                project: project_id,
            },
        ))?;
        let expired_ids = match result {
            QueryResult::ExpiredLeases(resp) => resp.expired,
            _ => {
                return Err(crate::Error::Core(autore_core::Error::Validation(
                    "unexpected query result for ListExpiredLeases".into(),
                )));
            }
        };
        if expired_ids.is_empty() {
            return Ok(());
        }
        for expired_id_str in &expired_ids {
            let Some(task) = tasks.iter().find(|t| t.id.to_string() == *expired_id_str) else {
                continue;
            };
            if task.attempt_count >= task.maximum_attempts {
                client.execute(ApplicationCommand::FailWorkItem(FailWorkItemRequest {
                    project: project_id,
                    work_item_id: expired_id_str.clone(),
                    reason: "lease expired, max attempts exceeded".into(),
                }))?;
            } else {
                client.execute(ApplicationCommand::RequeueWorkItem(
                    RequeueWorkItemRequest {
                        project: project_id,
                        work_item_id: expired_id_str.clone(),
                    },
                ))?;
            }
        }
        Ok(())
    }

    /// Promotes Pending tasks to Ready when all dependencies are Completed,
    /// issuing `PromoteWorkItem` for each.
    fn promote_ready_tasks(
        &self,
        client: &dyn AutoReClient,
        project_id: ProjectId,
        tasks: &[Task],
    ) -> crate::Result<()> {
        let completed_ids: Vec<TaskId> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Completed)
            .map(|t| t.id)
            .collect();
        for task in tasks {
            if task.state == TaskState::Pending && task.dependencies_satisfied(&completed_ids) {
                client.execute(ApplicationCommand::PromoteWorkItem(
                    PromoteWorkItemRequest {
                        project: project_id,
                        work_item_id: task.id.to_string(),
                    },
                ))?;
            }
        }
        Ok(())
    }

    /// Leases up to `max_dispatch_per_tick` ready tasks, sorted by stored
    /// priority (descending), issuing `LeaseWorkItem` for each.
    fn dispatch_tasks(
        &self,
        client: &dyn AutoReClient,
        project_id: ProjectId,
        tasks: &[Task],
        _now: OffsetDateTime,
    ) -> crate::Result<usize> {
        let completed_ids: Vec<TaskId> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Completed)
            .map(|t| t.id)
            .collect();

        let mut dispatchable: Vec<&Task> = tasks
            .iter()
            .filter(|t| {
                t.dependencies_satisfied(&completed_ids)
                    && matches!(t.state, TaskState::Ready | TaskState::Pending)
            })
            .collect();

        dispatchable.sort_by_key(|t| std::cmp::Reverse(t.priority.score()));

        let mut dispatched = 0;
        for task in dispatchable.into_iter().take(self.max_dispatch_per_tick) {
            let worker_id = uuid::Uuid::new_v4().to_string();
            client.execute(ApplicationCommand::LeaseWorkItem(LeaseWorkItemRequest {
                project: project_id,
                work_item_id: task.id.to_string(),
                worker_id,
            }))?;
            dispatched += 1;
        }
        Ok(dispatched)
    }

    /// Evaluates the campaign state from a task snapshot and dispatch count.
    fn evaluate_state(tasks: &[Task], dispatched: usize) -> CampaignEvaluation {
        if tasks.is_empty() {
            return CampaignEvaluation::Idle;
        }
        if tasks.iter().all(|t| t.state.is_terminal()) {
            return CampaignEvaluation::Complete;
        }
        if dispatched > 0 {
            return CampaignEvaluation::Active;
        }
        if tasks.iter().any(|t| t.state == TaskState::Ready) {
            return CampaignEvaluation::Active;
        }
        if tasks
            .iter()
            .any(|t| matches!(t.state, TaskState::Leased | TaskState::Running))
        {
            return CampaignEvaluation::Idle;
        }
        CampaignEvaluation::Blocked
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns `true` if the task kind is a verification-related operation.
fn is_verification_task(kind: &TaskKind) -> bool {
    matches!(
        kind,
        TaskKind::VerifyClaim
            | TaskKind::VerifyClaimSet
            | TaskKind::GenerateImplementationContract
            | TaskKind::ValidateImplementationContract
            | TaskKind::ValidateReimplementation
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::task::{RequiredCapabilities, TaskPriority, TaskSubject};
    use crate::ids::{CampaignId, ProjectId, TaskId};
    use crate::model::{ModelCapabilities, ModelClass, ModelDescriptor};
    use autore_app::application_service::requests::{
        AddEvidenceResponse, CommandResult, ExpiredLeasesResponse, WorkItemsResponse,
    };
    use autore_schema::domain::records::ProjectEvent;
    use std::sync::Mutex;

    // -- RecordingAutoReClient --

    struct RecordingAutoReClient {
        commands: Mutex<Vec<ApplicationCommand>>,
        queries: Mutex<Vec<ApplicationQuery>>,
        expired_leases: Mutex<Vec<String>>,
    }

    impl RecordingAutoReClient {
        fn new() -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                queries: Mutex::new(Vec::new()),
                expired_leases: Mutex::new(Vec::new()),
            }
        }

        fn with_expired_leases(expired: Vec<String>) -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                queries: Mutex::new(Vec::new()),
                expired_leases: Mutex::new(expired),
            }
        }

        fn recorded_commands(&self) -> Vec<ApplicationCommand> {
            self.commands.lock().unwrap().clone()
        }

        fn recorded_queries(&self) -> Vec<ApplicationQuery> {
            self.queries.lock().unwrap().clone()
        }
    }

    impl AutoReClient for RecordingAutoReClient {
        fn execute(&self, command: ApplicationCommand) -> autore_core::Result<CommandResult> {
            self.commands.lock().unwrap().push(command);
            Ok(CommandResult::EvidenceAdded(AddEvidenceResponse {
                id: autore_schema::ids::EvidenceRecordId::new(),
            }))
        }

        fn query(&self, query: ApplicationQuery) -> autore_core::Result<QueryResult> {
            self.queries.lock().unwrap().push(query.clone());
            match query {
                ApplicationQuery::ListExpiredLeases(_) => {
                    let expired = self.expired_leases.lock().unwrap().clone();
                    Ok(QueryResult::ExpiredLeases(ExpiredLeasesResponse {
                        expired,
                    }))
                }
                ApplicationQuery::ListWorkItems(_) => {
                    Ok(QueryResult::WorkItems(WorkItemsResponse {
                        work_items: vec![],
                    }))
                }
                _ => Ok(QueryResult::WorkItems(WorkItemsResponse {
                    work_items: vec![],
                })),
            }
        }

        fn events_after(
            &self,
            _project: ProjectId,
            _sequence: u64,
            _limit: usize,
        ) -> autore_core::Result<Vec<ProjectEvent>> {
            Ok(vec![])
        }

        fn subscribe_events(
            &self,
            _project: ProjectId,
            _after: u64,
        ) -> autore_core::Result<
            autore_app::autore_events::project_event_service::ProjectEventSubscription,
        > {
            unimplemented!("not needed in scheduler tests")
        }
    }

    // -- Helpers --

    fn make_task(kind: TaskKind, priority: u64, attempts: u32, deps: usize) -> Task {
        let mut task = Task::new(
            TaskId::new(),
            CampaignId::new(),
            kind,
            TaskSubject::Binary,
            TaskPriority::new(priority),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        task.attempt_count = attempts;
        for _ in 0..deps {
            task.dependencies.push(TaskId::new());
        }
        task
    }

    fn make_task_with_state(_campaign_id: CampaignId, priority: u64, state: TaskState) -> Task {
        let mut task = Task::new(
            TaskId::new(),
            _campaign_id,
            TaskKind::AnalyzeFunction,
            TaskSubject::Binary,
            TaskPriority::new(priority),
            RequiredCapabilities::new(false, true, false, false),
            None,
            None,
            3,
        );
        task.state = state;
        task
    }

    fn sample_models() -> Vec<ModelDescriptor> {
        vec![ModelDescriptor {
            id: "analyzer-1".into(),
            name: "Analyzer".into(),
            class: ModelClass::Analyzer,
            capabilities: ModelCapabilities {
                json_mode: true,
                tool_use: false,
                analysis: true,
                verification: false,
            },
            max_context_tokens: 8192,
        }]
    }

    fn make_scheduler() -> Scheduler {
        Scheduler::new(ModelRouter::new(sample_models()))
    }

    // -- Priority scoring tests --

    #[test]
    fn priority_score_is_stable() {
        let factors = PriorityFactors::default();
        let ctx = PriorityContext::default();
        let now = OffsetDateTime::now_utc();
        let task = make_task(TaskKind::AnalyzeFunction, 50, 2, 3);

        let score_a = Scheduler::priority_score(&task, &factors, &ctx, now);
        let score_b = Scheduler::priority_score(&task, &factors, &ctx, now);
        let score_c = Scheduler::priority_score(&task, &factors, &ctx, now);

        assert_eq!(score_a, score_b);
        assert_eq!(score_b, score_c);
        assert_eq!(score_a, 185);
    }

    #[test]
    fn priority_score_unblocked_dependents_bonus() {
        let factors = PriorityFactors::default();
        let now = OffsetDateTime::now_utc();
        let task = make_task(TaskKind::AnalyzeFunction, 50, 0, 0);

        let ctx_none = PriorityContext::default();
        let ctx_some = PriorityContext {
            dependents_unblocked: 5,
            ..PriorityContext::default()
        };

        let base = Scheduler::priority_score(&task, &factors, &ctx_none, now);
        let bonus = Scheduler::priority_score(&task, &factors, &ctx_some, now);

        assert!(bonus > base);
        assert_eq!(
            bonus - base,
            (5.0 * factors.dependents_unblocked_weight) as u64
        );
    }

    #[test]
    fn priority_score_build_blocker_bonus() {
        let factors = PriorityFactors::default();
        let now = OffsetDateTime::now_utc();
        let task = make_task(TaskKind::AnalyzeFunction, 50, 0, 0);

        let ctx_none = PriorityContext::default();
        let ctx_blocker = PriorityContext {
            removes_build_blocker: true,
            ..PriorityContext::default()
        };

        let base = Scheduler::priority_score(&task, &factors, &ctx_none, now);
        let bonus = Scheduler::priority_score(&task, &factors, &ctx_blocker, now);

        assert!(bonus > base);
        assert_eq!(bonus - base, factors.removes_build_blocker_weight as u64);
    }

    #[test]
    fn priority_factors_are_inspectable() {
        let factors = PriorityFactors {
            base_priority: 200,
            attempt_count_weight: 15.0,
            dependency_depth_weight: 8.0,
            deadline_weight: 2.5,
            verification_weight: 30.0,
            dependents_unblocked_weight: 12.0,
            high_impact_conflict_weight: 20.0,
            removes_build_blocker_weight: 25.0,
            verified_coverage_weight: 8.0,
            evidence_strength_weight: 4.0,
        };

        assert_eq!(factors.base_priority, 200);
        assert!((factors.dependents_unblocked_weight - 12.0).abs() < f64::EPSILON);
        assert!((factors.high_impact_conflict_weight - 20.0).abs() < f64::EPSILON);
        assert!((factors.removes_build_blocker_weight - 25.0).abs() < f64::EPSILON);
        assert!((factors.verified_coverage_weight - 8.0).abs() < f64::EPSILON);
        assert!((factors.evidence_strength_weight - 4.0).abs() < f64::EPSILON);

        let json = serde_json::to_string(&factors).unwrap();
        let deserialized: PriorityFactors = serde_json::from_str(&json).unwrap();
        assert_eq!(factors, deserialized);
    }

    #[test]
    fn priority_score_verification_bonus() {
        let factors = PriorityFactors::default();
        let ctx = PriorityContext::default();
        let now = OffsetDateTime::now_utc();

        let analysis_task = make_task(TaskKind::AnalyzeFunction, 50, 0, 0);
        let verify_task = make_task(TaskKind::VerifyClaim, 50, 0, 0);

        let analysis_score = Scheduler::priority_score(&analysis_task, &factors, &ctx, now);
        let verify_score = Scheduler::priority_score(&verify_task, &factors, &ctx, now);

        assert_eq!(analysis_score, 150);
        assert_eq!(verify_score, 170);
    }

    #[test]
    fn scheduler_score_tasks_batch() {
        let router = ModelRouter::new(sample_models());
        let scheduler = Scheduler::new(router);
        let factors = PriorityFactors::default();
        let ctx = PriorityContext::default();
        let now = OffsetDateTime::now_utc();

        let tasks = vec![
            make_task(TaskKind::AnalyzeFunction, 10, 0, 0),
            make_task(TaskKind::VerifyClaim, 20, 1, 2),
        ];

        let scores = scheduler.score_tasks(&tasks, &factors, &ctx, now);
        assert_eq!(scores.len(), 2);
        assert_eq!(scores[0], 110);
        assert_eq!(scores[1], 160);
    }

    // -- Campaign evaluation tests --

    #[test]
    fn scheduler_evaluates_complete_unchanged_after_refactor() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();
        let client: Arc<dyn AutoReClient> = Arc::new(RecordingAutoReClient::new());

        let tasks = vec![
            make_task_with_state(cid, 100, TaskState::Completed),
            make_task_with_state(cid, 200, TaskState::Completed),
        ];

        let scheduler = make_scheduler();
        let mut eval = CampaignEvaluation::Idle;

        scheduler
            .run_campaign(cid, &mut eval, client, pid, &tasks, now)
            .unwrap();

        assert_eq!(eval, CampaignEvaluation::Complete);
    }

    #[test]
    fn scheduler_recovers_expired_lease_via_command() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();

        let mut task = make_task_with_state(cid, 100, TaskState::Leased);
        task.attempt_count = 1;
        task.maximum_attempts = 3;
        let expired_id = task.id.to_string();

        let client = Arc::new(RecordingAutoReClient::with_expired_leases(vec![
            expired_id.clone(),
        ]));

        let scheduler = make_scheduler().with_max_dispatch(0);
        let mut eval = CampaignEvaluation::Idle;

        scheduler
            .run_campaign(
                cid,
                &mut eval,
                Arc::clone(&client) as Arc<dyn AutoReClient>,
                pid,
                &[task],
                now,
            )
            .unwrap();

        let commands = client.recorded_commands();
        let has_requeue = commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::RequeueWorkItem(_)));
        assert!(has_requeue, "expected RequeueWorkItem for retryable task");

        let queries = client.recorded_queries();
        let has_expired_query = queries
            .iter()
            .any(|q| matches!(q, ApplicationQuery::ListExpiredLeases(_)));
        assert!(has_expired_query, "expected ListExpiredLeases query");
    }

    #[test]
    fn scheduler_recovers_expired_lease_fails_exhausted() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();

        let mut task = make_task_with_state(cid, 100, TaskState::Leased);
        task.attempt_count = 3;
        task.maximum_attempts = 3;
        let expired_id = task.id.to_string();

        let client = Arc::new(RecordingAutoReClient::with_expired_leases(vec![expired_id]));

        let scheduler = make_scheduler().with_max_dispatch(0);
        let mut eval = CampaignEvaluation::Idle;

        scheduler
            .run_campaign(
                cid,
                &mut eval,
                Arc::clone(&client) as Arc<dyn AutoReClient>,
                pid,
                &[task],
                now,
            )
            .unwrap();

        let commands = client.recorded_commands();
        let has_fail = commands
            .iter()
            .any(|c| matches!(c, ApplicationCommand::FailWorkItem(_)));
        assert!(has_fail, "expected FailWorkItem for exhausted task");
    }

    #[test]
    fn scheduler_does_not_touch_storage() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();

        let tasks = vec![
            make_task_with_state(cid, 100, TaskState::Completed),
            make_task_with_state(cid, 200, TaskState::Pending),
        ];

        let client = Arc::new(RecordingAutoReClient::new());
        let scheduler = make_scheduler();
        let mut eval = CampaignEvaluation::Idle;

        scheduler
            .run_campaign(
                cid,
                &mut eval,
                Arc::clone(&client) as Arc<dyn AutoReClient>,
                pid,
                &tasks,
                now,
            )
            .unwrap();

        let queries = client.recorded_queries();
        let has_list_expired = queries
            .iter()
            .any(|q| matches!(q, ApplicationQuery::ListExpiredLeases(_)));
        assert!(
            has_list_expired,
            "expected exactly one ListExpiredLeases query via client"
        );
    }

    #[test]
    fn scheduler_respects_dependencies() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();

        let task_a = make_task_with_state(cid, 200, TaskState::Pending);
        let mut task_b = make_task_with_state(cid, 100, TaskState::Pending);
        task_b.dependencies.push(task_a.id);

        let client = Arc::new(RecordingAutoReClient::new());
        let scheduler = make_scheduler();
        let mut eval = CampaignEvaluation::Idle;

        scheduler
            .run_campaign(
                cid,
                &mut eval,
                Arc::clone(&client) as Arc<dyn AutoReClient>,
                pid,
                &[task_a.clone(), task_b.clone()],
                now,
            )
            .unwrap();

        let commands = client.recorded_commands();
        let promoted_ids: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                ApplicationCommand::PromoteWorkItem(req) => Some(req.work_item_id.clone()),
                _ => None,
            })
            .collect();

        assert!(
            promoted_ids.contains(&task_a.id.to_string()),
            "task_a (no deps) should be promoted"
        );
        assert!(
            !promoted_ids.contains(&task_b.id.to_string()),
            "task_b (dep not completed) should NOT be promoted"
        );

        let leased_ids: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                ApplicationCommand::LeaseWorkItem(req) => Some(req.work_item_id.clone()),
                _ => None,
            })
            .collect();

        assert!(
            leased_ids.contains(&task_a.id.to_string()),
            "task_a should be leased after promotion"
        );
        assert_eq!(eval, CampaignEvaluation::Active);
    }

    #[test]
    fn scheduler_idle_when_only_leased() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();

        let task = make_task_with_state(cid, 100, TaskState::Leased);
        let client: Arc<dyn AutoReClient> = Arc::new(RecordingAutoReClient::new());
        let scheduler = make_scheduler();
        let mut eval = CampaignEvaluation::Active;

        scheduler
            .run_campaign(cid, &mut eval, client, pid, &[task], now)
            .unwrap();

        assert_eq!(eval, CampaignEvaluation::Idle);
    }

    #[test]
    fn scheduler_dispatches_ready_by_priority() {
        let cid = CampaignId::new();
        let pid = ProjectId::new();
        let now = OffsetDateTime::now_utc();

        let low = make_task_with_state(cid, 50, TaskState::Ready);
        let high = make_task_with_state(cid, 200, TaskState::Ready);

        let client = Arc::new(RecordingAutoReClient::new());
        let scheduler = make_scheduler().with_max_dispatch(1);
        let mut eval = CampaignEvaluation::Idle;

        scheduler
            .run_campaign(
                cid,
                &mut eval,
                Arc::clone(&client) as Arc<dyn AutoReClient>,
                pid,
                &[low.clone(), high.clone()],
                now,
            )
            .unwrap();

        let commands = client.recorded_commands();
        let leased_ids: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                ApplicationCommand::LeaseWorkItem(req) => Some(req.work_item_id.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(leased_ids.len(), 1);
        assert_eq!(leased_ids[0], high.id.to_string());
        assert_eq!(eval, CampaignEvaluation::Active);
    }

    #[test]
    fn evaluate_state_pure_function() {
        assert_eq!(Scheduler::evaluate_state(&[], 0), CampaignEvaluation::Idle);

        let completed = vec![make_task_with_state(
            CampaignId::new(),
            10,
            TaskState::Completed,
        )];
        assert_eq!(
            Scheduler::evaluate_state(&completed, 0),
            CampaignEvaluation::Complete
        );

        let ready = vec![make_task_with_state(
            CampaignId::new(),
            10,
            TaskState::Ready,
        )];
        assert_eq!(
            Scheduler::evaluate_state(&ready, 0),
            CampaignEvaluation::Active
        );

        let leased = vec![make_task_with_state(
            CampaignId::new(),
            10,
            TaskState::Leased,
        )];
        assert_eq!(
            Scheduler::evaluate_state(&leased, 0),
            CampaignEvaluation::Idle
        );

        let pending = vec![make_task_with_state(
            CampaignId::new(),
            10,
            TaskState::Pending,
        )];
        assert_eq!(
            Scheduler::evaluate_state(&pending, 0),
            CampaignEvaluation::Blocked
        );

        assert_eq!(
            Scheduler::evaluate_state(&completed, 3),
            CampaignEvaluation::Complete
        );
    }
}
