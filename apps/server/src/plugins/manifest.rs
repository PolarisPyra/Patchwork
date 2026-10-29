//! Reading a bundle's manifest: its name, the hooks it runs, and the apps it names, Codex's format included.

use std::path::Path;
use serde_json::Value;
use super::{HookEvent, Plugin, PluginApp, PluginHook, PluginHookHandler};

/// Where `plugin.json` sits inside a bundle: Patchwork's own place, then Codex's.
const MANIFESTS: [&str; 2] = [".patchwork-plugin/plugin.json", ".codex-plugin/plugin.json"];

/// Codex writes `plugin.json` as a flat file in some bundles; so do we.
const ROOT_MANIFEST: &str = "plugin.json";

/// How long a hook command may run when its manifest does not say, and the bounds on what it may.
pub(super) const DEFAULT_TIMEOUT_SECS: u64 = 60;

const TIMEOUT_RANGE: (u64, u64) = (1, 600);

/// Reads one bundle's folder. `None` when it has no manifest or the manifest is not JSON. Each
/// part is read on its own, so a part this version does not understand is left empty rather than
/// losing the bundle.
pub(super) fn read_bundle(dir: &Path, enabled: bool) -> Option<Plugin> {
    let manifest_path = MANIFESTS
        .iter()
        .map(|rel| dir.join(rel))
        .chain(std::iter::once(dir.join(ROOT_MANIFEST)))
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
    Some(Plugin {
        id,
        name: string("name").unwrap_or_else(|| "Plugin".into()),
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

/// A part of the manifest that Codex nests under its own key, `{ "<key>": { … } }`, either inline
/// or in a file the manifest names by path relative to the bundle.
fn section(dir: &Path, value: Option<&Value>, key: &str) -> Option<Value> {
    let outer = match value? {
        Value::String(path) => {
            serde_json::from_str(&std::fs::read_to_string(dir.join(path)).ok()?).ok()?
        }
        other => other.clone(),
    };
    outer.get(key).cloned()
}

/// The hooks in `{ "<Event>": [ { "hooks": [ handler, … ] } ] }`, sorted by event. Events this
/// version does not know are skipped.
pub(super) fn hooks(events: &Value) -> Vec<PluginHook> {
    let Some(events) = events.as_object() else {
        return Vec::new();
    };
    let mut hooks: Vec<PluginHook> = events
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
                        .map(move |handler| PluginHook {
                            event,
                            matcher: matcher.clone(),
                            handler,
                        })
                })
        })
        .collect();
    hooks.sort_by_key(|hook| hook.event);
    hooks
}

/// One handler, by its `type`. A command without its command line is skipped.
pub(super) fn handler(value: &Value) -> Option<PluginHookHandler> {
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let kind = field("type").unwrap_or_else(|| "command".into());
    Some(match kind.to_ascii_lowercase().replace('_', "").as_str() {
        "command" => PluginHookHandler::Command {
            command: field("command").filter(|command| !command.trim().is_empty())?,
            timeout_secs: timeout(value),
        },
        "builtin" => PluginHookHandler::Builtin {
            name: field("name").filter(|name| !name.trim().is_empty())?,
            timeout_secs: timeout(value),
        },
        "mcptool" => PluginHookHandler::McpTool {
            server: field("server").unwrap_or_default(),
            tool: field("tool").unwrap_or_default(),
        },
        "prompt" => PluginHookHandler::Prompt,
        "agent" => PluginHookHandler::Agent,
        _ => PluginHookHandler::Unsupported { kind },
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
pub(super) fn apps(apps: &Value) -> Vec<PluginApp> {
    let Some(apps) = apps.as_object() else {
        return Vec::new();
    };
    let mut apps: Vec<PluginApp> = apps
        .iter()
        .map(|(name, app)| PluginApp {
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
