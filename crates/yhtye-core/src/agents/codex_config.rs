//! What Yhtye reads of the user's Codex configuration (Stage 7e): only which
//! model provider it uses, to decide where the model list comes from. The
//! file is never written and nothing else in it is looked at.

use std::ffi::OsString;
use std::path::PathBuf;

/// File name of Codex's configuration inside `CODEX_HOME`.
const CONFIG_FILE: &str = "config.toml";

/// Codex's home: `CODEX_HOME` when set and non-empty, else `~/.codex`.
pub fn codex_home(env: &(impl Fn(&str) -> Option<OsString> + ?Sized)) -> Option<PathBuf> {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    non_empty("CODEX_HOME")
        .or_else(|| non_empty("HOME").map(|home| home.join(".codex")))
        .filter(|p| p.is_absolute())
}

/// The top-level `model_provider` of a `config.toml` text. `None` when it is
/// absent (Codex's own OpenAI provider) or the text is not valid TOML.
pub fn model_provider(config_toml: &str) -> Option<String> {
    let table: toml::Table = config_toml.parse().ok()?;
    table.get("model_provider")?.as_str().map(str::to_string)
}

/// The `model_provider` of the user's Codex configuration, read now.
/// A missing or unreadable file is "no provider set".
pub async fn configured_provider(
    env: &(impl Fn(&str) -> Option<OsString> + ?Sized),
) -> Option<String> {
    let path = codex_home(env)?.join(CONFIG_FILE);
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => model_provider(&text),
        Err(e) => {
            tracing::debug!("no Codex config at {}: {e}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn the_home_is_codex_home_else_dot_codex() {
        assert_eq!(
            codex_home(&env(&[("CODEX_HOME", "/x/codex"), ("HOME", "/h")])),
            Some(PathBuf::from("/x/codex"))
        );
        assert_eq!(
            codex_home(&env(&[("HOME", "/h")])),
            Some(PathBuf::from("/h/.codex"))
        );
        assert_eq!(
            codex_home(&env(&[("CODEX_HOME", ""), ("HOME", "/h")])),
            Some(PathBuf::from("/h/.codex")),
            "an empty CODEX_HOME counts as unset"
        );
        assert_eq!(codex_home(&env(&[])), None);
        assert_eq!(codex_home(&env(&[("CODEX_HOME", "relative")])), None);
    }

    #[test]
    fn reads_the_top_level_provider_only() {
        assert_eq!(
            model_provider("model = \"m\"\nmodel_provider = \"openrouter\"\n").as_deref(),
            Some("openrouter")
        );
        let nested = "[model_providers.openrouter]\nname = \"x\"\n\n[profiles.p]\nmodel_provider = \"other\"\n";
        assert_eq!(model_provider(nested), None, "tables are not the top level");
        assert_eq!(model_provider("model = \"m\""), None);
        assert_eq!(model_provider("model_provider = 5"), None);
        assert_eq!(model_provider("this is = = not toml"), None);
    }

    #[tokio::test]
    async fn a_missing_file_means_no_provider() {
        let dir = tempfile::tempdir().expect("dir");
        let home = dir.path().display().to_string();
        let e = env(&[("CODEX_HOME", &home)]);
        assert_eq!(configured_provider(&e).await, None);
        std::fs::write(
            dir.path().join("config.toml"),
            "model_provider = \"openrouter\"\n",
        )
        .expect("write");
        assert_eq!(configured_provider(&e).await.as_deref(), Some("openrouter"));
    }
}
