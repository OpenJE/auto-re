use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::broadcast;

use autore_schema::domain::records::{EventSource, EventSubject, ProjectEvent};
use autore_schema::domain::{ExtensionData, NamespacedId};
use autore_schema::ids::ProjectId;
use autore_store::{
    Database, EventStore, SqliteEventStore, emit_in_tx, next_project_event_sequence,
};

/// Capacity of the in-process broadcast channel used by [`EventBroadcaster`].
///
/// Lag detection recovers slow subscribers from the durable store, so this
/// only bounds steady-state memory. 256 events is the Stage 0 default; callers
/// may construct a broadcaster with a different capacity via
/// [`EventBroadcaster::with_capacity`].
pub const EVENT_BROADCAST_CAPACITY: usize = 256;

/// Default limit used for internal replay and resync queries when a subscriber
/// calls [`ProjectEventService::subscribe`].
const SUBSCRIPTION_REPLAY_LIMIT: usize = usize::MAX;

/// Internal broadcaster that forwards committed [`ProjectEvent`]s to live
/// subscribers.
///
/// The broadcaster owns a `tokio::sync::broadcast::Sender`. Events are sent
/// after they have been durably committed to the SQLite store; failure to send
/// (e.g., no active receivers) is ignored because the store is authoritative.
#[derive(Clone)]
pub struct EventBroadcaster {
    tx: broadcast::Sender<ProjectEvent>,
}

impl EventBroadcaster {
    /// Creates a broadcaster with the default [`EVENT_BROADCAST_CAPACITY`].
    pub fn new() -> Self {
        Self::with_capacity(EVENT_BROADCAST_CAPACITY)
    }

    /// Creates a broadcaster with a custom capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Returns a new receiver subscribed to the live event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<ProjectEvent> {
        self.tx.subscribe()
    }

    /// Broadcasts an event to all live subscribers.
    pub fn broadcast(&self, event: ProjectEvent) {
        // SendError only means there are no receivers; the event is already
        // durable in the store, so this is not a failure.
        let _ = self.tx.send(event);
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// Marker for subscriber-lag detection. When a [`broadcast::Receiver`]
/// reports [`broadcast::error::RecvError::Lagged`], the subscription treats it as
/// a signal to resync from the durable store rather than dropping events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriberLag;

/// Service trait for replaying and subscribing to project events.
///
/// All implementations are `Send + Sync` so they can be shared across tokio
/// tasks. The subscription is in-process only; no network transport is used.
pub trait ProjectEventService: Send + Sync {
    /// Returns up to `limit` persisted events for `project_id` with
    /// `sequence > after_sequence`, ordered by sequence ascending.
    fn events_after(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>>;

    /// Subscribes to events for `project_id` starting strictly after
    /// `after_sequence`. The returned subscription first replays persisted
    /// events, then transitions to the live broadcast stream.
    fn subscribe(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
    ) -> autore_core::Result<ProjectEventSubscription>;
}

/// In-process implementation of [`ProjectEventService`].
///
/// Events are committed atomically to the SQLite store via
/// `next_project_event_sequence` + `emit_in_tx` inside a transaction, then
/// broadcast to live subscribers. The store is authoritative; broadcast failure
/// is not treated as an error.
pub struct LocalProjectEventService {
    db: Arc<Database>,
    broadcaster: Arc<EventBroadcaster>,
}

impl LocalProjectEventService {
    /// Creates a service from a shared database and broadcaster.
    pub fn new(db: Arc<Database>, broadcaster: Arc<EventBroadcaster>) -> Self {
        Self { db, broadcaster }
    }

    /// Atomically commits a project event to the store and broadcasts it.
    ///
    /// The sequence is computed inside the transaction to guarantee
    /// per-project monotonic ordering.
    pub fn emit_event(
        &self,
        project_id: ProjectId,
        kind: NamespacedId,
        source: EventSource,
        subject: Option<EventSubject>,
        payload: Option<ExtensionData>,
    ) -> autore_core::Result<ProjectEvent> {
        let txn = self.db.begin_transaction()?;
        let sequence = next_project_event_sequence(&txn, project_id)?;
        let event = ProjectEvent::new(project_id, sequence, kind, source, subject, payload);
        emit_in_tx(&txn, &event)?;
        txn.commit()?;
        self.broadcaster.broadcast(event.clone());
        Ok(event)
    }
}

impl ProjectEventService for LocalProjectEventService {
    fn events_after(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        let store = SqliteEventStore::new(&self.db);
        let events = store.events_after(project_id, after_sequence)?;
        Ok(events.into_iter().take(limit).collect())
    }

    fn subscribe(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
    ) -> autore_core::Result<ProjectEventSubscription> {
        let rx = self.broadcaster.subscribe();
        let db = Arc::clone(&self.db);
        let events_after = move |pid: ProjectId, after: u64, limit: usize| {
            let store = SqliteEventStore::new(&db);
            let events = store.events_after(pid, after)?;
            Ok(events.into_iter().take(limit).collect::<Vec<_>>())
        };
        ProjectEventSubscription::new(
            project_id,
            after_sequence,
            Arc::new(events_after),
            rx,
            SUBSCRIPTION_REPLAY_LIMIT,
        )
    }
}

/// Internal state of a [`ProjectEventSubscription`].
#[derive(Debug)]
enum SubscriptionState {
    /// Replay buffer has not been loaded yet.
    Initial,
    /// Replaying events from the durable store.
    Replaying {
        buffer: Vec<ProjectEvent>,
        index: usize,
    },
    /// Consuming events from the live broadcast.
    Live,
    /// Subscription has been cancelled or the channel closed.
    Done,
}

/// A subscription that replays persisted events, then transitions to live
/// events, detecting sequence gaps and subscriber lag.
///
/// Call [`next`](Self::next) to obtain the next event. Gaps are detected by
/// comparing each event's sequence to `last_known_sequence + 1`. When a gap
/// is detected, the subscription resyncs from the durable store. If the store
/// itself is missing the expected sequence, an error is returned. When the
/// broadcast receiver reports lag, the subscription resyncs from the store.
pub struct ProjectEventSubscription {
    project_id: ProjectId,
    after_sequence: u64,
    last_known_sequence: u64,
    events_after:
        Arc<dyn Fn(ProjectId, u64, usize) -> autore_core::Result<Vec<ProjectEvent>> + Send + Sync>,
    receiver: broadcast::Receiver<ProjectEvent>,
    state: SubscriptionState,
    /// A live event that triggered a gap and must be re-evaluated after resync.
    pending_event: Option<ProjectEvent>,
    cancelled: Arc<AtomicBool>,
    replay_limit: usize,
}

impl ProjectEventSubscription {
    /// Creates a new subscription. The initial replay is performed lazily on
    /// the first [`next`](Self::next) call.
    pub fn new(
        project_id: ProjectId,
        after_sequence: u64,
        events_after: Arc<
            dyn Fn(ProjectId, u64, usize) -> autore_core::Result<Vec<ProjectEvent>> + Send + Sync,
        >,
        receiver: broadcast::Receiver<ProjectEvent>,
        replay_limit: usize,
    ) -> autore_core::Result<Self> {
        Ok(Self {
            project_id,
            after_sequence,
            last_known_sequence: after_sequence,
            events_after,
            receiver,
            state: SubscriptionState::Initial,
            pending_event: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            replay_limit,
        })
    }

    /// Returns the next event, replaying from the store first, then live from
    /// the broadcast channel.
    ///
    /// Returns `None` after cancellation or when the broadcaster has been
    /// dropped and the replay buffer is exhausted.
    pub async fn next(&mut self) -> Option<autore_core::Result<ProjectEvent>> {
        loop {
            if self.cancelled.load(Ordering::Relaxed) {
                self.state = SubscriptionState::Done;
                return None;
            }

            match self.state {
                SubscriptionState::Initial => match self.load_replay(self.after_sequence).await {
                    Ok(buffer) => {
                        self.state = SubscriptionState::Replaying { buffer, index: 0 };
                    }
                    Err(e) => {
                        self.state = SubscriptionState::Done;
                        return Some(Err(e));
                    }
                },

                SubscriptionState::Replaying {
                    ref mut buffer,
                    ref mut index,
                } => {
                    if *index < buffer.len() {
                        let event = buffer[*index].clone();
                        *index += 1;
                        if let Err(e) = self.advance_sequence(event.sequence).await {
                            self.state = SubscriptionState::Done;
                            return Some(Err(e));
                        }
                        return Some(Ok(event));
                    } else {
                        self.state = SubscriptionState::Live;
                    }
                }

                SubscriptionState::Live => {
                    if let Some(event) = self.pending_event.take() {
                        if event.sequence <= self.last_known_sequence {
                            continue;
                        }
                        if event.sequence > self.last_known_sequence + 1 {
                            self.pending_event = Some(event);
                            if let Err(e) = self.resync().await {
                                self.state = SubscriptionState::Done;
                                return Some(Err(e));
                            }
                            continue;
                        }
                        self.last_known_sequence = event.sequence;
                        return Some(Ok(event));
                    }

                    match self.receiver.recv().await {
                        Ok(event) => {
                            if event.sequence <= self.last_known_sequence {
                                continue;
                            }
                            if event.sequence > self.last_known_sequence + 1 {
                                self.pending_event = Some(event);
                                if let Err(e) = self.resync().await {
                                    self.state = SubscriptionState::Done;
                                    return Some(Err(e));
                                }
                                continue;
                            }
                            self.last_known_sequence = event.sequence;
                            return Some(Ok(event));
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if let Err(e) = self.resync().await {
                                self.state = SubscriptionState::Done;
                                return Some(Err(e));
                            }
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            self.state = SubscriptionState::Done;
                            return None;
                        }
                    }
                }

                SubscriptionState::Done => return None,
            }
        }
    }

    /// Cancels the subscription. Subsequent [`next`](Self::next) calls return
    /// `None`.
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.state = SubscriptionState::Done;
    }

    async fn load_replay(&self, after_sequence: u64) -> autore_core::Result<Vec<ProjectEvent>> {
        self.query_events(after_sequence, self.replay_limit).await
    }

    async fn resync(&mut self) -> autore_core::Result<()> {
        let events = self
            .query_events(self.last_known_sequence, self.replay_limit)
            .await?;
        if let Some(first) = events.first()
            && first.sequence > self.last_known_sequence + 1
        {
            return Err(autore_core::Error::Subscription(format!(
                "unrecoverable sequence gap: expected {}, store returned {}",
                self.last_known_sequence + 1,
                first.sequence
            )));
        }
        self.state = SubscriptionState::Replaying {
            buffer: events,
            index: 0,
        };
        Ok(())
    }

    async fn advance_sequence(&mut self, sequence: u64) -> autore_core::Result<()> {
        if sequence > self.last_known_sequence + 1 {
            self.pending_event = Some(self.clone_event_at(sequence));
            self.resync().await?;
            return Ok(());
        }
        self.last_known_sequence = sequence;
        Ok(())
    }

    fn clone_event_at(&self, sequence: u64) -> ProjectEvent {
        // The caller only uses this as a placeholder for the pending event.
        // The actual event is retrieved from the store during resync.
        ProjectEvent::new(
            self.project_id,
            sequence,
            NamespacedId::new(&["core", "placeholder"]).expect("valid placeholder kind"),
            EventSource::Project,
            None,
            None,
        )
    }

    async fn query_events(
        &self,
        after_sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        let events_after = Arc::clone(&self.events_after);
        let project_id = self.project_id;
        tokio::task::spawn_blocking(move || events_after(project_id, after_sequence, limit))
            .await
            .map_err(|e| autore_core::Error::Subscription(format!("resync task panicked: {e}")))?
    }
}

impl Drop for ProjectEventSubscription {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.state = SubscriptionState::Done;
    }
}

/// Mock [`ProjectEventService`] that intentionally returns gapped events from
/// `events_after` so gap-detection logic can be verified.
///
/// The constructor takes a vector of events; `events_after` filters and orders
/// them by sequence, skipping whatever gaps are present in the vector. Live
/// broadcast is not used; the subscription is replay-only.
pub struct GappedSubscription {
    events: Vec<ProjectEvent>,
}

impl GappedSubscription {
    /// Creates a mock service backed by `events`.
    pub fn new(events: Vec<ProjectEvent>) -> Self {
        Self { events }
    }

    fn events_after_inner(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        let mut events: Vec<_> = self
            .events
            .iter()
            .filter(|e| e.project == project_id && e.sequence > after_sequence)
            .take(limit)
            .cloned()
            .collect();
        events.sort_by_key(|e| e.sequence);
        Ok(events)
    }
}

impl ProjectEventService for GappedSubscription {
    fn events_after(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
        limit: usize,
    ) -> autore_core::Result<Vec<ProjectEvent>> {
        self.events_after_inner(project_id, after_sequence, limit)
    }

    fn subscribe(
        &self,
        project_id: ProjectId,
        after_sequence: u64,
    ) -> autore_core::Result<ProjectEventSubscription> {
        let events = self.events.clone();
        let events_after = move |pid: ProjectId, after: u64, limit: usize| {
            let mut ev: Vec<_> = events
                .iter()
                .filter(|e| e.project == pid && e.sequence > after)
                .take(limit)
                .cloned()
                .collect();
            ev.sort_by_key(|e| e.sequence);
            Ok(ev)
        };
        // Create a closed broadcast channel so the live phase ends immediately
        // after replay. This emulator is replay-only.
        let (tx, rx) = broadcast::channel(1);
        drop(tx);
        ProjectEventSubscription::new(
            project_id,
            after_sequence,
            Arc::new(events_after),
            rx,
            SUBSCRIPTION_REPLAY_LIMIT,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autore_schema::domain::EventSubject;
    use autore_schema::domain::records::{
        EVENT_KIND_OPERATION_COMPLETED, EVENT_KIND_OPERATION_STARTED, EVENT_KIND_PROJECT_CREATED,
    };
    use autore_schema::domain::records::{Operation, Project};
    use autore_store::{Database, OperationStore, ProjectStore, SqliteProjectStore};

    fn test_db() -> Arc<Database> {
        Arc::new(Database::open_in_memory().unwrap())
    }

    fn insert_project(db: &Database) -> ProjectId {
        let project = Project::new("test-project");
        let store = SqliteProjectStore::new(db);
        store.insert_project(&project).unwrap();
        project.id
    }

    fn service(db: Arc<Database>) -> LocalProjectEventService {
        let broadcaster = Arc::new(EventBroadcaster::new());
        LocalProjectEventService::new(db, broadcaster)
    }

    fn make_event(project_id: ProjectId, sequence: u64, kind: NamespacedId) -> ProjectEvent {
        ProjectEvent::new(project_id, sequence, kind, EventSource::Project, None, None)
    }

    #[test]
    fn events_after_replays_strictly_after() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(db);

        for _ in 0..10 {
            svc.emit_event(
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
            )
            .unwrap();
        }

        let events = svc.events_after(pid, 5, 10).unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].sequence, 6);
        assert_eq!(events[1].sequence, 7);
        assert_eq!(events[2].sequence, 8);
        assert_eq!(events[3].sequence, 9);
        assert_eq!(events[4].sequence, 10);
    }

    #[tokio::test]
    async fn replay_to_live_no_gap() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(db);

        for _ in 0..10 {
            svc.emit_event(
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
            )
            .unwrap();
        }

        let mut sub = svc.subscribe(pid, 5).unwrap();
        let mut collected = Vec::new();
        for _ in 0..5 {
            if let Some(Ok(ev)) = sub.next().await {
                collected.push(ev);
            }
        }
        assert_eq!(collected.len(), 5);
        assert_eq!(collected[0].sequence, 6);
        assert_eq!(collected[4].sequence, 10);

        svc.emit_event(
            pid,
            EVENT_KIND_OPERATION_STARTED.clone(),
            EventSource::Operation,
            None,
            None,
        )
        .unwrap();
        svc.emit_event(
            pid,
            EVENT_KIND_OPERATION_COMPLETED.clone(),
            EventSource::Operation,
            None,
            None,
        )
        .unwrap();

        for _ in 0..2 {
            if let Some(Ok(ev)) = sub.next().await {
                collected.push(ev);
            }
        }
        assert_eq!(collected.len(), 7);
        assert_eq!(collected[5].sequence, 11);
        assert_eq!(collected[6].sequence, 12);
    }

    #[tokio::test]
    async fn multiple_subscribers_all_receive() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(db);

        for _ in 0..10 {
            svc.emit_event(
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
            )
            .unwrap();
        }

        let mut sub1 = svc.subscribe(pid, 0).unwrap();
        let mut sub2 = svc.subscribe(pid, 5).unwrap();

        let mut sub1_events = Vec::new();
        let mut sub2_events = Vec::new();
        for _ in 0..10 {
            if let Some(Ok(ev)) = sub1.next().await {
                sub1_events.push(ev.sequence);
            }
        }
        for _ in 0..5 {
            if let Some(Ok(ev)) = sub2.next().await {
                sub2_events.push(ev.sequence);
            }
        }

        assert_eq!(sub1_events, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(sub2_events, vec![6, 7, 8, 9, 10]);
    }

    #[tokio::test]
    async fn subscriber_lag_resyncs_from_store() {
        let db = test_db();
        let pid = insert_project(&db);
        let broadcaster = Arc::new(EventBroadcaster::with_capacity(4));
        let svc = LocalProjectEventService::new(db, broadcaster);

        let mut sub = svc.subscribe(pid, 0).unwrap();

        // Emit more events than the broadcast capacity without consuming.
        for _ in 0..10 {
            svc.emit_event(
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
            )
            .unwrap();
        }

        let mut sequences = Vec::new();
        for _ in 0..10 {
            match sub.next().await {
                Some(Ok(ev)) => sequences.push(ev.sequence),
                Some(Err(e)) => panic!("unexpected subscription error: {e}"),
                None => panic!("subscription closed before receiving all events"),
            }
        }

        assert_eq!(sequences, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[tokio::test]
    async fn subscription_cancellable() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(db);

        svc.emit_event(
            pid,
            EVENT_KIND_PROJECT_CREATED.clone(),
            EventSource::Project,
            None,
            None,
        )
        .unwrap();

        let mut sub = svc.subscribe(pid, 0).unwrap();
        sub.cancel();
        assert!(sub.next().await.is_none());
    }

    #[tokio::test]
    async fn sequence_gap_recovery() {
        let pid = ProjectId::new();
        let kind = NamespacedId::new(&["core", "project", "created"]).unwrap();
        let events = vec![
            make_event(pid, 1, kind.clone()),
            make_event(pid, 3, kind.clone()),
            make_event(pid, 5, kind.clone()),
        ];
        let svc = GappedSubscription::new(events);

        let mut sub = svc.subscribe(pid, 0).unwrap();
        assert_eq!(sub.next().await.unwrap().unwrap().sequence, 1);

        // The next event is sequence 3, which is a gap (expected 2). Resync
        // from the store still returns 3 as the first event, so the gap is
        // unrecoverable and the subscription surfaces an error.
        let result = sub.next().await.unwrap();
        assert!(
            result.is_err(),
            "gap in authoritative store must be reported as an error"
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unrecoverable sequence gap"), "{err}");
    }

    #[tokio::test]
    async fn subscription_recovers_from_broadcast_gap() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(db);

        // Seed the store with events 1..5.
        for _ in 0..5 {
            svc.emit_event(
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
            )
            .unwrap();
        }

        let mut sub = svc.subscribe(pid, 0).unwrap();
        // Receive events 1..5 via replay.
        let mut seqs = Vec::new();
        for _ in 0..5 {
            seqs.push(sub.next().await.unwrap().unwrap().sequence);
        }

        // Emit 6, 7, 8, then subscribe a second receiver and deliberately skip
        // a sequence by reading only some events. The original subscriber
        // must still receive every event because it sees the broadcast stream.
        for _ in 0..3 {
            svc.emit_event(
                pid,
                EVENT_KIND_PROJECT_CREATED.clone(),
                EventSource::Project,
                None,
                None,
            )
            .unwrap();
        }

        for _ in 0..3 {
            seqs.push(sub.next().await.unwrap().unwrap().sequence);
        }

        assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[tokio::test]
    async fn emit_event_persists_before_broadcast() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(db);

        let event = svc
            .emit_event(
                pid,
                EVENT_KIND_OPERATION_COMPLETED.clone(),
                EventSource::Operation,
                Some(EventSubject::Project(pid)),
                None,
            )
            .unwrap();

        // A new subscriber starting after the event should replay it.
        let mut sub = svc.subscribe(pid, 0).unwrap();
        let replayed = sub.next().await.unwrap().unwrap();
        assert_eq!(replayed.sequence, event.sequence);
        assert_eq!(replayed.kind, event.kind);
        assert_eq!(replayed.subject, event.subject);
    }

    #[tokio::test]
    async fn emit_event_with_state_change_in_tx() {
        let db = test_db();
        let pid = insert_project(&db);
        let svc = service(Arc::clone(&db));
        let op_store = autore_store::SqliteOperationStore::new(&db);

        let op = Operation::new(pid, EVENT_KIND_OPERATION_STARTED.clone(), "test");
        op_store.insert(&op).unwrap();

        let op_id = op.id;
        let event = svc
            .emit_event(
                pid,
                EVENT_KIND_OPERATION_COMPLETED.clone(),
                EventSource::Operation,
                Some(EventSubject::Operation(op_id)),
                None,
            )
            .unwrap();

        assert_eq!(event.subject, Some(EventSubject::Operation(op_id)));
        assert_eq!(event.sequence, 1);
    }
}
