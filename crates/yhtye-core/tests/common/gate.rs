//! A fake-agent harness that starts only when the test lets it, to hold a
//! probe in flight (the agent is a shell wrapper: Unix only).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use yhtye_core::acp::HarnessConfig;

use super::{EVENT_TIMEOUT, fake_agent_bin, fake_harness};

pub struct Gate {
    dir: PathBuf,
}

impl Gate {
    /// The wrapper and its two marker files live in `dir`.
    pub fn new(dir: &Path) -> Self {
        Self { dir: dir.into() }
    }

    fn started_marker(&self) -> PathBuf {
        self.dir.join("gate-started")
    }

    fn open_marker(&self) -> PathBuf {
        self.dir.join("gate-open")
    }

    /// [`fake_harness`] with `script`, behind the wrapper: it reports that it
    /// was launched, then waits for [`Gate::open`] before it runs the agent.
    pub fn harness(&self, script: serde_json::Value) -> HarnessConfig {
        let wrapper = self.dir.join("gated-agent");
        let text = format!(
            "#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done\nexec '{}' \"$@\"\n",
            self.started_marker().display(),
            self.open_marker().display(),
            fake_agent_bin().display()
        );
        std::fs::write(&wrapper, text).expect("write the wrapper");
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
            .expect("chmod the wrapper");
        let mut h = fake_harness(script);
        h.command = wrapper.display().to_string();
        h
    }

    /// Waits until a gated agent was launched (and so is held).
    pub async fn started(&self) {
        let marker = self.started_marker();
        tokio::time::timeout(EVENT_TIMEOUT, async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the gated agent was launched");
    }

    /// Lets the held agent (and every later one) start.
    pub fn open(&self) {
        std::fs::write(self.open_marker(), "").expect("open the gate");
    }
}
