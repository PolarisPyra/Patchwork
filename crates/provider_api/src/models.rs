//! The models the saved providers offer, and the connection a turn makes to one.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use patchwork_provider_core::ModelOption;
use patchwork_platform::Error;
use serde::Deserialize;
use serde_json::{Value, json};

use super::catalog::{ApiModel, ApiProvider, Header, blank};
use super::secrets::{key, with_headers};
use super::store::{check_url, provider, providers};

/// An error an API provider answered with, as one readable line.
#[must_use]
pub fn error_text(status: u16, body: &Value, raw: &str) -> String {
    let error = &body["error"];
    let message = error["message"]
        .as_str()
        .or_else(|| error.as_str())
        .or_else(|| body["message"].as_str())
        .or_else(|| body["detail"].as_str())
        .map_or_else(|| raw.trim().chars().take(300).collect(), String::from);
    let hint = match status {
        401 | 403 => " Check the API key in Settings → API Providers.",
        402 => " The account may be out of credit.",
        404 => " Check the base URL and the model id.",
        _ => "",
    };
    match status {
        0 => message,
        _ if message.is_empty() => format!("The provider answered {status}.{hint}"),
        _ => format!("{message} ({status}).{hint}"),
    }
}

/// Models of every enabled provider, as `provider/model` ids.
#[must_use]
pub fn models() -> Vec<ModelOption> {
    providers()
        .into_iter()
        .filter(|p| p.enabled)
        .flat_map(|p| {
            p.models.into_iter().map(move |m| {
                ModelOption::listed(
                    format!("{}/{}", p.id, m.id),
                    m.name.clone().unwrap_or_else(|| m.id.clone()),
                )
            })
        })
        .collect()
}

/// The first model of the first enabled provider, which a task with no model uses.
#[must_use]
pub fn default_model() -> Option<String> {
    models().into_iter().next().and_then(|m| m.id)
}

/// Splits `provider/model` into the provider and the model's own id.
#[must_use]
pub fn resolve(model: &str) -> Option<(ApiProvider, String)> {
    let (id, model) = model.split_once('/')?;
    Some((provider(id)?, model.to_string()))
}

/// What the models request needs: either a saved provider, or the settings being edited.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsQuery {
    pub base_url: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    /// A key typed but not saved yet.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_env: Option<String>,
    /// The saved provider whose key to use when none was typed.
    #[serde(default)]
    pub provider_id: Option<String>,
}

/// Asks the endpoint for its models (`GET /models`), which also checks the URL and key.
///
/// # Errors
///
/// Returns an error when the base URL does not start with `http://` or `https://`, when the server
/// cannot be reached, when it answers with an error status, or when its reply holds no model list.
pub fn fetch_models(query: ModelsQuery) -> Result<Vec<ApiModel>, Error> {
    let key = query
        .key
        .filter(|k| !k.trim().is_empty())
        .map(|k| k.trim().to_string())
        .or_else(|| {
            let var = query.key_env.filter(|v| !v.trim().is_empty());
            let saved = query.provider_id.as_deref().and_then(provider);
            match (var, saved) {
                (Some(var), _) => key(&ApiProvider {
                    key_env: Some(var),
                    ..blank()
                }),
                (None, Some(saved)) => key(&ApiProvider {
                    key_env: None,
                    ..saved
                }),
                (None, None) => None,
            }
        });
    let base = query.base_url.trim().trim_end_matches('/');
    check_url(base)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        // A cloud model list can be slow to build, but the dialog is waiting on it.
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent.get(format!("{base}/models"));
    if let Some(key) = &key {
        request = request.header("Authorization", &format!("Bearer {key}"));
    }
    for h in &query.headers {
        request = request.header(h.name.trim(), h.value.trim());
    }
    let mut response = request
        .call()
        .map_err(|e| patchwork_platform::unreachable(base, &e))?;
    let status = response.status();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(error_text(status.as_u16(), &body, &text).into());
    }
    let list = body["data"]
        .as_array()
        .or_else(|| body["models"].as_array());
    let Some(list) = list else {
        return Err(format!(
            "{base}/models returned no model list: {} Local servers such as LM Studio serve models under /v1; check the base URL.",
            error_text(0, &body, &text)
        )
        .into());
    };
    let mut models: Vec<ApiModel> = list
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str().or_else(|| m["name"].as_str())?.to_string();
            Some(ApiModel { id, name: None })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

/// The request line the API bridge reads first: everything a turn needs, key included, so
/// the key never appears in the bridge's arguments or environment.
///
/// # Errors
///
/// Returns an error when no enabled provider has the model, the base URL is invalid, or the
/// configured key environment variable is unset.
pub fn connection(model: Option<&str>) -> Result<Value, Error> {
    let model = model
        .map(String::from)
        .or_else(default_model)
        .ok_or("Add a provider with a model in Settings → Providers first")?;
    let (provider, model_id) =
        resolve(&model).ok_or_else(|| format!("No API provider has the model {model}"))?;
    if !provider.enabled {
        return Err(format!("{} is turned off in Settings → Providers", provider.name).into());
    }
    check_url(&provider.base_url)?;
    let provider = with_headers(provider);
    let key = key(&provider);
    if key.is_none() && provider.key_env.is_some() {
        let var = provider.key_env.as_deref().unwrap_or_default();
        return Err(format!("{var} is not set, so {} has no key", provider.name).into());
    }
    let headers: serde_json::Map<String, Value> = provider
        .headers
        .iter()
        .map(|h| (h.name.trim().to_string(), json!(h.value.trim())))
        .collect();
    Ok(json!({
        "baseUrl": provider.base_url,
        "apiKey": key,
        "headers": headers,
        "model": model_id,
        "provider": provider.name,
        "local": provider.local,
        "preset": provider.preset,
        "systemPrompt": provider.system_prompt,
    }))
}
