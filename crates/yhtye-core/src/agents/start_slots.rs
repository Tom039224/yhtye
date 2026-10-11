//! The limit on agent processes being started at the same time (T-48). One
//! [`StartSlots`] lives in the [`super::AgentCatalog`], so the limit holds for
//! every open project together; each project takes its own [`StartScope`] to be
//! able to drop the starts still waiting for it on shutdown without touching
//! the other projects'.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Most agent processes that are being started at the same time. A start (the
/// handshake, `session/load`, the mode and config calls) takes seconds and often
/// much longer for a restored session; many at once made them time out, so the
/// ones beyond this wait for a start to finish. Running sessions are not
/// counted.
pub const MAX_CONCURRENT_STARTS: usize = 4;

/// The start permits, shared by every project. Cheap to clone.
#[derive(Debug, Clone)]
pub struct StartSlots {
    slots: Arc<Semaphore>,
}

impl StartSlots {
    #[must_use]
    pub fn new(max: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(max)),
        }
    }

    /// A project's view of the permits: the same permits, but its own
    /// [`StartScope::close`].
    #[must_use]
    pub fn scope(&self) -> StartScope {
        StartScope {
            slots: self.slots.clone(),
            closed: CancellationToken::new(),
        }
    }
}

impl Default for StartSlots {
    fn default() -> Self {
        Self::new(MAX_CONCURRENT_STARTS)
    }
}

/// One project's access to the shared [`StartSlots`].
#[derive(Debug, Clone)]
pub struct StartScope {
    slots: Arc<Semaphore>,
    /// Set on this project's shutdown.
    closed: CancellationToken,
}

impl StartScope {
    /// Runs `start` once a permit is free and holds the permit until it is done.
    /// `None`: the scope is closed (the project is shutting down), `start` never
    /// ran.
    pub async fn run<T>(&self, start: impl Future<Output = T>) -> Option<T> {
        let _slot = tokio::select! {
            biased;
            () = self.closed.cancelled() => return None,
            slot = self.slots.acquire() => slot.ok()?,
        };
        Some(start.await)
    }

    /// Fails the starts of this scope that are still waiting for a permit, and
    /// every later one. Starts that hold a permit finish; other scopes are not
    /// affected.
    pub fn close(&self) {
        self.closed.cancel();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    /// Runs `starts` starts on each scope at once; returns the most that ran at
    /// the same time.
    async fn most_running(scopes: &[StartScope], starts: usize) -> usize {
        let (running, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let mut tasks = Vec::new();
        for scope in scopes {
            for _ in 0..starts {
                let (scope, running, most) = (scope.clone(), running.clone(), most.clone());
                tasks.push(tokio::spawn(async move {
                    scope
                        .run(async {
                            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                            most.fetch_max(now, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            running.fetch_sub(1, Ordering::SeqCst);
                        })
                        .await
                }));
            }
        }
        for task in tasks {
            assert!(task.await.expect("joined").is_some());
        }
        most.load(Ordering::SeqCst)
    }

    #[tokio::test]
    async fn only_a_few_starts_run_at_a_time_and_all_of_them_finish() {
        let scope = StartSlots::default().scope();
        let most = most_running(&[scope], MAX_CONCURRENT_STARTS * 3).await;
        assert_eq!(most, MAX_CONCURRENT_STARTS);
    }

    #[tokio::test]
    async fn the_limit_holds_across_scopes_instead_of_multiplying() {
        let slots = StartSlots::default();
        let scopes: Vec<StartScope> = (0..3).map(|_| slots.scope()).collect();
        let most = most_running(&scopes, MAX_CONCURRENT_STARTS * 2).await;
        assert_eq!(most, MAX_CONCURRENT_STARTS, "not once per project");
    }

    #[tokio::test]
    async fn a_start_waiting_for_a_slot_does_not_run_once_its_scope_is_closed() {
        let scope = StartSlots::new(0).scope();
        let waiting = {
            let scope = scope.clone();
            tokio::spawn(async move { scope.run(async { 1 }).await })
        };
        tokio::task::yield_now().await;
        scope.close();
        assert_eq!(waiting.await.expect("joined"), None);
        assert_eq!(scope.run(async { 2 }).await, None, "nor a later start");
    }

    #[tokio::test]
    async fn closing_one_scope_leaves_the_starts_of_the_others_waiting() {
        let slots = StartSlots::new(1);
        let (closing, other) = (slots.scope(), slots.scope());
        let (release, held) = tokio::sync::oneshot::channel::<()>();
        let holder = {
            let other = other.clone();
            tokio::spawn(async move { other.run(async { held.await.ok() }).await })
        };
        tokio::task::yield_now().await;
        let waiting_closed = {
            let closing = closing.clone();
            tokio::spawn(async move { closing.run(async { 1 }).await })
        };
        let waiting_other = {
            let other = other.clone();
            tokio::spawn(async move { other.run(async { 2 }).await })
        };
        tokio::task::yield_now().await;

        closing.close();
        assert_eq!(waiting_closed.await.expect("joined"), None);
        assert!(!waiting_other.is_finished(), "still waiting for the permit");

        release.send(()).expect("holder is waiting");
        assert!(holder.await.expect("joined").is_some());
        assert_eq!(waiting_other.await.expect("joined"), Some(2));
    }
}
