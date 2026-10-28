//! What the installed Codex CLI offers, read from the CLI itself: its version, its feature
//! flags, its model catalog, and the read-only diagnostics commands. Patchwork asks the binary
//! rather than keeping a copy of these lists, so a newer Codex shows up as it is.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::types::{CodexFeature, CodexInfo, CodexModel};

/// A read-only command, the only kind this module runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoCommand {
    /// `codex doctor`: installation, config, auth and runtime health.
    Doctor,
    /// `codex mcp list`: the MCP servers Codex has configured.
    McpList,
    /// `codex plugin list`: the plugins Codex has installed.
    PluginList,
}

impl InfoCommand {
    fn args(self) -> &'static [&'static str] {
        match self {
            InfoCommand::Doctor => &["doctor"],
            InfoCommand::McpList => &["mcp", "list"],
            InfoCommand::PluginList => &["plugin", "list"],
        }
    }
}

/// Longest a probe may take before it is killed. `doctor` is the slow one.
const TIMEOUT: Duration = Duration::from_secs(60);
/// Longest output kept from a command. The model catalog is nearly 700 KB of JSON, so this is
/// generous; it is a guard against a runaway command, not a display limit.
const MAX_PIPE: usize = 8 * 1024 * 1024;
/// Longest output shown for a command whose output is for a reader, such as `doctor`.
const MAX_OUTPUT: usize = 60_000;

/// The Codex CLI in use, if it is installed.
pub fn binary() -> Option<PathBuf> {
    crate::provider::codex_binary()
}

/// Runs the CLI with `args` and returns its stdout, then its stderr when stdout was empty.
///
/// Both pipes are read on threads of their own while the process runs. Its output can be far
/// larger than a pipe holds, and a process whose pipe fills up stops, so waiting for it to exit
/// before reading anything would hang on exactly the commands worth reading: the model catalog is
/// nearly 700 KB.
fn run(args: &[&str]) -> Result<String, String> {
    let binary = binary().ok_or("The Codex CLI is not installed")?;
    let mut child = Command::new(&binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run {}: {e}", binary.display()))?;
    let out = child.stdout.take().map(drain);
    let err = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("codex {} took too long", args.join(" ")));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("Could not run codex: {e}")),
        }
    };
    // The threads end when the process closes its pipes, which it has.
    let stdout = out
        .map(|out| out.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = err
        .map(|err| err.join().unwrap_or_default())
        .unwrap_or_default();
    let text = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    if !status.success() && text.trim().is_empty() {
        return Err(format!("codex {} failed", args.join(" ")));
    }
    Ok(text)
}

/// Reads a pipe to its end, keeping the first `MAX_PIPE` bytes and dropping the rest, so a long
/// output neither stops the process nor fills memory.
fn drain<R: std::io::Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = MAX_PIPE.saturating_sub(kept.len());
                    kept.extend_from_slice(&buffer[..n.min(room)]);
                }
            }
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
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

fn models() -> Vec<CodexModel> {
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

/// Runs one of the read-only diagnostics commands and returns its output, trimmed to what a
/// reader wants to see.
pub fn command(kind: InfoCommand) -> Result<String, String> {
    let text = run(kind.args())?;
    if text.len() <= MAX_OUTPUT {
        return Ok(text);
    }
    let mut end = MAX_OUTPUT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(format!("{}\n…", &text[..end]))
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
