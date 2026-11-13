//! Reading a bundle's manifest: its name, the hooks it runs, and the apps it names, Codex's format included.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use serde_json::Value;
use super::{HookEvent, Bundle, BundleApp, Hook, HookHandler};
use crate::provider::AgentKind;

/// Where `plugin.json` sits inside a bundle: Patchwork's own place, then Codex's.
const MANIFESTS: [&str; 2] = [".patchwork-plugin/plugin.json", ".codex-plugin/plugin.json"];

/// Codex writes `plugin.json` as a flat file in some bundles; so do we.
const ROOT_MANIFEST: &str = "plugin.json";

/// A bundle that only carries hooks can be this one file, named for its folder unless it says
/// `name`, as Claude Code's `hooks.json` is.
const HOOKS_ONLY: &str = "hooks.json";

/// How long a hook command may run when its manifest does not say, and the bounds on what it may.
pub(super) const DEFAULT_TIMEOUT_SECS: u64 = 60;

const TIMEOUT_RANGE: (u64, u64) = (1, 600);

/// Reads one bundle's folder. `None` when it has no manifest or the manifest is not JSON. Each
/// part is read on its own, so a part this version does not understand is left empty rather than
/// losing the bundle.
pub(super) fn read_bundle(dir: &Path, enabled: bool) -> Option<Bundle> {
    let manifest_path = MANIFESTS
        .iter()
        .chain([&ROOT_MANIFEST, &HOOKS_ONLY])
        .map(|rel| dir.join(rel))
        .find(|path| path.is_file())?;
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest: Value = serde_json::from_str(&text).ok()?;
    let id = dir.file_name()?.to_string_lossy().into_owned();
    let string = |key: &str| {
        manifest
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    // Without a name it is called after its folder, `patchwork` reading as "Patchwork".
    let mut folder = id.chars();
    let named_for_folder = folder.next().map_or_else(
        || "Hooks".into(),
        |first| first.to_uppercase().chain(folder).collect(),
    );
    Some(Bundle {
        name: string("name").unwrap_or(named_for_folder),
        id,
        version: string("version"),
        description: string("description"),
        path: dir.to_string_lossy().into_owned(),
        hooks: section(dir, manifest.get("hooks"), "hooks")
            .map(|events| hooks(&events))
            .unwrap_or_default(),
        apps: section(dir, manifest.get("apps"), "apps")
            .map(|apps| self::apps(&apps))
            .unwrap_or_default(),
        enabled,
    })
}

/// A part of the manifest, either inline or in a file the manifest names by path relative to the
/// bundle. Codex nests it under its own key, `{ "<key>": { … } }`; without that key it is the part
/// itself, so `plugin.json` can hold its hooks as `"hooks": { "preToolUse": … }`.
fn section(dir: &Path, value: Option<&Value>, key: &str) -> Option<Value> {
    let outer = match value? {
        Value::String(path) => {
            serde_json::from_str(&std::fs::read_to_string(dir.join(path)).ok()?).ok()?
        }
        other => other.clone(),
    };
    Some(outer.get(key).cloned().unwrap_or(outer))
}

/// The hooks in `{ "<Event>": [ { "hooks": [ handler, … ] } ] }`, sorted by event. Events this
/// version does not know are skipped.
pub(super) fn hooks(events: &Value) -> Vec<Hook> {
    let Some(events) = events.as_object() else {
        return Vec::new();
    };
    let mut hooks: Vec<Hook> = events
        .iter()
        .filter_map(|(key, groups)| Some((HookEvent::parse(key)?, groups)))
        .flat_map(|(event, groups)| {
            groups
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(move |group| {
                    // `*` and no matcher both mean every tool.
                    let matcher = group
                        .get("matcher")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|m| !m.is_empty() && *m != "*")
                        .map(str::to_string);
                    group
                        .get("hooks")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(handler)
                        .map(move |handler| Hook {
                            event,
                            matcher: matcher.clone(),
                            run_by: run_by(event, &handler),
                            handler,
                        })
                })
        })
        .collect();
    hooks.sort_by_key(|hook| hook.event);
    hooks
}

/// The agents whose turns run a hook. Patchwork runs a command on a turn event itself, for every
/// agent; the rest run in the agents that run their event, and only a command runs at all.
fn run_by(event: HookEvent, handler: &HookHandler) -> Vec<AgentKind> {
    let runs = match handler {
        HookHandler::Command { .. } => true,
        HookHandler::McpTool { .. }
        | HookHandler::Prompt
        | HookHandler::Agent
        | HookHandler::Unsupported { .. } => false,
    };
    if !runs {
        return Vec::new();
    }
    AgentKind::ALL
        .into_iter()
        .filter(|&agent| event.runs_in_patchwork() || crate::provider::hooks::runs(agent, event))
        .collect()
}

/// One handler, by its `type`. A command without its command line is skipped.
pub(super) fn handler(value: &Value) -> Option<HookHandler> {
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let kind = field("type").unwrap_or_else(|| "command".into());
    Some(match kind.to_ascii_lowercase().replace('_', "").as_str() {
        "command" => HookHandler::Command {
            command: field("command").filter(|command| !command.trim().is_empty())?,
            timeout_secs: timeout(value),
        },
        "mcptool" => HookHandler::McpTool {
            server: field("server").unwrap_or_default(),
            tool: field("tool").unwrap_or_default(),
        },
        "prompt" => HookHandler::Prompt,
        "agent" => HookHandler::Agent,
        _ => HookHandler::Unsupported { kind },
    })
}

/// How long a handler may run: its `timeoutSec` or `timeout`, kept within bounds.
fn timeout(value: &Value) -> u64 {
    ["timeoutSec", "timeout"]
        .iter()
        .find_map(|key| value.get(key).and_then(Value::as_u64))
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(TIMEOUT_RANGE.0, TIMEOUT_RANGE.1)
}

/// The apps in `{ "<name>": { "id": …, "required": … } }`, by name.
pub(super) fn apps(apps: &Value) -> Vec<BundleApp> {
    let Some(apps) = apps.as_object() else {
        return Vec::new();
    };
    let mut apps: Vec<BundleApp> = apps
        .iter()
        .map(|(name, app)| BundleApp {
            name: name.clone(),
            id: app
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            required: app
                .get("required")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect();
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}
