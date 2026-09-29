//! WebKitGTK's DMA-BUF renderer crashes on Wayland with NVIDIA's proprietary
//! driver (Gdk Error 71, README "Wayland + NVIDIA"). The app sets
//! `WEBKIT_DISABLE_DMABUF_RENDERER=1` at startup only in that environment, so
//! other machines keep the fast GPU path (decided in Stage 7a, docs/PLAN.md).

use std::path::Path;

pub const VAR: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// What the decision depends on (read from the process, injected in tests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    /// The user already set the variable (to anything): leave it alone.
    pub already_set: bool,
    pub wayland_display: Option<String>,
    pub xdg_session_type: Option<String>,
    /// `GDK_BACKEND`: `x11` forces X11 even in a Wayland session.
    pub gdk_backend: Option<String>,
    /// The NVIDIA kernel driver is loaded.
    pub nvidia: bool,
}

impl Probe {
    /// Reads the environment and two cheap filesystem checks (no processes run).
    pub fn from_system() -> Self {
        let var = |k: &str| std::env::var(k).ok();
        Self {
            already_set: std::env::var_os(VAR).is_some(),
            wayland_display: var("WAYLAND_DISPLAY"),
            xdg_session_type: var("XDG_SESSION_TYPE"),
            gdk_backend: var("GDK_BACKEND"),
            nvidia: Path::new("/proc/driver/nvidia").exists()
                || Path::new("/sys/module/nvidia").exists(),
        }
    }

    fn wayland(&self) -> bool {
        let set = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
        if self
            .gdk_backend
            .as_deref()
            .is_some_and(|b| b.trim().starts_with("x11"))
        {
            return false;
        }
        set(&self.wayland_display)
            || self
                .xdg_session_type
                .as_deref()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("wayland"))
    }

    /// Whether the DMA-BUF renderer must be disabled.
    pub fn should_disable_dmabuf(&self) -> bool {
        !self.already_set && self.nvidia && self.wayland()
    }
}

/// Sets the variable when needed. Call first thing in `run()`, before any
/// thread starts (setting the environment is not thread-safe).
pub fn apply() {
    if !cfg!(target_os = "linux") {
        return;
    }
    if Probe::from_system().should_disable_dmabuf() {
        std::env::set_var(VAR, "1");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wayland_nvidia() -> Probe {
        Probe {
            wayland_display: Some("wayland-1".into()),
            xdg_session_type: Some("wayland".into()),
            nvidia: true,
            ..Probe::default()
        }
    }

    #[test]
    fn wayland_with_nvidia_disables_dmabuf() {
        assert!(wayland_nvidia().should_disable_dmabuf());
        let only_session_type = Probe {
            wayland_display: None,
            ..wayland_nvidia()
        };
        assert!(only_session_type.should_disable_dmabuf());
        let only_display = Probe {
            xdg_session_type: Some("tty".into()),
            ..wayland_nvidia()
        };
        assert!(only_display.should_disable_dmabuf());
    }

    #[test]
    fn other_gpus_and_x11_keep_dmabuf() {
        let other_gpu = Probe {
            nvidia: false,
            ..wayland_nvidia()
        };
        assert!(!other_gpu.should_disable_dmabuf());
        let x11 = Probe {
            wayland_display: None,
            xdg_session_type: Some("x11".into()),
            ..wayland_nvidia()
        };
        assert!(!x11.should_disable_dmabuf());
        let empty_display = Probe {
            wayland_display: Some(" ".into()),
            xdg_session_type: None,
            ..wayland_nvidia()
        };
        assert!(!empty_display.should_disable_dmabuf());
        let forced_x11 = Probe {
            gdk_backend: Some("x11".into()),
            ..wayland_nvidia()
        };
        assert!(!forced_x11.should_disable_dmabuf());
    }

    #[test]
    fn a_value_the_user_set_is_left_alone() {
        let user_set = Probe {
            already_set: true,
            ..wayland_nvidia()
        };
        assert!(!user_set.should_disable_dmabuf());
    }
}
