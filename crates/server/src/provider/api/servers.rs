//! Local model servers: finding one that moved, and which of its models read images.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Value, json};

use super::config::{self, ApiProvider};

/// Ports well-known local servers listen on: Ollama, LM Studio, llama.cpp, vLLM, and others.
const KNOWN_PORTS: &[u16] = &[11434, 1234, 8080, 8081, 8000, 5000];

/// The most of this computer's listening ports tried when looking for a server that moved.
const MAX_PORTS: usize = 48;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// The scheme, host, port, and path of a base URL: `http`, `localhost`, `8080`, `/v1`.
fn split(base: &str) -> Option<(&str, &str, Option<u16>, &str)> {
    let (scheme, rest) = base.split_once("://")?;
    let (authority, path) = rest.find('/').map_or((rest, ""), |i| rest.split_at(i));
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, port.parse().ok()),
        _ => (authority, None),
    };
    Some((scheme, host, port, path.trim_end_matches('/')))
}

fn is_this_computer(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0")
}

/// The server at `base` runs on this computer.
pub(super) fn on_this_computer(base: &str) -> bool {
    split(base).is_some_and(|(_, host, ..)| is_this_computer(host))
}

/// The model ids the server at `base` serves, when it answers its model list.
fn served(agent: &ureq::Agent, provider: &ApiProvider, base: &str) -> Option<Vec<String>> {
    let mut request = agent.get(format!("{base}/models"));
    if let Some(key) = config::key(provider) {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    for h in &provider.headers {
        request = request.header(h.name.trim(), h.value.trim());
    }
    let mut response = request.call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: Value = serde_json::from_str(&response.body_mut().read_to_string().ok()?).ok()?;
    let list = body["data"]
        .as_array()
        .or_else(|| body["models"].as_array())?;
    Some(
        list.iter()
            .filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()))
            .map(String::from)
            .collect(),
    )
}

/// Ports something listens on on this computer, from Linux's socket tables; none elsewhere.
fn listening_ports() -> Vec<u16> {
    let mut ports = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        for line in text.lines().skip(1) {
            // sl, local_address, rem_address, st, ...
            let fields: Vec<&str> = line.split_whitespace().take(4).collect();
            let [_, local, _, "0A"] = fields[..] else {
                continue;
            };
            if let Some(port) = local
                .rsplit(':')
                .next()
                .and_then(|p| u16::from_str_radix(p, 16).ok())
                && port >= 1024
                && !ports.contains(&port)
            {
                ports.push(port);
            }
        }
    }
    ports.sort_unstable();
    ports.truncate(MAX_PORTS);
    ports
}

/// Finds a saved server's new address, saves it, and returns the base URL.
///
/// Finds where a saved server moved on its machine: the well-known ports on its host, and on
/// this computer every port something listens on, for a server that serves one of its models.
/// Saves and returns the new base URL.
///
/// # Errors
///
/// Returns an error when the provider is no longer saved, its base URL is not valid, no server on
/// that host serves its models, or saving the new address fails.
pub fn relocate(id: &str) -> Result<String, Error> {
    let provider = config::provider(id).ok_or("That provider is no longer saved")?;
    let provider = config::with_headers(provider);
    let (scheme, host, port, path) =
        split(&provider.base_url).ok_or("The base URL is not valid")?;
    let mut ports: Vec<u16> = KNOWN_PORTS.to_vec();
    if is_this_computer(host) {
        ports.extend(listening_ports());
    }
    let mut seen = std::collections::HashSet::new();
    ports.retain(|p| Some(*p) != port && seen.insert(*p));
    let mut paths = vec![path];
    if path != "/v1" {
        paths.push("/v1");
    }
    let wanted: Vec<&str> = provider.models.iter().map(|m| m.id.as_str()).collect();
    // A local server answers in milliseconds; a closed port must not hold up the search.
    let agent = agent(Duration::from_millis(1200));
    // Every port at once, read back in order so the well-known ports win.
    let found = std::thread::scope(|scope| {
        let tries: Vec<_> = ports
            .iter()
            .map(|port| {
                let (agent, provider, paths) = (&agent, &provider, &paths);
                scope.spawn(move || {
                    paths.iter().find_map(|path| {
                        let base = format!("{scheme}://{host}:{port}{path}");
                        served(agent, provider, &base).map(|models| (base, models))
                    })
                })
            })
            .collect();
        tries
            .into_iter()
            .filter_map(|t| t.join().ok().flatten())
            .find(|(_, models)| {
                wanted.is_empty() || models.iter().any(|m| wanted.contains(&m.as_str()))
            })
    });
    let Some((base, _)) = found else {
        return Err(format!(
            "No server on {host} serves {}'s models. Start it, or change its address.",
            provider.name
        )
        .into());
    };
    config::move_to(id, &base)?;
    log::info!(provider:% = provider.name, base:%; "{{provider}} moved to {{base}}");
    Ok(base)
}

/// Which of `models` read images, by what the server's engine says: LM Studio's model list,
/// llama.cpp's `/props`, or Ollama's `/api/show`. A model no engine answers for is left out.
pub(super) fn images(provider: &ApiProvider, models: &[String]) -> HashMap<String, bool> {
    let mut out = HashMap::new();
    let Some((scheme, host, port, _)) = split(&provider.base_url) else {
        return out;
    };
    let origin = match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    let agent = agent(Duration::from_millis(1500));
    let get = |path: &str| -> Option<Value> {
        let mut response = agent.get(format!("{origin}{path}")).call().ok()?;
        if !response.status().is_success() {
            return None;
        }
        serde_json::from_str(&response.body_mut().read_to_string().ok()?).ok()
    };
    // LM Studio: every model with its type, `vlm` for one that reads images.
    if let Some(list) = get("/api/v0/models").and_then(|b| b["data"].as_array().cloned()) {
        for m in list {
            if let Some(id) = m["id"].as_str().filter(|id| models.iter().any(|m| m == id)) {
                out.insert(id.to_string(), m["type"] == "vlm");
            }
        }
        return out;
    }
    // llama.cpp: one loaded model, and whether its projector reads images.
    if let Some(vision) = get("/props").and_then(|b| b["modalities"]["vision"].as_bool()) {
        for m in models {
            out.insert(m.clone(), vision);
        }
        return out;
    }
    // Ollama: each model's capabilities.
    for m in models {
        let shown = agent
            .post(format!("{origin}/api/show"))
            .header("Content-Type", "application/json")
            .send(json!({ "model": m }).to_string())
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|mut r| r.body_mut().read_to_string().ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let Some(capabilities) = shown.as_ref().and_then(|b| b["capabilities"].as_array()) else {
            break;
        };
        out.insert(m.clone(), capabilities.iter().any(|c| c == "vision"));
    }
    out
}
