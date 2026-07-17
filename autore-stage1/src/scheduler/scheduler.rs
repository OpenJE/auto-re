//! Scheduler — deterministic priority scoring and model-routed dispatch.
//!
//! The scheduler computes a stable, deterministic priority score for each
//! task using configurable `PriorityFactors`. The same inputs always produce
//! the same score — no randomness, no platform-dependent hashing.
//!
//! The `run_campaign` method performs one evaluation tick: recovering
//! expired leases, promoting pending tasks, dispatching ready tasks, and
//! evaluating the campaign state.

use time::OffsetDateTime;

use crate::domain::task::{Task, TaskKind, TaskState};
use crate::ids::{CampaignId, TaskId};
use crate::model::router::ModelRouter;

use super::repos::RepositorySet;

// ---------------------------------------------------------------------------
// PriorityFactors
// ---------------------------------------------------------------------------

/// Configurable weights for the priority score formula.
///
/// Each weight is applied to a specific task attribute. The final score is
/// a deterministic `u64` derived from integer-scaled floating-point arithmetic.
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
}

impl Default for PriorityFactors {
    fn default() -> Self {
        Self {
            base_priority: 100,
            attempt_count_weight: 10.0,
            dependency_depth_weight: 5.0,
            deadline_weight: 1.0,
            verification_weight: 20.0,
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
    /// ```
    ///
    /// The result is truncated to `u64`. The same inputs always produce the
    /// same output — no randomness or platform-dependent behavior.
    pub fn priority_score(task: &Task, factors: &PriorityFactors, _now: OffsetDateTime) -> u64 {
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

        let total = base + attempt_bonus + dep_bonus + priority_bonus + verification_bonus;
        total.max(0.0) as u64
    }

    /// Computes priority scores for a slice of tasks, returning them in the
    /// same order.
    pub fn score_tasks(
        &self,
        tasks: &[Task],
        factors: &PriorityFactors,
        now: OffsetDateTime,
    ) -> Vec<u64> {
        tasks
            .iter()
            .map(|t| Self::priority_score(t, factors, now))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Campaign evaluation loop
    // -----------------------------------------------------------------------

    /// Performs one campaign evaluation tick:
    ///
    /// 1. Recover expired leases (reset to Ready or Failed).
    /// 2. Promote Pending tasks with satisfied dependencies to Ready.
    /// 3. Invalidate stale work (M1: no-op).
    /// 4. Lease and dispatch up to `max_dispatch_per_tick` ready tasks.
    /// 5. Evaluate state and set `CampaignEvaluation`.
    pub async fn run_campaign(
        &self,
        campaign_id: CampaignId,
        evaluation: &mut CampaignEvaluation,
        repos: &RepositorySet,
        now: OffsetDateTime,
    ) -> crate::Result<()> {
        self.recover_expired_leases(campaign_id, repos, now).await?;
        self.promote_ready_tasks(campaign_id, repos).await?;
        // Step 3: invalidation is a no-op for M1.
        let dispatched = self.dispatch_tasks(campaign_id, repos, now).await?;
        *evaluation = self.evaluate_state(campaign_id, repos, dispatched).await?;
        Ok(())
    }

    /// Convenience method: runs one tick and returns the evaluation.
    pub async fn evaluate(
        &self,
        campaign_id: CampaignId,
        repos: &RepositorySet,
        now: OffsetDateTime,
    ) -> crate::Result<CampaignEvaluation> {
        let mut eval = CampaignEvaluation::Idle;
        self.run_campaign(campaign_id, &mut eval, repos, now)
            .await?;
        Ok(eval)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Recovers expired leases: resets tasks to Ready or fails them
    /// permanently if max attempts are exceeded.
    async fn recover_expired_leases(
        &self,
        campaign_id: CampaignId,
        repos: &RepositorySet,
        now: OffsetDateTime,
    ) -> crate::Result<()> {
        let expired = repos.queries.find_expired_leases(campaign_id, now).await?;
        if expired.is_empty() {
            return Ok(());
        }
        let tasks = repos.queries.find_tasks_by_campaign(campaign_id).await?;
        for lease in &expired {
            if let Some(task) = tasks.iter().find(|t| t.id == lease.task_id) {
                if task.attempt_count >= task.maximum_attempts {
                    repos
                        .tasks
                        .fail(task.id, "lease expired, max attempts exceeded".into())
                        .await?;
                } else {
                    repos
                        .queries
                        .update_task_state(task.id, TaskState::Ready)
                        .await?;
                    repos.queries.delete_lease(task.id).await?;
                }
            }
        }
        Ok(())
    }

    /// Promotes Pending tasks to Ready when all dependencies are Completed.
    async fn promote_ready_tasks(
        &self,
        campaign_id: CampaignId,
        repos: &RepositorySet,
    ) -> crate::Result<()> {
        let tasks = repos.queries.find_tasks_by_campaign(campaign_id).await?;
        let completed_ids: Vec<TaskId> = tasks
            .iter()
            .filter(|t| t.state == TaskState::Completed)
            .map(|t| t.id)
            .collect();
        for task in &tasks {
            if task.state == TaskState::Pending && task.dependencies_satisfied(&completed_ids) {
                repos
                    .queries
                    .update_task_state(task.id, TaskState::Ready)
                    .await?;
            }
        }
        Ok(())
    }

    /// Leases and dispatches up to `max_dispatch_per_tick` ready tasks.
    async fn dispatch_tasks(
        &self,
        campaign_id: CampaignId,
        repos: &RepositorySet,
        now: OffsetDateTime,
    ) -> crate::Result<usize> {
        let mut dispatched = 0;
        for _ in 0..self.max_dispatch_per_tick {
            match repos.tasks.lease_next(campaign_id, now).await? {
                Some(_) => dispatched += 1,
                None => break,
            }
        }
        Ok(dispatched)
    }

    /// Evaluates the campaign state based on current task statuses.
    async fn evaluate_state(
        &self,
        campaign_id: CampaignId,
        repos: &RepositorySet,
        dispatched: usize,
    ) -> crate::Result<CampaignEvaluation> {
        let tasks = repos.queries.find_tasks_by_campaign(campaign_id).await?;
        if tasks.is_empty() {
            return Ok(CampaignEvaluation::Idle);
        }
        if tasks.iter().all(|t| t.state.is_terminal()) {
            return Ok(CampaignEvaluation::Complete);
        }
        if dispatched > 0 {
            return Ok(CampaignEvaluation::Active);
        }
        if tasks.iter().any(|t| t.state == TaskState::Ready) {
            return Ok(CampaignEvaluation::Active);
        }
        if tasks
            .iter()
            .any(|t| matches!(t.state, TaskState::Leased | TaskState::Running))
        {
            return Ok(CampaignEvaluation::Idle);
        }
        Ok(CampaignEvaluation::Blocked)
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
    use crate::ids::{CampaignId, TaskId};
    use crate::model::{ModelCapabilities, ModelClass, ModelDescriptor};

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

    fn sample_models() -> Vec<ModelDescriptor> {
        vec![
            ModelDescriptor {
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
            },
            ModelDescriptor {
                id: "verifier-1".into(),
                name: "Verifier".into(),
                class: ModelClass::Verifier,
                capabilities: ModelCapabilities {
                    json_mode: true,
                    tool_use: true,
                    analysis: false,
                    verification: true,
                },
                max_context_tokens: 4096,
            },
        ]
    }

    #[test]
    fn priority_score_is_stable() {
        let factors = PriorityFactors::default();
        let now = OffsetDateTime::now_utc();
        let task = make_task(TaskKind::AnalyzeFunction, 50, 2, 3);

        let score_a = Scheduler::priority_score(&task, &factors, now);
        let score_b = Scheduler::priority_score(&task, &factors, now);
        let score_c = Scheduler::priority_score(&task, &factors, now);

        assert_eq!(score_a, score_b);
        assert_eq!(score_b, score_c);

        // Verify the expected value:
        // base(100) + attempts(2*10=20) + deps(3*5=15) + priority(50*1=50) + verification(0) = 185
        assert_eq!(score_a, 185);
    }

    #[test]
    fn priority_factors_are_inspectable() {
        let factors = PriorityFactors {
            base_priority: 200,
            attempt_count_weight: 15.0,
            dependency_depth_weight: 8.0,
            deadline_weight: 2.5,
            verification_weight: 30.0,
        };

        assert_eq!(factors.base_priority, 200);
        assert!((factors.attempt_count_weight - 15.0).abs() < f64::EPSILON);
        assert!((factors.dependency_depth_weight - 8.0).abs() < f64::EPSILON);
        assert!((factors.deadline_weight - 2.5).abs() < f64::EPSILON);
        assert!((factors.verification_weight - 30.0).abs() < f64::EPSILON);

        // Verify serialization roundtrip preserves values.
        let json = serde_json::to_string(&factors).unwrap();
        let deserialized: PriorityFactors = serde_json::from_str(&json).unwrap();
        assert_eq!(factors, deserialized);
    }

    #[test]
    fn priority_score_verification_bonus() {
        let factors = PriorityFactors::default();
        let now = OffsetDateTime::now_utc();

        let analysis_task = make_task(TaskKind::AnalyzeFunction, 50, 0, 0);
        let verify_task = make_task(TaskKind::VerifyClaim, 50, 0, 0);

        let analysis_score = Scheduler::priority_score(&analysis_task, &factors, now);
        let verify_score = Scheduler::priority_score(&verify_task, &factors, now);

        // Verify task gets the verification bonus (20.0 default).
        assert_eq!(analysis_score, 150); // 100 + 50
        assert_eq!(verify_score, 170); // 100 + 50 + 20
    }

    #[test]
    fn scheduler_score_tasks_batch() {
        let router = ModelRouter::new(sample_models());
        let scheduler = Scheduler::new(router);
        let factors = PriorityFactors::default();
        let now = OffsetDateTime::now_utc();

        let tasks = vec![
            make_task(TaskKind::AnalyzeFunction, 10, 0, 0),
            make_task(TaskKind::VerifyClaim, 20, 1, 2),
        ];

        let scores = scheduler.score_tasks(&tasks, &factors, now);
        assert_eq!(scores.len(), 2);
        // Task 1: 100 + 0 + 0 + 10 + 0 = 110
        assert_eq!(scores[0], 110);
        // Task 2: 100 + (1*10) + (2*5) + (20*1) + 20 = 160
        assert_eq!(scores[1], 160);
    }

    // -----------------------------------------------------------------------
    // Campaign evaluation tests
    // -----------------------------------------------------------------------

    mod campaign_tests {
        use super::*;
        use crate::domain::{Campaign, CampaignState, Claim, Evidence};
        use crate::ids::{ClaimId, EvidenceId};
        use crate::scheduler::lease::TaskLease;
        use crate::scheduler::repos::{RepositorySet, SchedulerQueries};
        use crate::storage::repositories::{
            CampaignRepository, ClaimRepository, EvidenceRepository, TaskRepository,
        };
        use async_trait::async_trait;
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};
        use time::Duration;

        // -- Mock store --

        struct MockStore {
            tasks: Mutex<Vec<Task>>,
            leases: Mutex<Vec<TaskLease>>,
        }

        impl MockStore {
            fn new() -> Self {
                Self {
                    tasks: Mutex::new(Vec::new()),
                    leases: Mutex::new(Vec::new()),
                }
            }

            fn add_task(&self, task: Task) {
                self.tasks.lock().unwrap().push(task);
            }

            fn add_lease(&self, lease: TaskLease) {
                self.leases.lock().unwrap().push(lease);
            }

            fn get_tasks(&self) -> Vec<Task> {
                self.tasks.lock().unwrap().clone()
            }

            fn get_leases(&self) -> Vec<TaskLease> {
                self.leases.lock().unwrap().clone()
            }
        }

        #[async_trait]
        impl TaskRepository for MockStore {
            async fn create(&self, task: &Task) -> autore_core::Result<TaskId> {
                self.tasks.lock().unwrap().push(task.clone());
                Ok(task.id)
            }

            async fn lease_next(
                &self,
                campaign_id: CampaignId,
                now: OffsetDateTime,
            ) -> autore_core::Result<Option<Task>> {
                let mut tasks = self.tasks.lock().unwrap();
                let mut leases = self.leases.lock().unwrap();

                let completed_ids: Vec<TaskId> = tasks
                    .iter()
                    .filter(|t| t.state == TaskState::Completed)
                    .map(|t| t.id)
                    .collect();

                let candidate_idx = tasks
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.campaign_id == campaign_id && t.state == TaskState::Ready)
                    .filter(|(_, t)| t.dependencies_satisfied(&completed_ids))
                    .max_by_key(|(_, t)| t.priority.score())
                    .map(|(i, _)| i);

                match candidate_idx {
                    Some(idx) => {
                        tasks[idx].state = TaskState::Leased;
                        let leased_task = tasks[idx].clone();
                        leases.push(TaskLease {
                            task_id: leased_task.id,
                            campaign_id: leased_task.campaign_id,
                            worker_id: uuid::Uuid::new_v4().to_string(),
                            started_at: now,
                            expires_at: now + Duration::seconds(300),
                        });
                        Ok(Some(leased_task))
                    }
                    None => Ok(None),
                }
            }

            async fn renew_lease(
                &self,
                task_id: TaskId,
                until: OffsetDateTime,
            ) -> autore_core::Result<()> {
                let mut leases = self.leases.lock().unwrap();
                if let Some(lease) = leases.iter_mut().find(|l| l.task_id == task_id) {
                    lease.expires_at = until;
                    Ok(())
                } else {
                    Err(autore_core::Error::Validation(format!(
                        "no lease for task {task_id}"
                    )))
                }
            }

            async fn complete(&self, task_id: TaskId) -> autore_core::Result<()> {
                let mut tasks = self.tasks.lock().unwrap();
                if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                    task.state = TaskState::Completed;
                }
                drop(tasks);
                let mut leases = self.leases.lock().unwrap();
                leases.retain(|l| l.task_id != task_id);
                Ok(())
            }

            async fn fail(&self, task_id: TaskId, _error: String) -> autore_core::Result<()> {
                let mut tasks = self.tasks.lock().unwrap();
                if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                    task.state = TaskState::Failed;
                    task.attempt_count += 1;
                }
                drop(tasks);
                let mut leases = self.leases.lock().unwrap();
                leases.retain(|l| l.task_id != task_id);
                Ok(())
            }
        }

        #[async_trait]
        impl SchedulerQueries for MockStore {
            async fn find_tasks_by_campaign(
                &self,
                campaign_id: CampaignId,
            ) -> crate::Result<Vec<Task>> {
                let tasks = self.tasks.lock().unwrap();
                Ok(tasks
                    .iter()
                    .filter(|t| t.campaign_id == campaign_id)
                    .cloned()
                    .collect())
            }

            async fn find_expired_leases(
                &self,
                campaign_id: CampaignId,
                now: OffsetDateTime,
            ) -> crate::Result<Vec<TaskLease>> {
                let tasks = self.tasks.lock().unwrap();
                let leases = self.leases.lock().unwrap();

                let leased_ids: HashSet<TaskId> = tasks
                    .iter()
                    .filter(|t| t.campaign_id == campaign_id && t.state == TaskState::Leased)
                    .map(|t| t.id)
                    .collect();

                Ok(leases
                    .iter()
                    .filter(|l| leased_ids.contains(&l.task_id) && l.expires_at <= now)
                    .cloned()
                    .collect())
            }

            async fn update_task_state(
                &self,
                task_id: TaskId,
                state: TaskState,
            ) -> crate::Result<()> {
                let mut tasks = self.tasks.lock().unwrap();
                if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                    task.state = state;
                }
                Ok(())
            }

            async fn delete_lease(&self, task_id: TaskId) -> crate::Result<()> {
                let mut leases = self.leases.lock().unwrap();
                leases.retain(|l| l.task_id != task_id);
                Ok(())
            }
        }

        // -- No-op repositories for unused traits --

        struct NoopCampaignRepo;

        #[async_trait]
        impl CampaignRepository for NoopCampaignRepo {
            async fn create(&self, _c: &Campaign) -> autore_core::Result<CampaignId> {
                Ok(CampaignId::new())
            }
            async fn find_by_id(&self, _id: CampaignId) -> autore_core::Result<Option<Campaign>> {
                Ok(None)
            }
            async fn update_state(
                &self,
                _id: CampaignId,
                _state: CampaignState,
            ) -> autore_core::Result<()> {
                Ok(())
            }
        }

        struct NoopClaimRepo;

        #[async_trait]
        impl ClaimRepository for NoopClaimRepo {
            async fn create(&self, _c: &Claim) -> autore_core::Result<ClaimId> {
                Ok(ClaimId::new())
            }
            async fn find_by_id(&self, _id: ClaimId) -> autore_core::Result<Option<Claim>> {
                Ok(None)
            }
        }

        struct NoopEvidenceRepo;

        #[async_trait]
        impl EvidenceRepository for NoopEvidenceRepo {
            async fn create(&self, _e: &Evidence) -> autore_core::Result<EvidenceId> {
                Ok(EvidenceId::new())
            }
            async fn find_by_id(&self, _id: EvidenceId) -> autore_core::Result<Option<Evidence>> {
                Ok(None)
            }
        }

        // -- Test helpers --

        fn make_repos(store: Arc<MockStore>) -> RepositorySet {
            RepositorySet {
                tasks: Arc::clone(&store) as Arc<dyn TaskRepository>,
                queries: Arc::clone(&store) as Arc<dyn SchedulerQueries>,
                campaigns: Arc::new(NoopCampaignRepo) as Arc<dyn CampaignRepository>,
                claims: Arc::new(NoopClaimRepo) as Arc<dyn ClaimRepository>,
                evidence: Arc::new(NoopEvidenceRepo) as Arc<dyn EvidenceRepository>,
            }
        }

        fn make_scheduler() -> Scheduler {
            Scheduler::new(ModelRouter::new(vec![ModelDescriptor {
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
            }]))
        }

        fn make_campaign_task(campaign_id: CampaignId, priority: u64, state: TaskState) -> Task {
            let mut task = Task::new(
                TaskId::new(),
                campaign_id,
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

        // -- Tests --

        #[tokio::test]
        async fn scheduler_evaluates_complete() {
            let store = Arc::new(MockStore::new());
            let cid = CampaignId::new();
            let now = OffsetDateTime::now_utc();

            store.add_task(make_campaign_task(cid, 100, TaskState::Completed));
            store.add_task(make_campaign_task(cid, 200, TaskState::Completed));

            let repos = make_repos(Arc::clone(&store));
            let scheduler = make_scheduler();
            let mut eval = CampaignEvaluation::Idle;

            scheduler
                .run_campaign(cid, &mut eval, &repos, now)
                .await
                .unwrap();

            assert_eq!(eval, CampaignEvaluation::Complete);
        }

        #[tokio::test]
        async fn scheduler_recovers_expired_lease() {
            let store = Arc::new(MockStore::new());
            let cid = CampaignId::new();
            let now = OffsetDateTime::now_utc();

            let mut task = make_campaign_task(cid, 100, TaskState::Leased);
            task.attempt_count = 1;
            task.maximum_attempts = 3;
            store.add_task(task.clone());
            store.add_lease(TaskLease {
                task_id: task.id,
                campaign_id: cid,
                worker_id: "worker-1".into(),
                started_at: now - Duration::seconds(600),
                expires_at: now - Duration::seconds(300),
            });

            let repos = make_repos(Arc::clone(&store));
            // Disable dispatch so the recovered task stays Ready.
            let scheduler = make_scheduler().with_max_dispatch(0);
            let mut eval = CampaignEvaluation::Idle;

            scheduler
                .run_campaign(cid, &mut eval, &repos, now)
                .await
                .unwrap();

            let tasks = store.get_tasks();
            let recovered = tasks.iter().find(|t| t.id == task.id).unwrap();
            assert_eq!(recovered.state, TaskState::Ready);
            assert!(
                store.get_leases().is_empty(),
                "expired lease should be deleted"
            );
        }

        #[tokio::test]
        async fn scheduler_invalidates_stale_work() {
            let store = Arc::new(MockStore::new());
            let cid = CampaignId::new();
            let now = OffsetDateTime::now_utc();

            // M1: invalidation is a no-op. A Ready task with non-zero
            // input_revision should still be dispatched normally.
            let mut task = make_campaign_task(cid, 100, TaskState::Ready);
            task.input_revision = 5;
            store.add_task(task.clone());

            let repos = make_repos(Arc::clone(&store));
            let scheduler = make_scheduler();
            let mut eval = CampaignEvaluation::Idle;

            scheduler
                .run_campaign(cid, &mut eval, &repos, now)
                .await
                .unwrap();

            let tasks = store.get_tasks();
            let t = tasks.iter().find(|t| t.id == task.id).unwrap();
            // Task dispatched normally — not invalidated.
            assert_eq!(t.state, TaskState::Leased);
            assert_eq!(eval, CampaignEvaluation::Active);
        }

        #[tokio::test]
        async fn scheduler_respects_dependencies() {
            let store = Arc::new(MockStore::new());
            let cid = CampaignId::new();
            let now = OffsetDateTime::now_utc();

            let task_a = make_campaign_task(cid, 200, TaskState::Pending);
            let mut task_b = make_campaign_task(cid, 100, TaskState::Pending);
            task_b.dependencies.push(task_a.id);

            store.add_task(task_a.clone());
            store.add_task(task_b.clone());

            let repos = make_repos(Arc::clone(&store));
            let scheduler = make_scheduler();
            let mut eval = CampaignEvaluation::Idle;

            scheduler
                .run_campaign(cid, &mut eval, &repos, now)
                .await
                .unwrap();

            let tasks = store.get_tasks();
            let a = tasks.iter().find(|t| t.id == task_a.id).unwrap();
            let b = tasks.iter().find(|t| t.id == task_b.id).unwrap();

            // task_a (no deps) promoted to Ready and leased.
            assert_eq!(a.state, TaskState::Leased);
            // task_b stays Pending — dependency not yet Completed.
            assert_eq!(b.state, TaskState::Pending);
            assert_eq!(eval, CampaignEvaluation::Active);
        }

        #[tokio::test]
        async fn scheduler_idle_sleeps() {
            let store = Arc::new(MockStore::new());
            let cid = CampaignId::new();
            let now = OffsetDateTime::now_utc();

            // One task actively leased (not expired) — nothing to dispatch.
            let task = make_campaign_task(cid, 100, TaskState::Leased);
            store.add_task(task.clone());
            store.add_lease(TaskLease {
                task_id: task.id,
                campaign_id: cid,
                worker_id: "worker-1".into(),
                started_at: now,
                expires_at: now + Duration::seconds(300),
            });

            let repos = make_repos(Arc::clone(&store));
            let scheduler = make_scheduler();
            let mut eval = CampaignEvaluation::Active;

            scheduler
                .run_campaign(cid, &mut eval, &repos, now)
                .await
                .unwrap();

            assert_eq!(eval, CampaignEvaluation::Idle);
        }
    }
}
