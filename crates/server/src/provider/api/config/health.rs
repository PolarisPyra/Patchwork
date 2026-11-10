//! Whether a provider's endpoint answers, and which local model servers are running.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::Error;
use crate::provider::api::servers;

use super::models::{ModelsQuery, fetch_models};
use super::secrets::with_headers;
use super::store::provider;

/// The live health of a saved provider's endpoint, probed on demand so Settings → Providers can
/// show whether a local server is answering and which models it serves right now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    /// The endpoint answered its model list.
    pub reachable: bool,
    /// Round-trip of the probe in milliseconds, when it answered.
    pub latency_ms: Option<u32>,
    /// The model ids the endpoint serves right now.
    pub served_models: Vec<String>,
    /// Why the probe failed, when it did.
    pub error: Option<String>,
    /// Whether each of its saved models reads images, for a local server whose engine says.
    pub images: HashMap<String, bool>,
}

/// Checks a saved provider's endpoint and reports its health and latency.
///
/// Probes a saved provider's endpoint (`GET /models`) with its stored key and headers, timing the
/// round-trip. A failure is reported in `error`, not as an `Err`, so one unreachable server does
/// not read as a broken request.
///
/// # Errors
///
/// Returns an error when the provider is no longer saved; an unreachable server is reported in the
/// returned health instead.
pub fn probe(id: &str) -> Result<ProviderHealth, Error> {
    let provider = provider(id).ok_or("That provider is no longer saved")?;
    let provider = with_headers(provider);
    let query = ModelsQuery {
        base_url: provider.base_url.clone(),
        headers: provider.headers.clone(),
        key: None,
        key_env: provider.key_env.clone(),
        provider_id: Some(provider.id.clone()),
    };
    let started = Instant::now();
    match fetch_models(query) {
        Ok(models) => {
            let latency_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
            let served_models: Vec<String> = models.into_iter().map(|m| m.id).collect();
            let saved: Vec<String> = provider
                .models
                .iter()
                .map(|m| m.id.clone())
                .filter(|m| served_models.contains(m))
                .collect();
            let images = if provider.local {
                servers::images(&provider, &saved)
            } else {
                HashMap::new()
            };
            Ok(ProviderHealth {
                reachable: true,
                latency_ms: Some(latency_ms),
                served_models,
                error: None,
                images,
            })
        }
        Err(error) => Ok(ProviderHealth {
            reachable: false,
            latency_ms: None,
            served_models: Vec::new(),
            error: Some(error.to_string()),
            images: HashMap::new(),
        }),
    }
}

/// A local model server found on this machine, offered to prefill the add-server form.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServer {
    /// The engine the port is known for, e.g. `Ollama`.
    pub name: String,
    pub base_url: String,
    /// The model ids it serves right now.
    pub models: Vec<String>,
}

/// Ports well-known OpenAI-compatible local servers listen on, with the engine each belongs to.
const DISCOVERY_CANDIDATES: &[(&str, &str)] = &[
    ("Ollama", "http://localhost:11434/v1"),
    ("LM Studio", "http://localhost:1234/v1"),
    ("llama.cpp", "http://localhost:8080/v1"),
    ("llama.cpp", "http://localhost:8081/v1"),
    ("vLLM", "http://localhost:8000/v1"),
    ("Local server", "http://localhost:5000/v1"),
];

/// Looks for local model servers on well-known ports, sending no key, and returns the ones that
/// answer their model list. Runs each time Settings → Providers opens.
#[must_use]
pub fn discover() -> Vec<DiscoveredServer> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        // Only local ports are asked, which answer at once when anything is listening.
        .timeout_global(Some(Duration::from_millis(1500)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut found: Vec<DiscoveredServer> = Vec::new();
    for (name, base) in DISCOVERY_CANDIDATES {
        if found.iter().any(|d| d.base_url == *base) {
            continue;
        }
        let Ok(mut response) = agent.get(format!("{base}/models")).call() else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let text = response.body_mut().read_to_string().unwrap_or_default();
        let Ok(body) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(list) = body["data"]
            .as_array()
            .or_else(|| body["models"].as_array())
        else {
            continue;
        };
        let models = list
            .iter()
            .filter_map(|m| {
                m["id"]
                    .as_str()
                    .or_else(|| m["name"].as_str())
                    .map(String::from)
            })
            .collect();
        found.push(DiscoveredServer {
            name: (*name).to_string(),
            base_url: (*base).to_string(),
            models,
        });
    }
    found
}
