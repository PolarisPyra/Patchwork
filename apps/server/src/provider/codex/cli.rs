//! What the installed Codex CLI offers, read from the CLI itself: its version, its feature
//! flags, its model catalog, and the read-only diagnostics commands. Patchwork asks the binary
//! rather than keeping a copy of these lists, so a newer Codex shows up as it is.

use std::path::PathBuf;

use crate::provider::cli_info::{self, InfoCommand};
use crate::provider::{CodexFeature, CodexInfo, CodexModel};

/// The Codex CLI in use, if it is installed.
pub fn binary() -> Option<PathBuf> {
    crate::provider::codex_binary()
}

/// Runs the Codex CLI with `args`. See `cli_info::run`.
fn run(args: &[&str]) -> Result<String, String> {
    let binary = binary().ok_or("The Codex CLI is not installed")?;
    cli_info::run(&binary, args)
}

/// The CLI's version line, e.g. `codex-cli 0.163.0-alpha.11.1`.
pub fn version() -> Result<String, String> {
    let text = run(&["--version"])?;
    Ok(text.lines().next().unwrap_or_default().trim().to_string())
}

/// Parses `codex features list`. Each line is a flag name, its stage, and whether it is on;
/// the stage may be two words, as in `under development`.
pub fn parse_features(text: &str) -> Vec<CodexFeature> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            if name.starts_with("Usage") || name.eq_ignore_ascii_case("name") {
                return None;
            }
            let rest: Vec<&str> = parts.collect();
            let (enabled, stage_words) = rest.split_last()?;
            let enabled = match *enabled {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            Some(CodexFeature {
                name: name.to_string(),
                stage: stage_words.join(" "),
                enabled,
            })
        })
        .collect()
}

/// Reads the CLI's feature flags, sorted by name.
pub fn features() -> Result<Vec<CodexFeature>, String> {
    let mut features = parse_features(&run(&["features", "list"])?);
    features.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(features)
}

/// Parses `codex debug models`, which prints `{ "models": [ … ] }`.
pub fn parse_models(text: &str) -> Vec<CodexModel> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let models = value
        .get("models")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    models
        .iter()
        .filter_map(|model| {
            let slug = model.get("slug")?.as_str()?.to_string();
            let display = model
                .get("display_name")
                .and_then(|v| v.as_str())
                .unwrap_or(&slug)
                .to_string();
            let efforts = model
                .get("supported_reasoning_levels")
                .and_then(|v| v.as_array())
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|level| level.get("effort")?.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let speed_tiers = model
                .get("service_tiers")
                .and_then(|v| v.as_array())
                .map(|tiers| {
                    tiers
                        .iter()
                        .filter_map(|tier| {
                            Some(format!(
                                "{} ({})",
                                tier.get("name")?.as_str()?,
                                tier.get("description")?.as_str().unwrap_or_default()
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(CodexModel {
                slug,
                display_name: display,
                context_window: model.get("context_window").and_then(|v| v.as_u64()),
                efforts,
                speed_tiers,
                multi_agent: model
                    .get("multi_agent_version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
        })
        .collect()
}

/// The models in the CLI's catalog, from `codex debug models`, each with the reasoning levels it
/// takes.
pub fn models() -> Vec<CodexModel> {
    run(&["debug", "models"])
        .map(|text| parse_models(&text))
        .unwrap_or_default()
}

/// The CLI's version, flags, and models, for the Settings pane.
pub fn info() -> Result<CodexInfo, String> {
    let path = binary().ok_or("The Codex CLI is not installed")?;
    Ok(CodexInfo {
        path: path.to_string_lossy().into_owned(),
        version: version().unwrap_or_else(|_| "unknown".into()),
        features: features().unwrap_or_default(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`.
pub fn command(kind: InfoCommand) -> Result<String, String> {
    let binary = binary().ok_or("The Codex CLI is not installed")?;
    cli_info::command(&binary, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_feature_list() {
        let text = "\
browser_use                              stable             true
code_mode                                under development  false
removed_flag                             removed            false
not a feature line
";
        let features = parse_features(text);
        assert_eq!(features.len(), 3);
        assert_eq!(features[0].name, "browser_use");
        assert_eq!(features[0].stage, "stable");
        assert!(features[0].enabled);
        assert_eq!(features[1].stage, "under development");
        assert!(!features[1].enabled);
    }

    #[test]
    fn parses_the_model_catalog() {
        let text = r#"{"models":[
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1-Sol","context_window":500000,
             "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}],
             "service_tiers":[{"id":"priority","name":"Fast","description":"2x speed"}],
             "multi_agent_version":"v2"},
            {"slug":"bare"}
        ]}"#;
        let models = parse_models(text);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].display_name, "GPT-6.1-Sol");
        assert_eq!(models[0].context_window, Some(500_000));
        assert_eq!(models[0].efforts, vec!["low", "high"]);
        assert_eq!(models[0].speed_tiers, vec!["Fast (2x speed)"]);
        assert_eq!(models[0].multi_agent.as_deref(), Some("v2"));
        // A model with only a slug still lists, under its slug.
        assert_eq!(models[1].display_name, "bare");
        assert!(models[1].efforts.is_empty());
    }

    /// Ignored by default: it runs the Codex CLI the machine has, if it has one.
    /// `cargo test -p patchwork-server -- --ignored reads_the_installed_codex`.
    #[test]
    #[ignore = "needs the Codex CLI"]
    fn reads_the_installed_codex() {
        let info = info().expect("the Codex CLI answers");
        assert!(info.version.starts_with("codex-cli"), "{}", info.version);
        assert!(
            info.features
                .iter()
                .any(|feature| feature.stage == "stable"),
            "the feature list parses: {} flags",
            info.features.len()
        );
        assert!(
            info.features
                .iter()
                .all(|feature| !feature.stage.is_empty()),
            "every flag has a stage"
        );
        assert!(!info.models.is_empty(), "the model catalog parses");
        assert!(
            info.models
                .iter()
                .any(|model| model.context_window.is_some())
        );
        let doctor = command(InfoCommand::Doctor).expect("codex doctor answers");
        assert!(!doctor.trim().is_empty());
        eprintln!(
            "{}: {} flags ({} stable, {} on), {} models",
            info.version,
            info.features.len(),
            info.features.iter().filter(|f| f.stage == "stable").count(),
            info.features.iter().filter(|f| f.enabled).count(),
            info.models.len()
        );
    }

    #[test]
    fn rubbish_is_not_a_catalog() {
        assert!(parse_models("no json here").is_empty());
        assert!(parse_models("{}").is_empty());
    }
}
