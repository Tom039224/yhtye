//! Publishing [`ApiEvent`]s: numbering, persisting and sending them.
//!
//! [`Emitter`] (cloned into the session manager) only buffers event bodies. The
//! loop's [`Publisher`] drains the buffer after every loop turn: durable events
//! are numbered from the store's last `seq` and written, then everything is sent
//! to the subscriber. So `ApiEvent.seq` equals the row in the event log, and a
//! subscriber never sees a durable event that is not on disk. Domain transitions
//! go through [`Publisher::commit`], which writes their events together with the
//! current-state tables in one transaction.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;

use crate::api::{ApiEvent, ApiEventBody};
use crate::domain::{DomainEvent, State};
use crate::store::{Store, StoreError};

type Pending = Vec<(u64, ApiEventBody)>;

/// Buffers event bodies for the [`Publisher`]. Cheap to clone.
#[derive(Clone, Default)]
pub(super) struct Emitter {
    pending: Arc<Mutex<Pending>>,
}

impl Emitter {
    pub(super) fn send(&self, body: ApiEventBody) {
        self.lock().push((now_ms(), body));
    }

    fn take(&self) -> Pending {
        std::mem::take(&mut *self.lock())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Owned by the runtime loop: numbers, stores and sends events.
pub(super) struct Publisher {
    project: Arc<str>,
    store: Store,
    /// `seq` of the last durable event.
    seq: u64,
    tx: mpsc::UnboundedSender<ApiEvent>,
    emitter: Emitter,
}

impl Publisher {
    pub(super) async fn new(
        project: &str,
        store: Store,
        tx: mpsc::UnboundedSender<ApiEvent>,
    ) -> Result<Self, StoreError> {
        let seq = store.last_seq(project).await?;
        Ok(Self {
            project: project.into(),
            store,
            seq,
            tx,
            emitter: Emitter::default(),
        })
    }

    pub(super) fn emitter(&self) -> Emitter {
        self.emitter.clone()
    }

    pub(super) fn store(&self) -> &Store {
        &self.store
    }

    /// `seq` of the last durable event.
    pub(super) fn seq(&self) -> u64 {
        self.seq
    }

    /// Stores and sends everything buffered. If storing fails, the numbers are
    /// taken back and the events are sent as live (shown, but not part of the
    /// log), so a subscriber never sees a durable `seq` that is not on disk.
    pub(super) async fn flush(&mut self) {
        let pending = self.emitter.take();
        if pending.is_empty() {
            return;
        }
        let seq = self.seq;
        let mut events = self.number(pending);
        let durable: Vec<ApiEvent> = events.iter().filter(|e| !e.live).cloned().collect();
        if let Err(e) = self.store.append(&durable).await {
            tracing::error!("storing {} events failed; sent as live: {e}", durable.len());
            self.seq = seq;
            for event in &mut events {
                event.seq = seq;
                event.live = true;
            }
        }
        self.send_all(events);
    }

    /// Flushes the buffer (in its own transaction), then stores a domain
    /// transition (its events and the state change `before` → `after`)
    /// atomically and sends its events. On error nothing of the transition is
    /// stored or sent.
    pub(super) async fn commit(
        &mut self,
        before: &State,
        after: &State,
        events: Vec<DomainEvent>,
    ) -> Result<(), StoreError> {
        self.flush().await;
        let seq = self.seq;
        let ts = now_ms();
        let bodies = events
            .into_iter()
            .map(|event| (ts, ApiEventBody::Domain { event }))
            .collect();
        let events = self.number(bodies);
        if let Err(e) = self.store.commit(&events, before, after).await {
            self.seq = seq;
            return Err(e);
        }
        self.send_all(events);
        Ok(())
    }

    fn number(&mut self, bodies: Pending) -> Vec<ApiEvent> {
        bodies
            .into_iter()
            .map(|(ts_ms, body)| {
                let live = body.is_live();
                if !live {
                    self.seq += 1;
                }
                ApiEvent {
                    seq: self.seq,
                    ts_ms,
                    project: self.project.to_string(),
                    live,
                    body,
                }
            })
            .collect()
    }

    fn send_all(&self, events: Vec<ApiEvent>) {
        for event in events {
            // Nobody listening is fine (e.g. the subscriber was dropped).
            let _ = self.tx.send(event);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
