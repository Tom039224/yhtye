//! A fake Tailscale for the remote access tests: this PC's "Tailscale address"
//! is 127.0.0.1 and every peer belongs to one chosen user.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use yhtye_core::remote::{Tailscale, TailscaleError, TailscalePeer, TailscaleSelf};

/// The Tailscale user of the PC.
pub const ME: u64 = 1;
pub const SOMEONE_ELSE: u64 = 2;

#[derive(Debug)]
pub struct FakeTailscale {
    /// `Err`: Tailscale is "stopped".
    pub up: Mutex<bool>,
    /// The user every peer belongs to.
    peer_user: AtomicU64,
    /// This PC is a tagged node.
    pub pc_tagged: AtomicBool,
    /// Every peer is a tagged node.
    peers_tagged: bool,
    /// `whois` lookups so far.
    whois_calls: AtomicUsize,
}

impl FakeTailscale {
    pub fn new() -> Arc<Self> {
        Self::with_peer_user(ME)
    }

    pub fn with_peer_user(peer_user: u64) -> Arc<Self> {
        Arc::new(Self {
            up: Mutex::new(true),
            peer_user: AtomicU64::new(peer_user),
            pc_tagged: AtomicBool::new(false),
            peers_tagged: false,
            whois_calls: AtomicUsize::new(0),
        })
    }

    /// Every peer is tagged, whoever the user is.
    pub fn with_tagged_peers(peer_user: u64) -> Arc<Self> {
        Arc::new(Self {
            up: Mutex::new(true),
            peer_user: AtomicU64::new(peer_user),
            pc_tagged: AtomicBool::new(false),
            peers_tagged: true,
            whois_calls: AtomicUsize::new(0),
        })
    }

    /// From now on (once the kept answers expire) every peer belongs to `user`.
    pub fn set_peer_user(&self, user: u64) {
        self.peer_user.store(user, Ordering::SeqCst);
    }

    pub fn whois_calls(&self) -> usize {
        self.whois_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Tailscale for FakeTailscale {
    async fn status(&self) -> Result<TailscaleSelf, TailscaleError> {
        if !*self.up.lock().unwrap() {
            return Err(TailscaleError(
                "Tailscale is not running (state: Stopped)".into(),
            ));
        }
        Ok(TailscaleSelf {
            addresses: vec!["127.0.0.1".parse().unwrap()],
            dns_name: Some("pc.test.ts.net".into()),
            user_id: ME,
            tagged: self.pc_tagged.load(Ordering::SeqCst),
        })
    }

    async fn whois(&self, _ip: IpAddr) -> Result<TailscalePeer, TailscaleError> {
        self.whois_calls.fetch_add(1, Ordering::SeqCst);
        Ok(TailscalePeer {
            user_id: self.peer_user.load(Ordering::SeqCst),
            tagged: self.peers_tagged,
        })
    }
}

/// A port nothing listens on at the moment.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
}
