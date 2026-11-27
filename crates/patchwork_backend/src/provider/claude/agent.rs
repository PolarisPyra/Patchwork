//! Claude Code behind the provider interface.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::ffi::OsString;

use serde_json::Value;

use crate::events::ChatEvent;
use crate::permissions::Target;
use crate::extensions::skills::Skill;
use crate::provider::cli_info::{
    CliFeature, CliModel, CliPage, CliReading, CliRows, InfoCommand, SHARED_COMMANDS,
};
use crate::provider::agent::{
    AgentProvider, Capabilities, Identity, Logo, ShellChoice, SkillCall, Steer, TurnLaunch,
    asks_outside_workspace,
};
use crate::settings::Settings;
use crate::provider::line_reader::{LineReader, Route};
use crate::provider::mcp::overrides::{dispatcher_overrides, overrides};
use crate::provider::turn::{prompt, prompt_text};
use crate::provider::{AgentKind, Parsed, Prompt, TurnSpec};

/// Claude Code, run as `claude -p` with stream-json input and output.
#[derive(Debug)]
pub(crate) struct Claude;

impl AgentProvider for Claude {
    fn kind(&self) -> AgentKind {
        AgentKind::Claude
    }

    fn identity(&self) -> Identity {
        Identity {
            id: "claude",
            name: "Claude Code",
            title: "Claude Code",
            logo: Logo::File {
                file: "claude-color.svg",
                mono: false,
            },
            cli: Some("claude"),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            steer: Steer::EndsReply,
            stays_up: true,
            streams_text: true,
            subagent_chats: true,
            resume_points: true,
            dispatches: true,
            custom_agents: true,
            shell: ShellChoice::BashOrZsh,
            checklist_tool: Some(
                "TaskCreate and TaskUpdate tools (TodoWrite if you have no TaskCreate)",
            ),
            skill_call: SkillCall::PluginCommand,
            restores_own_goal: true,
            relays_permissions: true,
            runs_permission_hook: true,
            ..Capabilities::BASE
        }
    }

    fn launch_args(&self, launch: &TurnLaunch<'_>) -> Vec<String> {
        let mut args = if launch.dispatcher {
            super::launch::dispatcher_args(crate::chats::tools::DISPATCHER)
        } else {
            [
                super::launch::launch_args(
                    launch.settings,
                    launch.hooks,
                    crate::orchestration::agent_instructions(launch.settings, AgentKind::Claude),
                ),
                super::launch::permission_args(),
                super::launch::agents_args(launch.agents, launch.root),
                launch.core.agent_claude_args(launch.chat_id),
            ]
            .concat()
        };
        // After an undo or a fork the session goes on from the reply they left last.
        if let Some(point) = launch.point {
            args.extend(super::launch::resume_point_args(point));
        }
        args
    }

    fn launch_env(&self, spec: &TurnSpec, shell: Option<&str>) -> Vec<(&'static str, OsString)> {
        let mut env = Vec::new();
        // Claude Code on Windows runs its commands in Git Bash, and refuses to start when it
        // cannot find it on PATH.
        if let Some(shell) = shell
            && cfg!(windows)
            && shell.ends_with("bash.exe")
        {
            env.push(("CLAUDE_CODE_GIT_BASH_PATH", shell.into()));
        }
        env.extend(
            super::launch::launch_env(spec.compact_window)
                .into_iter()
                .map(|(key, value)| (key, value.into())),
        );
        if let Some(file) = super::launch::shell_env_file() {
            env.push(("CLAUDE_ENV_FILE", file.into_os_string()));
        }
        env
    }

    // Claude takes its instructions as launch arguments instead.
    fn instructions(&self, _settings: &Settings, _skills: &[Skill]) -> Option<String> {
        None
    }

    fn clear_own_goal(&self, session_id: &str, root: &str, compact_window: u64) {
        super::launch::clear_own_goal(session_id, root, compact_window);
    }

    fn command_env(&self, _remote: bool) -> Vec<(&'static str, String)> {
        vec![("CLAUDE_CODE_ENTRYPOINT", super::entrypoint())]
    }

    fn sign_in(&self, status: &mut crate::provider::AgentStatus) {
        super::auth(status);
    }

    fn models(&self) -> (Option<String>, Vec<crate::provider::ModelOption>) {
        (super::configured_model(), super::models())
    }

    fn cli_page(&self) -> Option<CliPage> {
        Some(CliPage {
            name: "claude",
            commands: SHARED_COMMANDS,
            rows: Some(CliRows::Flags),
            filter: Some("On only"),
            empty: "Claude Code has not cached its feature gates yet. Start it once, then read again.",
        })
    }

    // Its gates and catalog come from its caches.
    fn read_cli(&self) -> Result<CliReading, crate::Error> {
        super::cli::info().map(|i| CliReading {
            path: i.path,
            version: i.version,
            features: i
                .features
                .into_iter()
                .map(|f| CliFeature {
                    kept: f.enabled == Some(true),
                    name: f.name,
                    detail: f.value,
                    enabled: f.enabled,
                    source: None,
                })
                .collect(),
            models: i
                .models
                .into_iter()
                .map(|m| CliModel {
                    id: m.id,
                    name: m.name,
                    context_window: m.context_window,
                    efforts: m.efforts,
                    tag: m.fast_mode.then(|| "fast".to_string()),
                })
                .collect(),
        })
    }

    fn run_cli_command(&self, command: InfoCommand) -> Result<String, crate::Error> {
        super::cli::command(command)
    }

    fn turn_args(&self, spec: &TurnSpec) -> Vec<String> {
        let servers = if spec.dispatcher {
            dispatcher_overrides(spec.chat_id)
        } else {
            overrides(AgentKind::Claude, spec.root, spec.chat_id)
        };
        [super::turn_args(spec), servers].concat()
    }

    fn turn_input(&self, spec: &TurnSpec) -> Vec<String> {
        vec![super::user_message(&prompt(spec), spec.images)]
    }

    fn parse(&self, value: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
        super::parse(value, spec.root, message_id)
    }

    fn reader(&self) -> Box<dyn LineReader> {
        Box::new(ClaudeReader::default())
    }

    fn steer_input(
        &self,
        prompt: Prompt<'_>,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        Some(super::steer_message(
            &prompt_text(prompt.text, prompt.attachments),
            prompt.images,
        ))
    }

    fn interrupt(&self, _session_id: Option<&str>, _turn_id: Option<&str>) -> Option<String> {
        Some(super::interrupt())
    }

    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String {
        super::approval_reply(reply, allow, answers)
    }

    // Claude's full access still sends questions, and calls no rule answers, which are allowed
    // at once.
    fn full_access_reply(&self, reply: &Value) -> Option<String> {
        Some(super::approval_reply(reply, true, &HashMap::new()))
    }

    fn mode_asks(&self, spec: &TurnSpec, target: &Target) -> bool {
        asks_outside_workspace(spec, target)
    }

    fn own_key_vars(&self) -> &'static [&'static str] {
        &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]
    }
}

/// Claude's task list, and its subagents' frames, which name the call that started them.
#[derive(Debug, Default)]
struct ClaudeReader {
    tasks: super::TaskList,
}

impl LineReader for ClaudeReader {
    // A message Claude took after it ended its reply starts a new one with `system/init`.
    fn starts_reply(&self, value: &Value) -> bool {
        value["type"] == "system" && value["subtype"] == "init"
    }

    fn route(&self, value: &Value, _session_id: Option<&str>) -> Route {
        value["parent_tool_use_id"]
            .as_str()
            .map_or(Route::Own, |id| Route::Also(id.to_owned()))
    }

    fn child_events(
        &self,
        value: &Value,
        _spec: &TurnSpec,
        root: &str,
        message_id: &str,
    ) -> Vec<ChatEvent> {
        super::subagent_events(value, root, message_id)
    }

    // The task list shows as the plan of whichever reply changes it.
    fn after_parse(
        &mut self,
        value: &Value,
        _session_id: &str,
        message_id: &str,
        parsed: &mut Vec<Parsed>,
    ) {
        if let Some(steps) = self.tasks.read(value) {
            parsed.push(Parsed::Event(ChatEvent::PlanUpdated {
                message_id: message_id.to_owned(),
                steps,
            }));
        }
    }
}
