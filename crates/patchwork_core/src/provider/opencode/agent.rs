//! OpenCode behind the provider interface. What it can do depends on its version: 2.x runs
//! through Patchwork's bridge to `opencode serve`, 1.x as one `opencode run` per turn.

// Rust guideline compliant 2026-10-09

use crate::provider::cli_info::{
    CliFeature, CliPage, CliReading, CliRows, InfoCommand, untagged_models,
};
use std::collections::HashMap;

use serde_json::Value;

use crate::permissions::Target;
use crate::provider::agent::{AgentProvider, Capabilities, Identity, Logo};
use crate::provider::mcp::overrides::opencode_config;
use crate::provider::turn::prompt_text;
use crate::provider::{AgentKind, Parsed, Prompt, TurnSpec};

/// OpenCode, through its server's bridge or as `opencode run`.
#[derive(Debug)]
pub(crate) struct OpenCode;

impl AgentProvider for OpenCode {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn identity(&self) -> Identity {
        Identity {
            id: "opencode",
            name: "OpenCode",
            title: "OpenCode CLI",
            logo: Logo::File {
                file: "opencode.svg",
                mono: true,
            },
            cli: Some("opencode"),
        }
    }

    // `opencode run` reads the prompt from stdin to its end before it starts.
    fn capabilities(&self) -> Capabilities {
        let serve = super::uses_serve();
        Capabilities {
            streams_text: serve,
            closes_input: !serve,
            keeps_provider_keys: true,
            patchwork_runs_hooks: true,
            custom_agents: true,
            skills_folder: Some("opencode"),
            checklist_tool: Some("todowrite tool"),
            blocks_at_launch: !serve,
            runs_in_patchwork: serve,
            ..Capabilities::BASE
        }
    }

    fn locate(&self) -> Option<std::path::PathBuf> {
        super::opencode_bin()
    }

    fn command_env(&self, remote: bool) -> Vec<(&'static str, String)> {
        if !remote && super::needs_own_database() {
            vec![("OPENCODE_DB", super::V1_DATABASE.to_string())]
        } else {
            Vec::new()
        }
    }

    fn sign_in(&self, status: &mut crate::provider::AgentStatus) {
        super::auth(status);
    }

    fn models(&self) -> (Option<String>, Vec<crate::provider::ModelOption>) {
        (None, super::models())
    }

    fn cli_page(&self) -> Option<CliPage> {
        Some(CliPage {
            name: "opencode",
            commands: &[
                ("Agents", "debug agents", InfoCommand::Agents),
                ("Paths", "debug paths", InfoCommand::Paths),
                ("MCP servers", "mcp list", InfoCommand::McpList),
                ("Plugins", "plugin list", InfoCommand::PluginList),
            ],
            rows: Some(CliRows::Settings),
            filter: None,
            empty: "No config file sets anything yet.",
        })
    }

    // Its rows are the settings in its config files.
    fn read_cli(&self) -> Result<CliReading, crate::Error> {
        super::cli::info().map(|i| CliReading {
            path: i.path,
            version: i.version,
            features: i
                .settings
                .into_iter()
                .map(|s| CliFeature {
                    name: s.name,
                    detail: s.value,
                    enabled: None,
                    kept: true,
                    source: Some(s.source),
                })
                .collect(),
            models: untagged_models(i.models),
        })
    }

    fn run_cli_command(&self, command: InfoCommand) -> Result<String, crate::Error> {
        super::cli::command(command)
    }

    fn turn_args(&self, spec: &TurnSpec) -> Vec<String> {
        super::turn_args(spec)
    }

    fn turn_env(&self, spec: &TurnSpec) -> Vec<(&'static str, String)> {
        // The bridge sets up its own server, which inherits this environment. 2.x takes the
        // system prompt from the bridge, as an instruction entry on the session.
        if super::uses_serve() {
            return super::config_content([
                super::skills_config(spec),
                super::agents_config(spec),
                super::todo_config(),
                opencode_config(spec.chat_id),
            ])
            .map(|config| ("OPENCODE_CONFIG_CONTENT", config))
            .into_iter()
            .collect();
        }
        std::iter::once(("PWD", spec.root.to_string()))
            .chain(
                super::config_content([
                    super::permission_config(spec),
                    super::agents_config(spec),
                    super::instructions_config(spec),
                    opencode_config(spec.chat_id),
                ])
                .map(|config| ("OPENCODE_CONFIG_CONTENT", config)),
            )
            .collect()
    }

    // The bridge reads its turn from stdin, and answers and steers after it.
    fn turn_input(&self, spec: &TurnSpec) -> Vec<String> {
        if super::uses_serve() {
            vec![super::bridge_request(spec)]
        } else {
            super::turn_input(spec)
        }
    }

    fn parse(&self, value: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
        super::parse(value, spec, message_id)
    }

    // `opencode run` reads one message per run; later ones start the next turn.
    fn steer_input(
        &self,
        prompt: Prompt<'_>,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        super::uses_serve().then(|| {
            super::steer_line(
                &prompt_text(prompt.text, prompt.attachments),
                prompt.attachments,
            )
        })
    }

    // `opencode run` reads nothing once it starts; only the bridge to `opencode serve` can.
    fn interrupt(&self, _session_id: Option<&str>, _turn_id: Option<&str>) -> Option<String> {
        super::uses_serve().then(super::interrupt_line)
    }

    // `opencode run` runs headless and never asks.
    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String {
        if super::uses_serve() {
            super::approval_reply(reply, allow, answers)
        } else {
            String::new()
        }
    }

    fn mode_asks(&self, spec: &TurnSpec, target: &Target) -> bool {
        super::mode_asks(spec.mode, target.kind)
    }
}
