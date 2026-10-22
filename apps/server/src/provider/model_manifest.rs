//! Model lists for CLIs that don't publish one. The manifest lives in the private
//! repo; the server fetches it in the background and caches it in the config dir.
//! Nothing is compiled in, so a provider's models appear only after a fetch succeeds.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::types::ModelOption;

const MANIFEST_URL: &str = "https://api.github.com/repos/PolarisPyra/patchwork/contents/apps/server/src/provider/model-manifest.json";
const TOKEN_VAR: &str = "PATCHWORK_MANIFEST_TOKEN";
const CACHE_FILE: &str = "model-manifest.json";

static CACHE: OnceLock<PathBuf> = OnceLock::new();

/// Starts the background fetch. Call once at startup with the config directory.
/// Without a token (environment or `.env`) nothing is fetched.
pub fn start(config_dir: &Path) {
    let path = config_dir.join(CACHE_FILE);
    let _ = CACHE.set(path.clone());
    let token = std::env::var(TOKEN_VAR)
        .ok()
        .or_else(|| dotenv_value(TOKEN_VAR))
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    let Some(token) = token else {
        eprintln!(
            "model manifest: {TOKEN_VAR} is not set in the environment or .env; no models will be listed"
        );
        return;
    };
    std::thread::spawn(move || {
        if let Err(e) = refresh(&path, &token) {
            eprintln!("model manifest: {e}");
        }
    });
}

/// The cached manifest's models for `provider` ("claude", "antigravity").
/// Empty until a fetch has succeeded.
pub fn models(provider: &str) -> Vec<ModelOption> {
    CACHE
        .get()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| models_in(&text, provider))
        .unwrap_or_default()
}

/// `key` from the nearest `.env` above the working directory, as `KEY=value` lines.
fn dotenv_value(key: &str) -> Option<String> {
    let dir = std::env::current_dir().ok()?;
    let text = dir
        .ancestors()
        .map(|d| d.join(".env"))
        .find_map(|p| std::fs::read_to_string(p).ok())?;
    value_in(&text, key)
}

fn value_in(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches(['"', '\'']).to_owned())
    })
}

fn refresh(path: &Path, token: &str) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(MANIFEST_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github.raw")
        .header("User-Agent", "patchwork")
        .call()
        .map_err(|e| format!("couldn't fetch: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("GitHub answered {}", response.status()));
    }
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("couldn't read: {e}"))?;
    let fetched: Value = serde_json::from_str(&text).map_err(|e| format!("bad manifest: {e}"))?;
    let cached = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    if is_newer(&fetched, cached.as_ref()) {
        std::fs::write(path, text).map_err(|e| format!("couldn't cache: {e}"))?;
    }
    Ok(())
}

/// `updatedAt` is an ISO timestamp, so a later one compares greater as a string.
fn is_newer(fetched: &Value, cached: Option<&Value>) -> bool {
    let date = |v: &Value| v["updatedAt"].as_str().unwrap_or_default().to_owned();
    cached.is_none_or(|c| date(fetched) > date(c))
}

fn models_in(text: &str, provider: &str) -> Vec<ModelOption> {
    let manifest: Value = serde_json::from_str(text).unwrap_or_default();
    manifest["providers"][provider]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(ModelOption {
                id: Some(m["id"].as_str()?.into()),
                label: m["name"].as_str()?.into(),
                default_model: None,
                is_legacy: m["legacy"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
        "updatedAt": "2026-10-19T20:30:00Z",
        "providers": {
            "claude": [{ "id": "claude-opus-5-5", "name": "Opus 5.5" }],
            "antigravity": [{ "id": "gemini-3.8-flash", "name": "3.8 Flash" }]
        }
    }"#;

    #[test]
    fn models_come_from_the_named_provider() {
        let claude = models_in(MANIFEST, "claude");
        assert_eq!(claude.len(), 1);
        assert_eq!(claude[0].id.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(claude[0].label, "Opus 5.5");
        assert_eq!(models_in(MANIFEST, "codex").len(), 0);
    }

    #[test]
    fn dotenv_value_reads_the_key() {
        let text = "# comment\nOTHER=1\nPATCHWORK_MANIFEST_TOKEN=\"abc\"\n";
        assert_eq!(value_in(text, TOKEN_VAR).as_deref(), Some("abc"));
        assert_eq!(
            value_in("PATCHWORK_MANIFEST_TOKEN=\n", TOKEN_VAR).as_deref(),
            Some("")
        );
        assert_eq!(value_in(text, "MISSING"), None);
    }

    #[test]
    fn newer_manifest_replaces_cache() {
        let cached: Value = serde_json::from_str(MANIFEST).unwrap();
        let newer: Value = serde_json::from_str(r#"{"updatedAt": "2026-11-02"}"#).unwrap();
        let older: Value = serde_json::from_str(r#"{"updatedAt": "2026-09-01"}"#).unwrap();
        assert!(is_newer(&newer, Some(&cached)));
        assert!(!is_newer(&older, Some(&cached)));
        assert!(is_newer(&older, None));
    }
}
