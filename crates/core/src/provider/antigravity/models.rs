//! The models agy offers, grouped by base model with the reasoning efforts each supports.

// Rust guideline compliant 2026-11-23

use std::collections::HashMap;

use serde_json::Value;

use crate::provider::{AgentKind, ModelOption, probe};

use super::probe::{AcpProbe, probe_acp_cached};

/// Suffix on a model id and the reasoning effort it maps to.
pub(super) const EFFORT_SUFFIXES: &[(&str, &str)] = &[
    ("-extra-high", "xhigh"),
    ("-xhigh", "xhigh"),
    ("-high", "high"),
    ("-medium", "medium"),
    ("-low", "low"),
];

/// Strips a reasoning effort suffix from a model id if present.
pub(super) fn strip_effort_suffix(id: &str) -> &str {
    for (suffix, _) in EFFORT_SUFFIXES {
        if let Some(base) = id.strip_suffix(suffix) {
            return base;
        }
    }
    id
}

/// Strips a trailing effort label from a display name.
fn strip_effort_label(name: &str) -> &str {
    for label in [
        " (Extra High)",
        " (XHigh)",
        " (High)",
        " (Medium)",
        " (Low)",
    ] {
        if let Some(base) = name.strip_suffix(label) {
            return base;
        }
    }
    name
}

/// Detects reasoning effort from a model id.
fn effort_from_id(id: &str) -> Option<&'static str> {
    for (suffix, effort) in EFFORT_SUFFIXES {
        if id.ends_with(suffix) {
            return Some(effort);
        }
    }
    None
}

fn sort_efforts(efforts: &mut [String]) {
    efforts.sort_by_key(|e| match e.as_str() {
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "xhigh" => 4,
        _ => 5,
    });
}

/// The models the CLI offers, from `agy acp` (falling back to `agy models`), grouped by base
/// model with their supported reasoning efforts.
#[must_use]
pub fn models() -> Vec<ModelOption> {
    if let AcpProbe::SignedIn { models, .. } = probe_acp_cached()
        && !models.is_empty()
    {
        return distinct_labels(models);
    }
    let Some(listed) = probe(AgentKind::Antigravity, &["models"]).filter(|p| p.success) else {
        return Vec::new();
    };
    distinct_labels(parse_models(&listed.stdout))
}

/// Names each model whose label another shares with its id as well, so the picker tells them
/// apart.
fn distinct_labels(mut models: Vec<ModelOption>) -> Vec<ModelOption> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for model in &models {
        *counts.entry(model.label.clone()).or_default() += 1;
    }
    for model in &mut models {
        if counts.get(&model.label).is_some_and(|&n| n > 1)
            && let Some(id) = &model.id
        {
            model.label = format!("{} ({id})", model.label);
        }
    }
    models
}

/// Parses base models and their per-model effort options from an ACP `session/new` result.
#[must_use]
pub fn parse_acp_models(session_result: &Value) -> (Option<String>, Vec<ModelOption>) {
    let available: Vec<(&str, &str)> = session_result["models"]["availableModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["modelId"].as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let name = m["name"].as_str().unwrap_or(id).trim();
            Some((id, name))
        })
        .collect();

    let model_config = session_result["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|opt| opt["id"].as_str() == Some("model"));

    let Some(config) = model_config else {
        let default_model = session_result["models"]["currentModelId"]
            .as_str()
            .map(|id| strip_effort_suffix(id).to_string());
        return (default_model, group_model_pairs(&available));
    };

    let default_model = config["currentValue"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(String::from);

    let mut out = Vec::new();
    for entry in config["options"].as_array().into_iter().flatten() {
        let Some(base_id) = entry["value"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let label = entry["name"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(base_id)
            .to_string();

        let mut efforts = Vec::new();
        for &(variant_id, _) in &available {
            if let Some(rest) = variant_id.strip_prefix(base_id)
                && let Some(effort) = EFFORT_SUFFIXES
                    .iter()
                    .find_map(|(suffix, effort)| (*suffix == rest).then_some(*effort))
                && !efforts.iter().any(|e| e == effort)
            {
                efforts.push(effort.to_string());
            }
        }
        sort_efforts(&mut efforts);
        out.push(ModelOption {
            id: Some(base_id.to_string()),
            label,
            default_model: None,
            is_legacy: false,
            efforts: Some(efforts),
            context_window: None,
        });
    }
    if out.is_empty() {
        return (default_model, group_model_pairs(&available));
    }
    (default_model, out)
}

/// Parses the tab-separated model list from `agy models`, consolidating effort variants into
/// base models with their supported effort levels.
#[must_use]
pub fn parse_models(stdout: &str) -> Vec<ModelOption> {
    let pairs: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(id, name)| (id.trim(), name.trim()))
        .filter(|(id, _)| !id.is_empty())
        .collect();
    group_model_pairs(&pairs)
}

fn group_model_pairs(pairs: &[(&str, &str)]) -> Vec<ModelOption> {
    let mut models: Vec<(String, String, Vec<String>, bool)> = Vec::new();
    for &(id, name) in pairs {
        let base_id = strip_effort_suffix(id);
        let base_name = strip_effort_label(name);
        let effort = effort_from_id(id);

        if let Some(existing) = models.iter_mut().find(|(m_id, _, _, _)| m_id == base_id) {
            if let Some(eff) = effort
                && !existing.2.iter().any(|e| e == eff)
            {
                existing.2.push(eff.to_string());
            }
        } else {
            let mut efforts = Vec::new();
            let had_effort = effort.is_some();
            if let Some(eff) = effort {
                efforts.push(eff.to_string());
            }
            models.push((
                base_id.to_string(),
                base_name.to_string(),
                efforts,
                had_effort,
            ));
        }
    }

    models
        .into_iter()
        .map(|(id, name, mut efforts, had_effort)| {
            sort_efforts(&mut efforts);
            ModelOption {
                id: Some(id),
                label: name,
                default_model: None,
                is_legacy: false,
                efforts: Some(if had_effort { efforts } else { Vec::new() }),
                context_window: None,
            }
        })
        .collect()
}
