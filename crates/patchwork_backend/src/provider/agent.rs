//! One interface over every agent CLI: what it is called, what it can do, and how a turn talks
//! to it. Code outside the provider modules reads capabilities, never the agent kind.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::ffi::OsString;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::cli_info::{CliPage, CliReading, InfoCommand};
use super::line_reader::{LineReader, Plain};
use super::{AgentKind, AgentStatus, ModelOption, Parsed, Prompt, TurnSpec};
use crate::Core;
use crate::chats::{AccessMode, BackgroundTask, ResumePoint, ToolCall};
use crate::extensions::agents::Agent;
use crate::extensions::hooks::AgentHook;
use crate::extensions::skills::Skill;
use crate::permissions::{PermissionKind, Target};
use crate::settings::Settings;

#[doc(inline)]
pub use patchwork_provider_core::{
    Capabilities, DISPLAY_ORDER, Identity, Logo, ShellChoice, SkillCall, Steer,
};

/// What a turn's launch knows beyond its spec, for providers that take more at launch.
#[derive(Debug, Clone, Copy)]
pub struct TurnLaunch<'a> {
    pub(crate) core: &'a Core,
    pub(crate) chat_id: &'a str,
    pub(crate) settings: &'a Settings,
    pub(crate) hooks: &'a [AgentHook],
    pub(crate) agents: &'a [Agent],
    pub(crate) root: &'a str,
    pub(crate) dispatcher: bool,
    pub(crate) point: Option<&'a ResumePoint>,
}

/// One agent CLI, behind the interface every turn uses.
pub trait AgentProvider: Send + Sync + Debug {
    /// The agent's saved identity.
    fn kind(&self) -> AgentKind;

    /// How the agent shows itself.
    fn identity(&self) -> Identity;

    /// What the agent can do. Some answers depend on the installed CLI's version.
    fn capabilities(&self) -> Capabilities;

    /// Command-line arguments for one turn.
    fn turn_args(&self, spec: &TurnSpec) -> Vec<String>;

    /// Arguments the launch adds after the turn's own.
    fn launch_args(&self, _launch: &TurnLaunch<'_>) -> Vec<String> {
        Vec::new()
    }

    /// Environment the launch adds; `shell` is the shell that runs the agent's commands.
    fn launch_env(&self, _spec: &TurnSpec, _shell: Option<&str>) -> Vec<(&'static str, OsString)> {
        Vec::new()
    }

    /// The instructions the turn's spec carries, given the skills a skill-less agent is told of.
    fn instructions(&self, settings: &Settings, _skills: &[Skill]) -> Option<String> {
        crate::orchestration::system_prompt(settings, self.kind()).map(str::to_string)
    }

    /// Clears a goal of the agent's own that session `session_id` would restore.
    fn clear_own_goal(&self, _session_id: &str, _root: &str, _compact_window: u64) {}

    /// Where the installed CLI is: the path the user chose, else the one found by its name.
    fn locate(&self) -> Option<PathBuf> {
        let name = self.identity().cli?;
        super::locate::located_or(self.kind(), || super::locate::find_binary(name))
    }

    /// Environment every run of the CLI gets; `remote` when it runs on an SSH host.
    fn command_env(&self, _remote: bool) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Fills in whether the installed agent is signed in, and as whom.
    fn sign_in(&self, _status: &mut AgentStatus) {}

    /// The model the agent's own config picks, and the models it offers.
    fn models(&self) -> (Option<String>, Vec<ModelOption>);

    /// What the CLI's Details page offers, when Patchwork can read the CLI.
    fn cli_page(&self) -> Option<CliPage> {
        None
    }

    /// Reads the installed CLI's version, flags or settings, and models.
    ///
    /// # Errors
    ///
    /// Returns an error when the CLI cannot be read, or Patchwork reads none of it.
    fn read_cli(&self) -> Result<CliReading, crate::Error> {
        Err(if self.identity().cli.is_some() {
            "Patchwork reads no details for this agent"
        } else {
            "API providers have no CLI"
        }
        .into())
    }

    /// Runs one of the CLI's read-only diagnostics commands.
    ///
    /// # Errors
    ///
    /// Returns an error when the command fails, or Patchwork runs none for this agent.
    fn run_cli_command(&self, _command: InfoCommand) -> Result<String, crate::Error> {
        Err(if self.identity().cli.is_some() {
            "Patchwork runs no commands for this agent"
        } else {
            "API providers have no CLI"
        }
        .into())
    }

    /// Environment for one turn's process, on top of the app's own.
    fn turn_env(&self, _spec: &TurnSpec) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// Lines written to stdin when the turn starts.
    fn turn_input(&self, spec: &TurnSpec) -> Vec<String>;

    /// Maps one line of the agent's output.
    fn parse(&self, value: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed>;

    /// The per-turn state that reads the agent's lines around `parse`.
    fn reader(&self) -> Box<dyn LineReader> {
        Box::new(Plain)
    }

    /// The stdin line that sends a message while the agent replies.
    fn steer_input(
        &self,
        prompt: Prompt<'_>,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Option<String>;

    /// The stdin line that starts the next reply of an idle process.
    fn resume_input(
        &self,
        prompt: Prompt<'_>,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> Option<String> {
        self.steer_input(prompt, session_id, turn_id)
    }

    /// The stdin line that stops the reply without ending the process.
    fn interrupt(&self, session_id: Option<&str>, turn_id: Option<&str>) -> Option<String>;

    /// The stdin line answering an approval, from the `reply` the parser attached to it.
    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String;

    /// The answer to an approval full access sends at once, without asking anyone.
    fn full_access_reply(&self, _reply: &Value) -> Option<String> {
        None
    }

    /// Whether `mode` has the agent ask the user about `target`, short of full access.
    fn mode_asks(&self, _spec: &TurnSpec, _target: &Target) -> bool {
        true
    }

    /// The variables the CLI reads its account's key from, which it keeps.
    fn own_key_vars(&self) -> &'static [&'static str] {
        &[]
    }

    /// The background task a call stands for, when it runs past its own return.
    fn background_task(&self, _tool: &ToolCall) -> Option<BackgroundTask> {
        None
    }

    /// The transcripts of the subagents a finished call started, which their chats follow.
    fn child_transcripts(&self, _output: &str) -> Vec<super::antigravity::Transcript> {
        Vec::new()
    }
}

/// Whether an agent that reads its workspace freely asks about `target` in `spec`'s mode.
pub(crate) fn asks_outside_workspace(spec: &TurnSpec, target: &Target) -> bool {
    if target.kind != PermissionKind::Folder
        && !(spec.mode == AccessMode::AutoAcceptEdits && target.kind == PermissionKind::Edit)
    {
        return true;
    }
    let root = Path::new(spec.root);
    !target
        .resources
        .iter()
        .all(|r| crate::system::files::paths::is_inside(root, r))
}

static CLAUDE: super::claude::Claude = super::claude::Claude;
static CODEX: super::codex::Codex = super::codex::Codex;
static ANTIGRAVITY: super::antigravity::Antigravity = super::antigravity::Antigravity;
static OPENCODE: super::opencode::OpenCode = super::opencode::OpenCode;
static API: super::api::Api = super::api::Api;

/// Resolves an [`AgentKind`] to its runtime [`AgentProvider`] and live [`Capabilities`].
pub trait ProviderKind: Copy {
    /// The provider that runs this agent.
    #[must_use]
    fn provider(self) -> &'static dyn AgentProvider;

    /// What this agent can do.
    #[must_use]
    fn capabilities(self) -> Capabilities {
        self.provider().capabilities()
    }
}

impl ProviderKind for AgentKind {
    fn provider(self) -> &'static dyn AgentProvider {
        match self {
            AgentKind::Claude => &CLAUDE,
            AgentKind::Codex => &CODEX,
            AgentKind::Antigravity => &ANTIGRAVITY,
            AgentKind::OpenCode => &OPENCODE,
            AgentKind::Api => &API,
            _ => super::acp::provider(self).expect("every listed ACP agent has a spec"),
        }
    }
}

/// Every provider, in [`DISPLAY_ORDER`].
pub fn providers() -> impl Iterator<Item = &'static dyn AgentProvider> {
    DISPLAY_ORDER.into_iter().map(ProviderKind::provider)
}
