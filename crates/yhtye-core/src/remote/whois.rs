//! Keeps `tailscale whois` from being run for every request (`core-design.md`
//! §18.4): answers are kept per peer address for a short time, and only a few
//! lookups run at once.
//!
//! Both answers are kept (this PC's user and another user): a node's user does
//! not change in a minute, and keeping the refusal is what stops another
//! user's device from making this PC start a process per request. Failures
//! (Tailscale briefly unreachable, a timeout) are not kept, so they are retried.
//!
//! A lookup over the limit waits for a free slot rather than being refused
//! with 503: each lookup ends within the CLI timeout, waiting costs no
//! process, and requests from an address that is being looked up find the
//! answer cached once they get a slot, so a flood from other devices only
//! delays this user's device instead of turning it away.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::time::Instant;

use super::tailscale::{Tailscale, TailscaleError};

/// How long an answer is kept.
pub const WHOIS_TTL: Duration = Duration::from_secs(60);
/// Most addresses kept at once.
pub const WHOIS_CACHE_CAPACITY: usize = 256;
/// Most `tailscale whois` processes at once.
pub const WHOIS_CONCURRENCY: usize = 4;

/// Answers of `whois` by peer address.
#[derive(Debug)]
pub struct WhoisCache {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<HashMap<IpAddr, Entry>>,
    slots: Semaphore,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    user: u64,
    expires_at: Instant,
}

impl Default for WhoisCache {
    fn default() -> Self {
        Self::new(WHOIS_TTL, WHOIS_CACHE_CAPACITY, WHOIS_CONCURRENCY)
    }
}

impl WhoisCache {
    #[must_use]
    pub fn new(ttl: Duration, capacity: usize, concurrency: usize) -> Self {
        Self {
            ttl,
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
            slots: Semaphore::new(concurrency.max(1)),
        }
    }

    /// The Tailscale user of `ip`: kept, or asked of `tailscale` (waiting for
    /// a free slot).
    pub async fn user_of(
        &self,
        tailscale: &dyn Tailscale,
        ip: IpAddr,
    ) -> Result<u64, TailscaleError> {
        if let Some(user) = self.cached(ip) {
            return Ok(user);
        }
        let _slot = self
            .slots
            .acquire()
            .await
            .map_err(|_| TailscaleError("the Tailscale lookup was stopped".into()))?;
        // Another request from the same address may have asked meanwhile.
        if let Some(user) = self.cached(ip) {
            return Ok(user);
        }
        let user = tailscale.whois(ip).await?;
        self.insert(ip, user);
        Ok(user)
    }

    /// Forgets every answer (the server stopped).
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn cached(&self, ip: IpAddr) -> Option<u64> {
        let mut entries = self.lock();
        let entry = *entries.get(&ip)?;
        if Instant::now() < entry.expires_at {
            return Some(entry.user);
        }
        entries.remove(&ip);
        None
    }

    fn insert(&self, ip: IpAddr, user: u64) {
        let now = Instant::now();
        let mut entries = self.lock();
        if entries.len() >= self.capacity && !entries.contains_key(&ip) {
            entries.retain(|_, e| now < e.expires_at);
            if entries.len() >= self.capacity
                && let Some(oldest) = entries
                    .iter()
                    .min_by_key(|(_, e)| e.expires_at)
                    .map(|(ip, _)| *ip)
            {
                entries.remove(&oldest);
            }
        }
        entries.insert(
            ip,
            Entry {
                user,
                expires_at: now + self.ttl,
            },
        );
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<IpAddr, Entry>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::remote::TailscaleSelf;

    /// Counts lookups; every address belongs to `user` unless `failing`.
    #[derive(Debug, Default)]
    struct Counting {
        user: u64,
        failing: AtomicBool,
        calls: AtomicUsize,
        running: AtomicUsize,
        most_running: AtomicUsize,
        delay: Duration,
    }

    #[async_trait]
    impl Tailscale for Counting {
        async fn status(&self) -> Result<TailscaleSelf, TailscaleError> {
            Err(TailscaleError("not used".into()))
        }

        async fn whois(&self, _ip: IpAddr) -> Result<u64, TailscaleError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_running.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.running.fetch_sub(1, Ordering::SeqCst);
            if self.failing.load(Ordering::SeqCst) {
                return Err(TailscaleError("tailscaled is not answering".into()));
            }
            Ok(self.user)
        }
    }

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([100, 64, 0, n])
    }

    fn counting(user: u64) -> Counting {
        Counting {
            user,
            ..Counting::default()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn repeated_requests_from_one_address_ask_once_until_the_answer_expires() {
        let ts = counting(7);
        let cache = WhoisCache::new(Duration::from_secs(60), 16, 4);
        for _ in 0..10 {
            assert_eq!(cache.user_of(&ts, ip(1)).await, Ok(7));
        }
        assert_eq!(ts.calls.load(Ordering::SeqCst), 1);
        tokio::time::advance(Duration::from_secs(59)).await;
        assert_eq!(cache.user_of(&ts, ip(1)).await, Ok(7));
        assert_eq!(ts.calls.load(Ordering::SeqCst), 1, "still fresh");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(cache.user_of(&ts, ip(1)).await, Ok(7));
        assert_eq!(
            ts.calls.load(Ordering::SeqCst),
            2,
            "asked again after expiry"
        );
        assert_eq!(cache.user_of(&ts, ip(2)).await, Ok(7));
        assert_eq!(ts.calls.load(Ordering::SeqCst), 3, "per address");
    }

    #[tokio::test(start_paused = true)]
    async fn another_users_answer_is_kept_too() {
        let ts = counting(2);
        let cache = WhoisCache::default();
        for _ in 0..5 {
            assert_eq!(cache.user_of(&ts, ip(1)).await, Ok(2));
        }
        assert_eq!(ts.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn failures_are_not_kept() {
        let ts = counting(7);
        ts.failing.store(true, Ordering::SeqCst);
        let cache = WhoisCache::default();
        assert!(cache.user_of(&ts, ip(1)).await.is_err());
        assert!(cache.user_of(&ts, ip(1)).await.is_err());
        assert_eq!(ts.calls.load(Ordering::SeqCst), 2);
        ts.failing.store(false, Ordering::SeqCst);
        assert_eq!(cache.user_of(&ts, ip(1)).await, Ok(7));
        assert_eq!(cache.user_of(&ts, ip(1)).await, Ok(7));
        assert_eq!(ts.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn at_most_the_limit_run_at_once_and_waiters_on_one_address_share_the_answer() {
        let ts = Arc::new(Counting {
            user: 7,
            delay: Duration::from_secs(1),
            ..Counting::default()
        });
        let cache = Arc::new(WhoisCache::new(Duration::from_secs(60), 64, 4));
        let lookups: Vec<_> = (0..20u8)
            .map(|n| {
                let (ts, cache) = (Arc::clone(&ts), Arc::clone(&cache));
                tokio::spawn(async move { cache.user_of(&*ts, ip(n)).await })
            })
            .collect();
        for l in lookups {
            assert_eq!(l.await.expect("joined"), Ok(7));
        }
        assert_eq!(ts.calls.load(Ordering::SeqCst), 20);
        assert_eq!(ts.most_running.load(Ordering::SeqCst), 4);

        let same: Vec<_> = (0..20)
            .map(|_| {
                let (ts, cache) = (Arc::clone(&ts), Arc::clone(&cache));
                tokio::spawn(async move { cache.user_of(&*ts, ip(200)).await })
            })
            .collect();
        for l in same {
            assert_eq!(l.await.expect("joined"), Ok(7));
        }
        assert!(
            ts.calls.load(Ordering::SeqCst) <= 20 + 4,
            "at most one lookup per slot for one address"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_cache_is_bounded_and_forgets_the_oldest() {
        let ts = counting(7);
        let cache = WhoisCache::new(Duration::from_secs(60), 3, 4);
        for n in 1..=3 {
            cache.user_of(&ts, ip(n)).await.expect("user");
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        cache.user_of(&ts, ip(4)).await.expect("user");
        assert_eq!(cache.lock().len(), 3);
        assert!(cache.cached(ip(1)).is_none(), "the oldest went");
        assert!(cache.cached(ip(2)).is_some() && cache.cached(ip(4)).is_some());
        cache.clear();
        assert!(cache.lock().is_empty());
    }
}
