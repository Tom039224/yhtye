//! Numbers and timestamps [`ApiEvent`]s and sends them to the subscriber.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;

use crate::api::{ApiEvent, ApiEventBody};

/// Cheap to clone; all clones share one sequence. Used only on the runtime loop
/// task, so sequence order equals send order.
#[derive(Clone)]
pub(super) struct Emitter {
    project: Arc<str>,
    seq: Arc<AtomicU64>,
    tx: mpsc::UnboundedSender<ApiEvent>,
}

impl Emitter {
    pub(super) fn new(project: &str, tx: mpsc::UnboundedSender<ApiEvent>) -> Self {
        Self {
            project: project.into(),
            seq: Arc::new(AtomicU64::new(0)),
            tx,
        }
    }

    /// Sequence number of the last event sent.
    pub(super) fn seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    pub(super) fn send(&self, body: ApiEventBody) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let event = ApiEvent {
            seq,
            ts_ms,
            project: self.project.to_string(),
            body,
        };
        // Nobody listening is fine (e.g. the subscriber was dropped).
        let _ = self.tx.send(event);
    }
}
