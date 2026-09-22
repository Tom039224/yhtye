//! Deterministic fake ACP agent for Yhtye tests.
//!
//! Speaks ACP (agent side) on stdio and follows the scenario in the
//! `YHTYE_FAKE_SCRIPT` environment variable (see [`scenario`]).

mod agent;
mod mcp;
mod scenario;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
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
