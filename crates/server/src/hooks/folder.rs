//! The hooks folder, and the bundles in it.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::settings::Settings;

use super::{Bundle, read_bundle};

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the config folder, where the default hooks folder is.
///
/// The folder was called `plugins` before, so one left from then is renamed to `hooks`.
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    let (old, new) = (config_dir.join("plugins"), config_dir.join("hooks"));
    if old.is_dir()
        && !new.exists()
        && let Err(e) = std::fs::rename(&old, &new)
    {
        log::warn!(error:% = e; "could not rename the plugins folder to hooks: {{error}}");
    }
}

fn default_dir() -> PathBuf {
    CONFIG_DIR.get().cloned().unwrap_or_default().join("hooks")
}

/// The hooks folder in use.
pub fn dir(settings: &Settings) -> PathBuf {
    match settings.hooks_dir.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_dir(),
    }
}

/// Every bundle in the hooks folder, by name.
pub fn list(settings: &Settings) -> Vec<Bundle> {
    let root = dir(settings);
    let mut bundles: Vec<Bundle> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let path = entry.path();
            let id = entry.file_name().to_string_lossy().into_owned();
            let enabled = !settings.disabled_hooks.contains(&id);
            read_bundle(&path, enabled)
        })
        .collect();
    bundles.sort_by_key(|bundle| bundle.name.to_lowercase());
    bundles
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::manifest::{DEFAULT_TIMEOUT_SECS, handler};
    use super::super::{HookContext, HookEvent, BundleApp, HookHandler, run_hooks};
    use std::time::Duration;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(crate::system::new_id());
        std::fs::create_dir_all(&dir).expect("a temp dir");
        dir
    }

    fn settings_with(hooks_dir: &Path, disabled: &[&str]) -> Settings {
        Settings {
            hooks_dir: Some(hooks_dir.to_string_lossy().into_owned()),
            disabled_hooks: disabled.iter().map(|id| (*id).to_string()).collect(),
            ..Settings::default()
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
        std::fs::write(path, text).expect("a file");
    }

    fn command(command: &str, timeout_secs: u64) -> HookHandler {
        HookHandler::Command {
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
        let bundles = list(&settings_with(&root, &[]));
        assert_eq!(bundles.len(), 1);
        let bundle = &bundles[0];
        assert_eq!(bundle.id, "codexish");
        assert_eq!(bundle.name, "Codexish");
        assert_eq!(bundle.version.as_deref(), Some("1.2.3"));
        let events: Vec<&str> = bundle.hooks.iter().map(|hook| hook.event.name()).collect();
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
            bundle.hooks[0].handler,
            command("echo hi", DEFAULT_TIMEOUT_SECS)
        );
        assert_eq!(
            bundle.hooks[1].handler,
            HookHandler::McpTool {
                server: "notes".into(),
                tool: "save".into()
            }
        );
        assert_eq!(bundle.hooks[2].handler, HookHandler::Prompt);
        assert_eq!(
            bundle.hooks[3].handler,
            HookHandler::Unsupported {
                kind: "webhook".into()
            }
        );
        assert_eq!(bundle.hooks[4].handler, command("echo done", 10));
        assert_eq!(
            bundle.apps,
            [
                BundleApp {
                    name: "figma".into(),
                    id: "asdk_figma".into(),
                    required: true
                },
                BundleApp {
                    name: "linear".into(),
                    id: "asdk_linear".into(),
                    required: false
                },
            ]
        );
        assert!(bundle.enabled);
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
        let bundles = list(&settings_with(&root, &[]));
        assert_eq!(bundles[0].hooks.len(), 1);
        assert_eq!(bundles[0].hooks[0].event, HookEvent::Stop);
        assert_eq!(bundles[0].apps.len(), 1);
        assert_eq!(bundles[0].apps[0].id, "asdk_figma");
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
        let bundles = list(&settings_with(&root, &[]));
        assert_eq!(bundles.len(), 1);
        assert!(bundles[0].apps.is_empty());
        assert_eq!(bundles[0].hooks.len(), 1);
        assert_eq!(bundles[0].hooks[0].event, HookEvent::Stop);
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
    fn runs_an_enabled_bundles_command_hooks_in_the_project() {
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
            event: HookEvent::Stop,
            thread_id: "t1",
            root: &project_path,
            agent: "claude",
            status: Some("complete"),
        };
        let failures = run_hooks(&settings_with(&root, &[]), &context);
        assert_eq!(failures, ["Demo: broke"]);
        assert_eq!(
            std::fs::read_to_string(project.join("hook.txt")).expect("the hook wrote"),
            "stop claude complete"
        );
        assert!(
            !project.join("ended").exists(),
            "only the event's own hooks run"
        );
        // A disabled bundle runs nothing.
        std::fs::remove_file(project.join("hook.txt")).expect("remove");
        assert!(run_hooks(&settings_with(&root, &["demo"]), &context).is_empty());
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
            event: HookEvent::Stop,
            thread_id: "t1",
            root: &project_path,
            agent: "codex",
            status: None,
        };
        let started = std::time::Instant::now();
        let failures = run_hooks(&settings_with(&root, &[]), &context);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("Slow: "));
    }

    /// A bundle of hooks alone is one `hooks.json`, which names itself and holds its hooks, so
    /// there is no `plugin.json` beside it to keep in step.
    #[test]
    fn reads_a_hooks_only_bundle() {
        let root = temp_dir();
        write(
            &root.join("patchwork/hooks.json"),
            r#"{"name":"Patchwork","description":"Patchwork's own hooks.","hooks":{
                "preToolUse":[{"hooks":[{"type":"command","command":"exit 0","timeoutSec":10}]}]
            }}"#,
        );
        let bundles = list(&settings_with(&root, &[]));
        assert_eq!(bundles.len(), 1);
        let bundle = &bundles[0];
        assert_eq!(bundle.id, "patchwork");
        assert_eq!(bundle.name, "Patchwork");
        assert_eq!(
            bundle.description.as_deref(),
            Some("Patchwork's own hooks.")
        );
        assert_eq!(bundle.hooks.len(), 1);
        let hook = &bundle.hooks[0];
        assert_eq!(hook.event, HookEvent::PreToolUse);
        assert_eq!(
            hook.handler,
            HookHandler::Command {
                command: "exit 0".into(),
                timeout_secs: 10,
            }
        );
    }

    #[test]
    fn a_folder_without_a_manifest_is_not_a_bundle() {
        let root = temp_dir();
        write(
            &root.join("plain/skills/one/SKILL.md"),
            "---\nname: one\n---\n",
        );
        assert!(list(&settings_with(&root, &[])).is_empty());
    }
}
