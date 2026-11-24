//! The agent CLIs Patchwork runs over the Agent Client Protocol beside Antigravity: one table of
//! how each starts, installs, signs in and updates, and what sets them apart on the wire.
//!
//! Their turns run through the ACP client in [`super::antigravity`], which checks
//! [`super::TurnSpec::agent`] for what only `agy` does.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};

use super::{AgentKind, AgentStatus, ModelOption, find_binary, located_or};
use crate::chats::AccessMode;
use crate::system::Locked;
use crate::system::files::paths::home_dir;

/// How Patchwork runs, installs and updates one ACP agent CLI.
#[derive(Debug)]
pub(crate) struct AcpSpec {
    pub(crate) kind: AgentKind,
    /// The name errors give the agent.
    pub(crate) label: &'static str,
    /// The program's name on PATH.
    pub(crate) binary: &'static str,
    /// The arguments that start its ACP server on stdio.
    pub(crate) args: &'static [&'static str],
    pub(crate) env: &'static [(&'static str, &'static str)],
    /// Folders under the home folder its installer puts it in, beside the usual ones.
    pub(crate) install_dirs: &'static [&'static str],
    /// The official install script for macOS and Linux, run with bash; None installs from npm.
    pub(crate) install_script: Option<&'static str>,
    /// The official PowerShell install script; None installs from npm.
    pub(crate) install_script_windows: Option<&'static str>,
    pub(crate) npm_package: &'static str,
    /// What the user runs in a terminal to sign in.
    pub(crate) login: &'static str,
    /// Its own update command, and the first version that has it. None updates through npm by hand.
    pub(crate) update: Option<(&'static [&'static str], &'static str)>,
    /// The first version whose ACP server works.
    pub(crate) min_version: &'static str,
    /// Its session modes for Supervised, Accept edits, Auto and Full access. None sets no mode.
    pub(crate) modes: Option<[&'static str; 4]>,
    /// Its own name for `/compact`.
    pub(crate) compact: Option<&'static str>,
    /// Commands it lists that would end or replace the session Patchwork keeps.
    pub(crate) hidden_commands: &'static [&'static str],
}

/// Qwen Code, Kimi Code and Auggie.
const SPECS: [AcpSpec; 3] = [
    AcpSpec {
        kind: AgentKind::Qwen,
        label: "Qwen Code",
        binary: "qwen",
        args: &["--acp"],
        env: &[],
        install_dirs: &[".local/bin"],
        install_script: Some(
            "https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.sh",
        ),
        install_script_windows: Some(
            "https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.ps1",
        ),
        npm_package: "@qwen-code/qwen-code",
        login: "qwen, then /auth",
        update: Some((&["update"], "0.20.0")),
        min_version: "0.16.0",
        modes: Some(["default", "auto-edit", "auto", "yolo"]),
        compact: Some("/compress"),
        hidden_commands: &["compress", "clear", "reset", "new"],
    },
    AcpSpec {
        kind: AgentKind::Kimi,
        label: "Kimi Code",
        binary: "kimi",
        args: &["acp"],
        // A staged update would otherwise replace the binary while the turn starts.
        env: &[("KIMI_CODE_NO_AUTO_UPDATE", "1")],
        install_dirs: &[".kimi-code/bin"],
        install_script: Some("https://code.kimi.com/kimi-code/install.sh"),
        install_script_windows: None,
        npm_package: "@moonshot-ai/kimi-code",
        login: "kimi login",
        update: Some((&["update", "--yes"], "2.0.0")),
        min_version: "2.5.0",
        // Kimi's `yolo` still asks before risky commands, and its `auto` never asks.
        modes: Some(["default", "default", "yolo", "auto"]),
        compact: None,
        hidden_commands: &[],
    },
    AcpSpec {
        kind: AgentKind::Auggie,
        label: "Auggie",
        binary: "auggie",
        args: &["--acp"],
        env: &[],
        install_dirs: &[],
        install_script: None,
        install_script_windows: None,
        npm_package: "@augmentcode/auggie",
        login: "auggie auth login --headless",
        update: None,
        min_version: "1.2.0",
        modes: None,
        compact: None,
        hidden_commands: &[],
    },
];

/// Marks a config option Patchwork made from an older session result's `models` or `modes`,
/// which are set with `session/set_model` and `session/set_mode`.
pub(crate) const LEGACY: &str = "patchworkLegacy";

/// The models each agent's last session offered, which it lists only inside a session.
static OFFERED: Mutex<Vec<(AgentKind, Vec<ModelOption>)>> = Mutex::new(Vec::new());

/// `kind`'s spec, when it is one of the listed ACP agents.
pub(crate) fn spec(kind: AgentKind) -> Option<&'static AcpSpec> {
    SPECS.iter().find(|s| s.kind == kind)
}

/// The name errors give an ACP agent.
pub(crate) fn label(kind: AgentKind) -> &'static str {
    spec(kind).map_or("Antigravity", |s| s.label)
}

/// Where `kind`'s CLI is: the path the user chose, else PATH, the usual folders, or its own.
pub(crate) fn binary(spec: &AcpSpec) -> Option<PathBuf> {
    located_or(spec.kind, || {
        find_binary(spec.binary).or_else(|| {
            let home = home_dir()?;
            let name = if cfg!(windows) {
                format!("{}.exe", spec.binary)
            } else {
                spec.binary.to_string()
            };
            spec.install_dirs
                .iter()
                .map(|dir| home.join(dir).join(&name))
                .find(|p| p.is_file())
        })
    })
}

/// Fills in whether `status`'s CLI can run. These agents say whether they are signed in only
/// when a session starts, so an installed one reads as ready.
pub(crate) fn auth(spec: &AcpSpec, status: &mut AgentStatus) {
    let too_old = status
        .version
        .as_deref()
        .is_some_and(|v| super::cli_update::is_older(v, spec.min_version));
    if too_old {
        status.detail = format!("Needs version {} or newer", spec.min_version);
        return;
    }
    status.signed_in = true;
    status.detail = "Ready".into();
}

/// The session mode `mode` runs as, when the agent has modes.
pub(crate) fn mode_for(spec: &AcpSpec, mode: AccessMode) -> Option<&'static str> {
    let [supervised, accept_edits, auto, full] = spec.modes?;
    Some(match mode {
        AccessMode::Supervised => supervised,
        AccessMode::AutoAcceptEdits => accept_edits,
        AccessMode::Auto => auto,
        AccessMode::FullAccess => full,
    })
}

/// `text` with `/compact` renamed to the agent's own command for it.
pub(crate) fn own_command(kind: AgentKind, text: String) -> String {
    let Some(compact) = spec(kind).and_then(|s| s.compact) else {
        return text;
    };
    match text.trim_start().strip_prefix("/compact") {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            format!("{compact}{rest}")
        }
        _ => text,
    }
}

/// Whether the agent's command `name` is kept out of the slash menu.
pub(crate) fn hides_command(kind: AgentKind, name: &str) -> bool {
    spec(kind).is_some_and(|s| s.hidden_commands.contains(&name))
}

/// A session result's settings as `configOptions`, with older `models` and `modes` lists made
/// into options of their own.
pub(crate) fn config_options(result: &Value) -> Value {
    let mut options = result["configOptions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let has = |options: &[Value], category: &str| {
        options
            .iter()
            .any(|o| o["id"] == category || o["category"] == category)
    };
    let legacy = |category: &str, current: &Value, list: &Value, id_key: &str| {
        let choices: Vec<Value> = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let value = entry[id_key].as_str()?;
                Some(json!({"value": value, "name": entry["name"].as_str().unwrap_or(value)}))
            })
            .collect();
        (!choices.is_empty()).then(|| {
            json!({
                "id": category,
                "category": category,
                "currentValue": current,
                "options": choices,
                LEGACY: true,
            })
        })
    };
    if !has(&options, "model")
        && let Some(option) = legacy(
            "model",
            &result["models"]["currentModelId"],
            &result["models"]["availableModels"],
            "modelId",
        )
    {
        options.push(option);
    }
    if !has(&options, "mode")
        && let Some(option) = legacy(
            "mode",
            &result["modes"]["currentModeId"],
            &result["modes"]["availableModes"],
            "id",
        )
    {
        options.push(option);
    }
    Value::Array(options)
}

/// Remembers the models `config` offers, for the picker until the agent's next session.
pub(crate) fn remember_models(kind: AgentKind, config: &Value) {
    let Some(model) = config
        .as_array()
        .into_iter()
        .flatten()
        .find(|o| o["id"] == "model" || o["category"] == "model")
    else {
        return;
    };
    let models: Vec<ModelOption> = model["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|choice| {
            let id = choice["value"].as_str()?.trim();
            (!id.is_empty()).then(|| ModelOption {
                id: Some(id.to_string()),
                label: choice["name"].as_str().unwrap_or(id).trim().to_string(),
                default_model: None,
                is_legacy: false,
                efforts: None,
                context_window: None,
                needs_cli: None,
            })
        })
        .collect();
    if models.is_empty() {
        return;
    }
    let mut offered = OFFERED.locked();
    offered.retain(|(k, _)| *k != kind);
    offered.push((kind, models));
}

/// The models `kind`'s last session offered.
pub(crate) fn models(kind: AgentKind) -> Vec<ModelOption> {
    OFFERED
        .locked()
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, models)| models.clone())
        .unwrap_or_default()
}

/// The environment each listed agent's turn runs with, by kind.
pub(crate) fn env(kind: AgentKind) -> Vec<(&'static str, String)> {
    spec(kind)
        .map(|s| s.env.iter().map(|(k, v)| (*k, (*v).to_string())).collect())
        .unwrap_or_default()
}
