//! Codex behind the provider interface.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use serde_json::Value;

use crate::chats::{BackgroundTask, ToolCall};
use crate::events::ChatEvent;
use crate::extensions::skills::Skill;
use crate::provider::cli_info::{
    CliFeature, CliModel, CliPage, CliReading, CliRows, InfoCommand, SHARED_COMMANDS,
};
use crate::provider::agent::{AgentProvider, Capabilities, Identity, Logo, ShellChoice, SkillCall};
use crate::settings::Settings;
use crate::provider::line_reader::{LineReader, Route};
use crate::provider::mcp::overrides::overrides;
use crate::provider::turn::prompt_text;
use crate::provider::{AgentKind, Parsed, Prompt, TurnSpec};

/// Codex, run as `codex app-server` over JSON-RPC.
#[derive(Debug)]
pub(crate) struct Codex;

impl AgentProvider for Codex {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn identity(&self) -> Identity {
        Identity {
            id: "codex",
            name: "Codex",
            title: "Codex",
            logo: Logo::File {
                file: "openai.svg",
                mono: true,
            },
            cli: Some("codex"),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            exits_on_stdin_close: true,
            lists_open_calls: true,
            subagent_chats: true,
            custom_agents: true,
            shell: ShellChoice::LoginPosix,
            skills_folder: Some("codex"),
            checklist_tool: Some("update_plan tool"),
            skill_call: SkillCall::Mention,
            runs_permission_hook: true,
            ..Capabilities::BASE
        }
    }

    fn sign_in(&self, status: &mut crate::provider::AgentStatus) {
        super::auth(status);
    }

    fn models(&self) -> (Option<String>, Vec<crate::provider::ModelOption>) {
        (super::configured_model(), super::models())
    }

    fn cli_page(&self) -> Option<CliPage> {
        Some(CliPage {
            name: "codex",
            commands: SHARED_COMMANDS,
            rows: Some(CliRows::Flags),
            filter: Some("Stable only"),
            empty: "Nothing matches.",
        })
    }

    // Its flags and catalog come from the binary.
    fn read_cli(&self) -> Result<CliReading, crate::Error> {
        super::cli::info().map(|i| CliReading {
            path: i.path,
            version: i.version,
            features: i
                .features
                .into_iter()
                .map(|f| CliFeature {
                    kept: f.stage == "stable",
                    name: f.name,
                    detail: f.stage,
                    enabled: Some(f.enabled),
                    source: None,
                })
                .collect(),
            models: i
                .models
                .into_iter()
                .map(|m| CliModel {
                    id: m.slug,
                    name: m.display_name,
                    context_window: m.context_window,
                    efforts: m.efforts,
                    tag: m.multi_agent,
                })
                .collect(),
        })
    }

    fn run_cli_command(&self, command: InfoCommand) -> Result<String, crate::Error> {
        super::cli::command(command)
    }

    // Codex takes them as the chat's developer instructions.
    fn instructions(&self, settings: &Settings, _skills: &[Skill]) -> Option<String> {
        Some(crate::orchestration::agent_instructions(
            settings,
            AgentKind::Codex,
        ))
        .filter(|text| !text.is_empty())
    }

    fn turn_args(&self, spec: &TurnSpec) -> Vec<String> {
        [
            super::turn_args(),
            overrides(AgentKind::Codex, spec.root, spec.chat_id),
            crate::provider::hooks::codex_args(spec),
            super::compact_args(spec),
        ]
        .concat()
    }

    fn turn_input(&self, spec: &TurnSpec) -> Vec<String> {
        super::turn_input(spec)
    }

    fn parse(&self, value: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
        super::parse(value, spec, message_id)
    }

    fn reader(&self) -> Box<dyn LineReader> {
        Box::new(CodexReader)
    }

    fn steer_input(
        &self,
        prompt: Prompt<'_>,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Option<String> {
        Some(super::steer_input(
            session_id?,
            turn_id?,
            &prompt_text(prompt.text, prompt.attachments),
            prompt.attachments,
        ))
    }

    fn interrupt(&self, session_id: Option<&str>, turn_id: Option<&str>) -> Option<String> {
        Some(super::interrupt(session_id?, turn_id?))
    }

    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String {
        super::approval_reply(reply, allow, answers)
    }

    fn own_key_vars(&self) -> &'static [&'static str] {
        &["OPENAI_API_KEY", "CODEX_API_KEY"]
    }

    fn background_task(&self, tool: &ToolCall) -> Option<BackgroundTask> {
        super::background_task(tool)
    }
}

/// Codex sends a subagent's notifications on the same stream, under the subagent's thread.
#[derive(Debug)]
struct CodexReader;

impl LineReader for CodexReader {
    // They go to its chat alone: its finished turn is not this one's. Its requests, such as
    // approvals, are answered here like any other.
    fn route(&self, value: &Value, session_id: Option<&str>) -> Route {
        match value["params"]["threadId"].as_str() {
            Some(thread)
                if value.get("id").is_none() && session_id.is_some_and(|own| own != thread) =>
            {
                Route::Only(thread.to_owned())
            }
            _ => Route::Own,
        }
    }

    fn child_events(
        &self,
        value: &Value,
        spec: &TurnSpec,
        _root: &str,
        message_id: &str,
    ) -> Vec<ChatEvent> {
        super::subagent_events(value, spec, message_id)
    }
}
