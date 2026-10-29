//! Plugins: a folder with a `plugin.json`, the hooks it runs on turn events, and the apps it
//! names. Skills belong in the skills folder and MCP servers in Settings → MCP Servers, so a
//! plugin carries neither. The format is Codex's `.codex-plugin/plugin.json` where it can be, so
//! a Codex bundle works as it is. The app ships none: a plugin's hooks can go in a `hooks.json`
//! beside its manifest, which the manifest names as `"hooks": "./hooks.json"`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::types::{Plugin, PluginApp, PluginHook, PluginHookHandler, Settings};

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the config folder, where the default plugins folder is.
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
}

fn default_dir() -> PathBuf {
    CONFIG_DIR
        .get()
        .cloned()
        .unwrap_or_default()
        .join("plugins")
}

/// The plugins folder in use.
pub fn dir(settings: &Settings) -> PathBuf {
    match settings.plugins_dir.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_dir(),
    }
}

/// Where `plugin.json` sits inside a bundle: Patchwork's own place, then Codex's.
const MANIFESTS: [&str; 2] = [".patchwork-plugin/plugin.json", ".codex-plugin/plugin.json"];

/// Codex writes `plugin.json` as a flat file in some bundles; so do we.
const ROOT_MANIFEST: &str = "plugin.json";

/// The turn events a hook can run on, as Patchwork names them. A manifest's keys match these
/// whatever their case, so Codex's `Stop` is `stop`.
pub const EVENTS: [&str; 12] = [
    "userPromptSubmit",
    "sessionStart",
    "preToolUse",
    "permissionRequest",
    "postToolUse",
    "preCompact",
    "postCompact",
    "subagentStart",
    "subagentStop",
    "stop",
    "interrupt",
    "sessionEnd",
];

/// How long a hook command may run when its manifest does not say, and the bounds on what it may.
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const TIMEOUT_RANGE: (u64, u64) = (1, 600);

/// Reads one bundle's folder. `None` when it has no manifest or the manifest is not JSON. Each
/// part is read on its own, so a part this version does not understand is left empty rather than
/// losing the bundle.
fn read_bundle(dir: &Path, enabled: bool) -> Option<Plugin> {
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
fn hooks(events: &Value) -> Vec<PluginHook> {
    let Some(events) = events.as_object() else {
        return Vec::new();
    };
    let mut hooks: Vec<PluginHook> = events
        .iter()
        .filter_map(|(key, groups)| {
            let event = EVENTS
                .iter()
                .find(|event| event.eq_ignore_ascii_case(key))?;
            Some((*event, groups))
        })
        .flat_map(|(event, groups)| {
            groups
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|group| group.get("hooks")?.as_array())
                .flatten()
                .filter_map(handler)
                .map(move |handler| PluginHook {
                    event: event.to_string(),
                    handler,
                })
        })
        .collect();
    hooks.sort_by_key(|hook| EVENTS.iter().position(|event| *event == hook.event));
    hooks
}

/// One handler, by its `type`. A command without its command line is skipped.
fn handler(value: &Value) -> Option<PluginHookHandler> {
    let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let kind = field("type").unwrap_or_else(|| "command".into());
    Some(match kind.to_ascii_lowercase().replace('_', "").as_str() {
        "command" => PluginHookHandler::Command {
            command: field("command").filter(|command| !command.trim().is_empty())?,
            timeout_secs: ["timeoutSec", "timeout"]
                .iter()
                .find_map(|key| value.get(key).and_then(Value::as_u64))
                .unwrap_or(DEFAULT_TIMEOUT_SECS)
                .clamp(TIMEOUT_RANGE.0, TIMEOUT_RANGE.1),
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

/// The apps in `{ "<name>": { "id": …, "required": … } }`, by name.
fn apps(apps: &Value) -> Vec<PluginApp> {
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

/// Every plugin in the plugins folder, by name.
pub fn list(settings: &Settings) -> Vec<Plugin> {
    let root = dir(settings);
    let mut plugins: Vec<Plugin> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            let enabled = !settings.disabled_plugins.contains(&id);
            read_bundle(&path, enabled)
        })
        .collect();
    plugins.sort_by_key(|plugin| plugin.name.to_lowercase());
    plugins
}

/// The turn a hook runs for, as its environment tells it.
pub struct HookContext<'a> {
    pub event: &'a str,
    pub thread_id: &'a str,
    /// The turn's project folder, where the hook runs.
    pub root: &'a str,
    pub agent: &'a str,
    /// How the turn's reply ended, for the events that end one.
    pub status: Option<&'a str>,
}

/// Runs the enabled plugins' command hooks for `context.event`, one after another, in the user's
/// shell in the project folder. Returns `"<plugin name>: <why>"` for each one that failed; a
/// failed hook never stops the turn.
pub fn run_hooks(settings: &Settings, context: &HookContext<'_>) -> Vec<String> {
    let commands: Vec<(String, String, u64)> = list(settings)
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .flat_map(|plugin| {
            let name = plugin.name;
            plugin
                .hooks
                .into_iter()
                .filter(|hook| hook.event == context.event)
                .filter_map(move |hook| match hook.handler {
                    PluginHookHandler::Command {
                        command,
                        timeout_secs,
                    } => Some((name.clone(), command, timeout_secs)),
                    PluginHookHandler::McpTool { .. }
                    | PluginHookHandler::Prompt
                    | PluginHookHandler::Agent
                    | PluginHookHandler::Unsupported { .. } => None,
                })
        })
        .collect();
    if commands.is_empty() {
        return Vec::new();
    }
    let Some(shell) = crate::effective_shell(None) else {
        return vec!["hooks: no shell to run them in".into()];
    };
    commands
        .into_iter()
        .filter_map(|(name, command, timeout_secs)| {
            run_command(&shell, &command, timeout_secs, context)
                .err()
                .map(|why| format!("{name}: {why}"))
        })
        .collect()
}

/// Runs one hook command, killing it at its timeout. Its output goes to the log.
fn run_command(
    shell: &str,
    command: &str,
    timeout_secs: u64,
    context: &HookContext<'_>,
) -> Result<(), String> {
    let mut cmd = crate::host_command(shell);
    cmd.current_dir(context.root)
        .env("PATCHWORK_EVENT", context.event)
        .env("PATCHWORK_THREAD", context.thread_id)
        .env("PATCHWORK_CWD", context.root)
        .env("PATCHWORK_AGENT", context.agent)
        .env("PATCHWORK_STATUS", context.status.unwrap_or_default());
    let timeout = Duration::from_secs(timeout_secs);
    let args = crate::shell_args(shell, command);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let probe = crate::provider::run_probe(cmd, &args, timeout, |_| false)
        .ok_or_else(|| format!("did not start, or ran past {timeout_secs}s and was stopped"))?;
    let stdout = probe.stdout.trim();
    if !stdout.is_empty() {
        log::info!("plugin hook {}: {stdout}", context.event);
    }
    if probe.success {
        Ok(())
    } else {
        let stderr = probe.stderr.trim();
        Err(if stderr.is_empty() {
            "exited with an error".into()
        } else {
            stderr.to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(crate::types::new_id());
        std::fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

    fn settings_with(plugins_dir: PathBuf, disabled: &[&str]) -> Settings {
        Settings {
            plugins_dir: Some(plugins_dir.to_string_lossy().into_owned()),
            disabled_plugins: disabled.iter().map(|id| (*id).to_string()).collect(),
            ..Settings::default()
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(path, text).expect("a file");
    }

    fn command(command: &str, timeout_secs: u64) -> PluginHookHandler {
        PluginHookHandler::Command {
            command: command.into(),
            timeout_secs,
        }
    }

    #[test]
    fn reads_a_codex_bundle_with_hooks_and_apps() {
        let root = temp_dir();
        write(
            &root.join("codexish/.codex-plugin/plugin.json"),
            r#"{
                "name": "Codexish",
                "version": "1.2.3",
                "hooks": { "hooks": {
                    "Stop": [ { "hooks": [ { "type": "command", "command": "echo done", "timeoutSec": 10 } ] } ],
                    "UserPromptSubmit": [ { "hooks": [
                        { "type": "command", "command": "echo hi" },
                        { "type": "mcp_tool", "server": "notes", "tool": "save" },
                        { "type": "prompt" },
                        { "type": "webhook" }
                    ] } ],
                    "NotAnEvent": [ { "hooks": [ { "type": "command", "command": "echo no" } ] } ]
                } },
                "apps": { "apps": { "figma": { "id": "asdk_figma", "required": true }, "linear": { "id": "asdk_linear" } } }
            }"#,
        );
        let plugins = list(&settings_with(root, &[]));
        assert_eq!(plugins.len(), 1);
        let plugin = &plugins[0];
        assert_eq!(plugin.id, "codexish");
        assert_eq!(plugin.name, "Codexish");
        assert_eq!(plugin.version.as_deref(), Some("1.2.3"));
        let events: Vec<&str> = plugin
            .hooks
            .iter()
            .map(|hook| hook.event.as_str())
            .collect();
        assert_eq!(
            events,
            [
                "userPromptSubmit",
                "userPromptSubmit",
                "userPromptSubmit",
                "userPromptSubmit",
                "stop"
            ]
        );
        assert_eq!(
            plugin.hooks[0].handler,
            command("echo hi", DEFAULT_TIMEOUT_SECS)
        );
        assert_eq!(
            plugin.hooks[1].handler,
            PluginHookHandler::McpTool {
                server: "notes".into(),
                tool: "save".into()
            }
        );
        assert_eq!(plugin.hooks[2].handler, PluginHookHandler::Prompt);
        assert_eq!(
            plugin.hooks[3].handler,
            PluginHookHandler::Unsupported {
                kind: "webhook".into()
            }
        );
        assert_eq!(plugin.hooks[4].handler, command("echo done", 10));
        assert_eq!(
            plugin.apps,
            [
                PluginApp {
                    name: "figma".into(),
                    id: "asdk_figma".into(),
                    required: true
                },
                PluginApp {
                    name: "linear".into(),
                    id: "asdk_linear".into(),
                    required: false
                },
            ]
        );
        assert!(plugin.enabled);
    }

    #[test]
    fn reads_hooks_and_apps_from_files_the_manifest_names() {
        let root = temp_dir();
        write(
            &root.join("demo/plugin.json"),
            r#"{"name":"Demo","hooks":"./hooks.json","apps":"./.app.json"}"#,
        );
        write(
            &root.join("demo/hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
        );
        write(
            &root.join("demo/.app.json"),
            r#"{"apps":{"figma":{"id":"asdk_figma","required":true}}}"#,
        );
        let plugins = list(&settings_with(root, &[]));
        assert_eq!(plugins[0].hooks.len(), 1);
        assert_eq!(plugins[0].hooks[0].event, "stop");
        assert_eq!(plugins[0].apps.len(), 1);
        assert_eq!(plugins[0].apps[0].id, "asdk_figma");
    }

    /// A part in a shape this version does not read used to fail the whole manifest, and the
    /// bundle vanished along with its hooks.
    #[test]
    fn a_part_it_cannot_read_leaves_the_rest_of_the_bundle() {
        let root = temp_dir();
        write(
            &root.join("demo/.patchwork-plugin/plugin.json"),
            r#"{"name":"Demo","apps":[],"hooks":{"hooks":{"stop":[{"hooks":[{"type":"command","command":"true"}]}]}}}"#,
        );
        let plugins = list(&settings_with(root, &[]));
        assert_eq!(plugins.len(), 1);
        assert!(plugins[0].apps.is_empty());
        assert_eq!(plugins[0].hooks.len(), 1);
        assert_eq!(plugins[0].hooks[0].event, "stop");
    }

    #[test]
    fn timeouts_are_kept_within_bounds() {
        let parse = |timeout: u64| {
            handler(&serde_json::json!({ "command": "true", "timeoutSec": timeout }))
        };
        assert_eq!(parse(0), Some(command("true", 1)));
        assert_eq!(parse(9999), Some(command("true", 600)));
    }

    #[test]
    fn runs_an_enabled_plugins_command_hooks_in_the_project() {
        let root = temp_dir();
        let project = temp_dir();
        write(
            &root.join("demo/plugin.json"),
            r#"{"name":"Demo","hooks":{"hooks":{
                "stop":[{"hooks":[
                    {"type":"command","command":"printf '%s %s %s' \"$PATCHWORK_EVENT\" \"$PATCHWORK_AGENT\" \"$PATCHWORK_STATUS\" > hook.txt"},
                    {"type":"command","command":"echo broke >&2; exit 3"}
                ]}],
                "sessionEnd":[{"hooks":[{"type":"command","command":"touch ended"}]}]
            }}}"#,
        );
        let project_path = project.to_string_lossy().into_owned();
        let context = HookContext {
            event: "stop",
            thread_id: "t1",
            root: &project_path,
            agent: "claude",
            status: Some("complete"),
        };
        let failures = run_hooks(&settings_with(root.clone(), &[]), &context);
        assert_eq!(failures, ["Demo: broke"]);
        assert_eq!(
            std::fs::read_to_string(project.join("hook.txt")).expect("the hook wrote"),
            "stop claude complete"
        );
        assert!(
            !project.join("ended").exists(),
            "only the event's own hooks run"
        );
        // A disabled plugin runs nothing.
        std::fs::remove_file(project.join("hook.txt")).expect("remove");
        assert!(run_hooks(&settings_with(root, &["demo"]), &context).is_empty());
        assert!(!project.join("hook.txt").exists());
    }

    #[test]
    fn a_hook_past_its_timeout_is_stopped() {
        let root = temp_dir();
        let project = temp_dir();
        write(
            &root.join("slow/plugin.json"),
            r#"{"name":"Slow","hooks":{"hooks":{"stop":[{"hooks":[{"type":"command","command":"sleep 30","timeoutSec":1}]}]}}}"#,
        );
        let project_path = project.to_string_lossy().into_owned();
        let context = HookContext {
            event: "stop",
            thread_id: "t1",
            root: &project_path,
            agent: "codex",
            status: None,
        };
        let started = std::time::Instant::now();
        let failures = run_hooks(&settings_with(root, &[]), &context);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("Slow: "));
    }

    #[test]
    fn a_folder_without_a_manifest_is_not_a_plugin() {
        let root = temp_dir();
        write(
            &root.join("plain/skills/one/SKILL.md"),
            "---\nname: one\n---\n",
        );
        assert!(list(&settings_with(root, &[])).is_empty());
    }
}
