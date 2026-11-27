//! Antigravity behind the provider interface, and the reader every ACP agent shares.

// Rust guideline compliant 2026-10-09

use crate::provider::cli_info::{CliPage, CliReading, InfoCommand, untagged_models};
use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::chats::{BackgroundTask, ToolCall};
use crate::permissions::Target;
use crate::provider::agent::{
    AgentProvider, Capabilities, Identity, Logo, Steer, asks_outside_workspace,
};
use crate::provider::line_reader::LineReader;
use crate::provider::turn::prompt_text;
use crate::provider::{AgentKind, Parsed, Prompt, TurnSpec};

/// What every agent that speaks ACP can do; each adds its own on top.
pub(crate) const ACP: Capabilities = Capabilities {
    acp: true,
    steer: Steer::StopsReply,
    stays_up: true,
    streams_text: true,
    exits_on_stdin_close: true,
    ..Capabilities::BASE
};

/// The stdin line that starts the next reply of an idle ACP agent.
pub(crate) fn resume_input(
    kind: AgentKind,
    prompt: Prompt<'_>,
    session_id: Option<&str>,
) -> Option<String> {
    Some(super::prompt_input(
        session_id?,
        &crate::provider::acp::own_command(kind, prompt_text(prompt.text, prompt.attachments)),
        prompt.attachments,
        prompt.images,
    ))
}

/// Antigravity, run as `agy acp`.
#[derive(Debug)]
pub(crate) struct Antigravity;

impl AgentProvider for Antigravity {
    fn kind(&self) -> AgentKind {
        AgentKind::Antigravity
    }

    fn identity(&self) -> Identity {
        Identity {
            id: "antigravity",
            name: "Antigravity",
            title: "Antigravity CLI",
            logo: Logo::File {
                file: "gemini-color.svg",
                mono: false,
            },
            cli: Some("agy"),
        }
    }

    // Google's terms for Antigravity do not allow changing its instructions.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            lists_open_calls: true,
            subagent_chats: true,
            restates_calls: true,
            patchwork_compacts: true,
            takes_persona: false,
            runs_permission_hook: true,
            ..ACP
        }
    }

    fn locate(&self) -> Option<std::path::PathBuf> {
        super::antigravity_bin()
    }

    fn sign_in(&self, status: &mut crate::provider::AgentStatus) {
        super::auth(status);
    }

    fn models(&self) -> (Option<String>, Vec<crate::provider::ModelOption>) {
        (super::configured_model(), super::models())
    }

    fn cli_page(&self) -> Option<CliPage> {
        Some(CliPage {
            name: "agy",
            commands: &[
                ("Changelog", "changelog", InfoCommand::Changelog),
                ("MCP servers", "mcp list", InfoCommand::McpList),
                ("Plugins", "plugin list", InfoCommand::PluginList),
            ],
            rows: None,
            filter: None,
            empty: "Nothing matches.",
        })
    }

    // It shows no rows.
    fn read_cli(&self) -> Result<CliReading, crate::Error> {
        super::info().map(|i| CliReading {
            path: i.path,
            version: i.version,
            features: Vec::new(),
            models: untagged_models(i.models),
        })
    }

    fn run_cli_command(&self, command: InfoCommand) -> Result<String, crate::Error> {
        super::command(command)
    }

    fn turn_args(&self, spec: &TurnSpec) -> Vec<String> {
        super::turn_args(spec)
    }

    fn turn_env(&self, spec: &TurnSpec) -> Vec<(&'static str, String)> {
        super::turn_env(spec)
    }

    fn turn_input(&self, spec: &TurnSpec) -> Vec<String> {
        super::turn_input(spec)
    }

    fn parse(&self, value: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
        super::parse(value, spec, message_id)
    }

    fn reader(&self) -> Box<dyn LineReader> {
        Box::new(AntigravityReader::default())
    }

    // `agy` drops a reply it is given a prompt during and may ignore that prompt, so a steer
    // stops the reply instead, and the reader sends the steer once it has stopped.
    fn steer_input(
        &self,
        _prompt: Prompt<'_>,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        Some(super::interrupt(session_id?))
    }

    fn resume_input(
        &self,
        prompt: Prompt<'_>,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        resume_input(AgentKind::Antigravity, prompt, session_id)
    }

    fn interrupt(&self, session_id: Option<&str>, _turn_id: Option<&str>) -> Option<String> {
        Some(super::interrupt(session_id?))
    }

    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String {
        super::approval_reply(reply, allow, answers)
    }

    fn mode_asks(&self, spec: &TurnSpec, target: &Target) -> bool {
        asks_outside_workspace(spec, target)
    }

    fn own_key_vars(&self) -> &'static [&'static str] {
        &["GEMINI_API_KEY", "GOOGLE_API_KEY"]
    }

    // A subagent's own commands stay with its chat.
    fn background_task(&self, tool: &ToolCall) -> Option<BackgroundTask> {
        tool.parent
            .is_none()
            .then(|| super::background_task(tool))
            .flatten()
    }

    fn child_transcripts(&self, output: &str) -> Vec<super::Transcript> {
        super::child_transcripts(output)
    }
}

/// An ACP agent's running calls, so an update that leaves fields out keeps the earlier ones.
#[derive(Debug, Default)]
pub(crate) struct AcpReader {
    calls: super::ToolCalls,
}

impl LineReader for AcpReader {
    fn prepare(&mut self, value: &mut Value) {
        self.calls.merge(value);
    }
}

/// Antigravity's calls, and its swarm workers, read from the files agy keeps while a swarm runs.
#[derive(Debug, Default)]
struct AntigravityReader {
    acp: AcpReader,
    swarm: super::Swarm,
}

impl LineReader for AntigravityReader {
    fn prepare(&mut self, value: &mut Value) {
        self.acp.prepare(value);
    }

    fn starts_own_work(&self, value: &Value) -> bool {
        super::is_work(value)
    }

    fn poll_every(&self) -> Option<Duration> {
        self.swarm.poll_every()
    }

    fn tick(&mut self, session_id: &str, message_id: &str) -> Vec<Parsed> {
        self.swarm.poll(session_id, message_id)
    }

    fn after_parse(
        &mut self,
        value: &Value,
        session_id: &str,
        message_id: &str,
        parsed: &mut Vec<Parsed>,
    ) {
        if value["method"] == "session/update"
            && let Some(tool) = value["params"]["update"]["name"].as_str()
        {
            self.swarm.note_tool(tool);
            // The workers a swarm call added are shown under it straight away.
            if matches!(
                value["params"]["update"]["status"].as_str(),
                Some("completed" | "failed")
            ) {
                parsed.extend(self.swarm.poll(session_id, message_id));
            }
        }
        // Before the reply's end decides whether to close the agent's input, so workers at work
        // hold it open.
        if parsed.iter().any(|p| matches!(p, Parsed::Finished)) {
            let mut polled = self.swarm.poll(session_id, message_id);
            polled.append(parsed);
            *parsed = polled;
        }
    }

    fn is_holding(&self) -> bool {
        self.swarm.is_holding()
    }

    fn transcript(&self, tool_id: &str) -> Option<super::Transcript> {
        self.swarm.transcript(tool_id)
    }

    fn finish_open(&mut self, message_id: &str, failed: bool) -> Vec<Parsed> {
        self.swarm.finish_open(message_id, failed)
    }

    fn reports_context(&self, request_id: &str) -> bool {
        request_id.starts_with(super::CONTEXT_REQUEST)
    }
}
