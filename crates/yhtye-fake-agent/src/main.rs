//! Deterministic fake ACP agent for Yhtye tests.
//!
//! Speaks ACP (agent side) on stdio and follows the scenario in the
//! `YHTYE_FAKE_SCRIPT` environment variable (see [`scenario`]).

mod agent;
mod mcp;
mod scenario;

use std::process::ExitCode;

/// Argument of the sleeping child that `spawn_child` starts (a stand-in for a
/// descendant of the agent that must not outlive it).
const SLEEP_ARG: &str = "--sleep";

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some(SLEEP_ARG) {
        tokio::time::sleep(std::time::Duration::from_secs(600)).await;
        return ExitCode::SUCCESS;
    }
    let script = std::env::var("YHTYE_FAKE_SCRIPT").unwrap_or_else(|_| "{}".into());
    let scenario = match serde_json::from_str::<scenario::Scenario>(&script) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("yhtye-fake-agent: invalid YHTYE_FAKE_SCRIPT: {e}");
            return ExitCode::from(64);
        }
    };
    match agent::serve(scenario).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("yhtye-fake-agent: connection error: {e}");
            ExitCode::FAILURE
        }
    }
}
