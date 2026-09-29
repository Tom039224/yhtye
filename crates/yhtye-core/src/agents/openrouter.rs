//! The model list of OpenRouter (Stage 7e, `acp-harnesses.md` §9.4). The Codex
//! adapter lists only OpenAI's own catalog, so when the user's Codex config
//! uses the `openrouter` provider the models come from OpenRouter's public
//! `GET /api/v1/models` instead. The request carries no credentials (the
//! endpoint is public); none are ever read or sent here.

use std::time::Duration;

use serde_json::Value;

use super::models::{EffortOption, ModelOption};

/// Public model list of OpenRouter.
pub const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";
/// The Codex `model_provider` value that selects it.
pub const OPENROUTER_PROVIDER: &str = "openrouter";

/// Upper bound for the request (the answer is about 1 MB).
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest answer read (the real one is about 1 MB).
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;
/// A model must support tool calls to work as an agent.
const TOOLS_PARAMETER: &str = "tools";

/// Downloads and reads the model list from `url` (no `Authorization` header).
pub async fn fetch_openrouter_models(url: &str) -> Result<Vec<ModelOption>, String> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("yhtye/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("could not create the HTTP client: {e}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not reach {url}: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{url} answered {status}"));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BODY_BYTES as u64)
    {
        return Err(format!("{url} answered more than {MAX_BODY_BYTES} bytes"));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("could not read the answer of {url}: {e}"))?;
    if bytes.len() > MAX_BODY_BYTES {
        return Err(format!("{url} answered more than {MAX_BODY_BYTES} bytes"));
    }
    let body: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{url} did not answer JSON: {e}"))?;
    models_from_openrouter(&body)
}

/// The models of an OpenRouter `/models` answer: the entries of `data` whose
/// `supported_parameters` include `tools`, in the order given. Each model's
/// efforts are `reasoning.supported_efforts` (`Some(empty)`: no effort option).
/// Codex passes `model_reasoning_effort` on as a plain string (its schema only
/// requires a non-empty one), so every value OpenRouter lists is offered as is.
pub fn models_from_openrouter(body: &Value) -> Result<Vec<ModelOption>, String> {
    let data = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or("the answer has no `data` list")?;
    let mut models: Vec<ModelOption> = Vec::new();
    for entry in data {
        let Some(id) = entry.get("id").and_then(Value::as_str) else {
            continue;
        };
        let supports_tools = entry
            .get("supported_parameters")
            .and_then(Value::as_array)
            .is_some_and(|p| p.iter().any(|v| v.as_str() == Some(TOOLS_PARAMETER)));
        if !supports_tools || models.iter().any(|m| m.value == id) {
            continue;
        }
        models.push(ModelOption {
            value: id.to_string(),
            name: entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string(),
            description: None,
            efforts: Some(efforts_of(entry)),
        });
    }
    Ok(models)
}

fn efforts_of(entry: &Value) -> Vec<EffortOption> {
    let mut efforts: Vec<EffortOption> = Vec::new();
    let listed = entry
        .pointer("/reasoning/supported_efforts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|e| !e.is_empty());
    for value in listed {
        if efforts.iter().all(|e| e.value != value) {
            efforts.push(EffortOption {
                value: value.to_string(),
                name: value.to_string(),
                description: None,
            });
        }
    }
    efforts
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn values(efforts: &[EffortOption]) -> Vec<&str> {
        efforts.iter().map(|e| e.value.as_str()).collect()
    }

    #[test]
    fn keeps_only_models_with_tools_and_reads_their_efforts() {
        let body = json!({"data": [
            {"id": "a/with-effort", "name": "A: With effort",
             "supported_parameters": ["tools", "reasoning"],
             "reasoning": {"supported_efforts": ["max", "xhigh", "high", "none"]}},
            {"id": "b/no-effort:free", "name": "B: Free",
             "supported_parameters": ["tools"]},
            {"id": "c/no-tools", "supported_parameters": ["temperature"]},
            {"id": "d/no-params"},
            {"id": "a/with-effort", "supported_parameters": ["tools"]},
            {"name": "no id", "supported_parameters": ["tools"]},
            {"id": "e/empty-efforts", "supported_parameters": ["tools"],
             "reasoning": {"mandatory": false, "supported_efforts": ["", "low", "low"]}}
        ]});
        let models = models_from_openrouter(&body).expect("models");
        let ids: Vec<&str> = models.iter().map(|m| m.value.as_str()).collect();
        assert_eq!(
            ids,
            ["a/with-effort", "b/no-effort:free", "e/empty-efforts"]
        );
        assert_eq!(models[0].name, "A: With effort");
        assert_eq!(
            values(models[0].efforts.as_deref().expect("read")),
            ["max", "xhigh", "high", "none"],
            "the API's order and every listed value"
        );
        assert_eq!(
            models[1].efforts,
            Some(Vec::new()),
            "no reasoning info: no effort option, and nothing left to ask for"
        );
        assert_eq!(models[2].name, "e/empty-efforts", "the id when unnamed");
        assert_eq!(values(models[2].efforts.as_deref().expect("read")), ["low"]);
    }

    #[test]
    fn an_answer_without_data_is_an_error() {
        assert!(models_from_openrouter(&json!({"error": "x"})).is_err());
        assert_eq!(models_from_openrouter(&json!({"data": []})), Ok(Vec::new()));
    }
}
