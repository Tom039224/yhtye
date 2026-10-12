//! The bridge runs the same session as the remote server (`yhtye_core::ws`), so
//! it pings its clients and drops one that stays silent (T-75): a script that
//! vanished does not keep a session, with its queues, for hours. Browsers and
//! tungstenite answer pings by themselves.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{Bridge, TIMEOUT, TOKEN, connect};
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use yhtye_core::runtime::{Core, CoreConfig};
use yhtye_core::secrets::MemoryBackend;
use yhtye_core::ws::{IDLE_TIMEOUT, PING_INTERVAL};

/// Moves the (paused) clock forward `total`, a second at a time, letting real
/// time pass between the steps (a blocking task keeps the paused clock from
/// jumping meanwhile) so the sockets' data moves.
async fn advance_secs(total: u64) {
    for _ in 0..total {
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(3)))
            .await
            .expect("slept");
    }
}

#[tokio::test]
async fn the_bridge_pings_clients_and_drops_a_silent_one() {
    let dir = tempfile::tempdir().expect("dir");
    let mut cfg = CoreConfig::claude_code(dir.path(), "haiku");
    cfg.usage = None;
    cfg.secret_backend = Arc::new(MemoryBackend::default());
    let core = Core::start(cfg).await.expect("core");
    let bridge = Bridge::start(&core).await;

    let mut silent = connect(bridge.port, TOKEN, None).await.expect("silent");
    let mut healthy = connect(bridge.port, TOKEN, None).await.expect("healthy");
    // Reading answers the server's pings.
    let healthy_pings = Arc::new(AtomicUsize::new(0));
    let reader = tokio::spawn({
        let healthy_pings = Arc::clone(&healthy_pings);
        async move {
            while let Some(Ok(msg)) = healthy.next().await {
                if matches!(msg, Message::Ping(_)) {
                    healthy_pings.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    });

    tokio::time::pause();
    advance_secs(IDLE_TIMEOUT.as_secs() * 2).await;
    tokio::time::resume();

    // Never read until now: the pings were queued in order, and the server has
    // closed the connection since (answering the first ping may already fail).
    let mut pings = 0;
    tokio::time::timeout(TIMEOUT, async {
        while let Some(Ok(msg)) = silent.next().await {
            pings += usize::from(matches!(msg, Message::Ping(_)));
        }
    })
    .await
    .expect("the silent client's connection is closed");
    assert!(pings >= 2, "pinged every {PING_INTERVAL:?}: {pings}");

    assert!(
        !reader.is_finished(),
        "the client that answers is still connected"
    );
    assert!(
        healthy_pings.load(Ordering::SeqCst) >= 4,
        "and was pinged all along"
    );
    // Both ends of the sockets go before the bridge stops.
    reader.abort();
    drop(silent);
    bridge.stop().await;
    core.shutdown().await;
}
