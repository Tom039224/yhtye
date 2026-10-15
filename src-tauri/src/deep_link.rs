//! `yhtye://pair?…` links (`core-design.md` §9, §18.9): a click on the PC's
//! QR code / link (or a link pasted into the browser) starts `yhtye <link>` or
//! reaches the running app; the app shows the person that host. **Nothing
//! pairs by itself**: pairing spends the host's one-time code and registers
//! this device there, so the webview opens the settings' "接続先" with the
//! link in the box, shows the host and asks for a name
//! (`src/ui/HostsSettingsSection.tsx`), and only when the person presses
//! "接続" calls `yhtye_remote_add`.
//!
//! Where links come from:
//! - the one the app was started with (`DeepLink::get_current`, read at startup);
//! - while it runs, a second `yhtye <link>` (the OS starts one per click on
//!   Linux and Windows) hands its arguments over to the first one
//!   (`tauri-plugin-single-instance`, which forwards them to the deep-link
//!   plugin), and macOS reports them to the running app.
//!
//! A link reaches the webview as the event `yhtye://pair-link` (its payload is
//! the link). One that arrives before the webview is listening (the app was
//! started by the link) waits in [`PairLinks`] until the webview asks for them
//! with `yhtye_pair_links_ready`, which it does once its listener is in place.

use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_deep_link::DeepLinkExt;
use yhtye_core::remote_client::looks_like_pairing_uri;

/// Event name of a pairing link (`src/api/remoteHosts.ts`).
pub const PAIR_LINK_EVENT: &str = "yhtye://pair-link";

/// Links that wait for the webview, and whether it listens yet.
#[derive(Debug, Default)]
pub struct PairLinks {
    state: Mutex<Waiting>,
}

#[derive(Debug, Default)]
struct Waiting {
    /// The webview has taken the waiting links: later ones are emitted.
    ready: bool,
    links: Vec<String>,
}

impl PairLinks {
    fn lock(&self) -> MutexGuard<'_, Waiting> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A link arrived. `true`: the webview listens, emit it now; `false`: it
    /// was kept for [`ready`](Self::ready) (the same link twice is kept once).
    pub fn arrived(&self, link: &str) -> bool {
        let mut state = self.lock();
        if state.ready {
            return true;
        }
        if !state.links.iter().any(|l| l == link) {
            state.links.push(link.to_string());
        }
        false
    }

    /// The webview listens now: the links that waited, oldest first, and from
    /// now on [`arrived`](Self::arrived) says to emit. Calling it again (the
    /// page was reloaded) returns nothing new.
    pub fn ready(&self) -> Vec<String> {
        let mut state = self.lock();
        state.ready = true;
        std::mem::take(&mut state.links)
    }
}

/// The pairing links among `urls` (another `yhtye://…` link is not for this
/// app to act on).
pub fn pairing_links<U: AsRef<str>>(urls: &[U]) -> Vec<String> {
    urls.iter()
        .map(|u| u.as_ref())
        .filter(|u| looks_like_pairing_uri(u))
        .map(str::to_string)
        .collect()
}

/// A link arrived: to the webview, or kept for it, and the window comes forward.
pub fn deliver<R: Runtime>(app: &AppHandle<R>, links: &PairLinks, link: &str) {
    // The link holds a pairing code: it is not logged.
    tracing::info!("a pairing link arrived");
    if links.arrived(link) {
        if let Err(e) = app.emit(PAIR_LINK_EVENT, link) {
            tracing::warn!("could not pass a pairing link on: {e}");
        }
    }
    focus_main_window(app);
}

/// Brings the window to the front (a second launch or a link wants the person's eyes).
pub fn focus_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    // Failing to do any of this is no reason to do less of it.
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Listens for links: the one the app was started with, and those that come
/// while it runs. In a debug build the scheme is registered with the OS too
/// (Windows, Linux), so that a link can be tried without installing the app;
/// a release build gets it from the installer / the `.desktop` file.
pub fn listen<R: Runtime>(app: &tauri::App<R>) {
    let handle = app.handle().clone();
    let deep_link = app.deep_link();
    if cfg!(debug_assertions) {
        if let Err(e) = deep_link.register_all() {
            tracing::debug!("could not register the yhtye:// scheme: {e}");
        }
    }
    let links = app.state::<PairLinks>();
    if let Ok(Some(urls)) = deep_link.get_current() {
        let urls: Vec<&str> = urls.iter().map(|u| u.as_str()).collect();
        for link in pairing_links(&urls) {
            links.arrived(&link);
        }
    }
    deep_link.on_open_url(move |event| {
        let urls: Vec<String> = event.urls().iter().map(ToString::to_string).collect();
        let links = handle.state::<PairLinks>();
        for link in pairing_links(&urls) {
            deliver(&handle, &links, &link);
        }
    });
}

/// The webview listens for `yhtye://pair-link` now: returns the links that
/// arrived before (at startup).
#[tauri::command]
pub fn yhtye_pair_links_ready(links: tauri::State<'_, PairLinks>) -> Vec<String> {
    links.ready()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files that tell the OS and Tauri about the scheme, which tests cannot run.
    const TAURI_CONF: &str = include_str!("../tauri.conf.json");
    const CAPABILITY: &str = include_str!("../capabilities/default.json");
    const PKGBUILD: &str = include_str!("../../packaging/arch/PKGBUILD");

    const LINK: &str = "yhtye://pair?v=1&ip=100.64.0.1&port=1423&code=ABCD2345&sid=s1";

    #[test]
    fn links_wait_until_the_webview_listens_and_are_then_emitted() {
        let links = PairLinks::default();
        assert!(!links.arrived(LINK), "kept: nobody listens yet");
        assert!(!links.arrived("yhtye://pair?v=1&ip=100.64.0.2"));
        assert_eq!(
            links.ready(),
            [LINK, "yhtye://pair?v=1&ip=100.64.0.2"],
            "oldest first"
        );
        assert!(links.arrived(LINK), "emit now: the webview listens");
        assert_eq!(links.ready(), Vec::<String>::new(), "taken once");
    }

    #[test]
    fn the_same_waiting_link_is_kept_once() {
        let links = PairLinks::default();
        links.arrived(LINK);
        links.arrived(LINK);
        assert_eq!(links.ready(), [LINK]);
    }

    #[test]
    fn only_pairing_links_are_passed_on() {
        let urls = [
            LINK,
            "yhtye://other?x=1",
            "https://example.com/yhtye://pair",
            "YHTYE://pair?v=1",
        ];
        assert_eq!(pairing_links(&urls), [LINK, "YHTYE://pair?v=1"]);
        assert!(pairing_links::<&str>(&[]).is_empty());
    }

    #[test]
    fn the_scheme_is_declared_where_the_os_and_tauri_read_it() {
        let conf: serde_json::Value = serde_json::from_str(TAURI_CONF).expect("tauri.conf.json");
        assert_eq!(
            conf["plugins"]["deep-link"]["desktop"]["schemes"],
            serde_json::json!(["yhtye"])
        );
        assert!(CAPABILITY.contains("\"deep-link:default\""));
        // The Linux package: the `.desktop` file takes the link as an argument and claims the scheme.
        assert!(PKGBUILD.contains("Exec=yhtye %u"));
        assert!(PKGBUILD.contains("MimeType=x-scheme-handler/yhtye;"));
    }
}
