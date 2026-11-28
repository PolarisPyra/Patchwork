//! Agent identity metadata and static capability flags.

// Rust guideline compliant 2026-10-09

use super::AgentKind;

/// The agents in the order pickers and the task list show them.
pub const DISPLAY_ORDER: [AgentKind; 8] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
    AgentKind::Qwen,
    AgentKind::Kimi,
    AgentKind::Auggie,
    AgentKind::Api,
];

/// How an agent shows itself: its names and its mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    /// The name settings, chats and logs store, as `AgentKind` serializes it.
    pub id: &'static str,
    /// The short name, as messages and the web client give it.
    pub name: &'static str,
    /// The name pickers and settings show.
    pub title: &'static str,
    pub logo: Logo,
    /// The CLI's program name, for an agent with a CLI of its own to install and update.
    pub cli: Option<&'static str>,
}

/// An agent's mark, as plain data the UI draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Logo {
    /// A file under `logos/`; a mono one takes the text color.
    File { file: &'static str, mono: bool },
    /// One of the app's icons, for an agent with no mark Patchwork ships.
    Icon(&'static str),
    /// The mark of the API provider the chat's model belongs to.
    ApiProvider,
}

/// What happens to a message sent while the agent is replying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Steer {
    /// The agent ends its reply to take the message, then answers it in a new one.
    EndsReply,
    /// Patchwork stops the reply, then sends the message as the next prompt.
    StopsReply,
    /// The agent takes the message into the reply under way, or not at all.
    Inline,
}

/// How a message that names a skill reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillCall {
    /// As a command of the plugin Patchwork loads the skills from.
    PluginCommand,
    /// As a `$name` mention the CLI resolves itself.
    Mention,
    /// As a line telling it which `SKILL.md` to read.
    ReadFile,
}

/// Which shell runs the agent's commands, as its CLI picks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellChoice {
    /// Bash, else zsh: the only shells it runs.
    BashOrZsh,
    /// The account's login shell, when it is a POSIX one.
    LoginPosix,
    /// The shell Patchwork runs commands in, which it reads from SHELL.
    Command,
}

/// What an agent can do, as plain facts the rest of the app reads.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about the agent"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Runs as an Agent Client Protocol server on stdio.
    pub acp: bool,
    pub steer: Steer,
    /// The process stays up between replies, so the next message skips startup.
    pub stays_up: bool,
    /// Text arrives in chunks of one block, which run together.
    pub streams_text: bool,
    /// A stopped reply ends once stdin closes, rather than by a signal.
    pub exits_on_stdin_close: bool,
    /// Calls the finished reply left open are listed under the background.
    pub lists_open_calls: bool,
    /// A subagent's call gets a chat of its own.
    pub subagent_chats: bool,
    /// A call's later updates can name or describe it better than its start.
    pub restates_calls: bool,
    /// Patchwork stops a reply that fills the window to compact it.
    pub patchwork_compacts: bool,
    /// The session can be resumed from an earlier message, for undo and forks.
    pub resume_points: bool,
    /// May be given a system prompt or a personality.
    pub takes_persona: bool,
    /// Stdin closes once the turn's input is written.
    pub closes_input: bool,
    /// Keeps every API provider's key variable, as it reads its own providers' keys from them.
    pub keeps_provider_keys: bool,
    /// Can run a chat app's dispatcher.
    pub dispatches: bool,
    /// Patchwork runs the commands of its own tools for it, so they can outlive it.
    pub patchwork_runs_commands: bool,
    /// Has no hooks of its own, so Patchwork runs the user's hooks before its calls.
    pub patchwork_runs_hooks: bool,
    /// Can start the user's custom agents as subagents.
    pub custom_agents: bool,
    pub shell: ShellChoice,
    /// Its turns run Patchwork's own binary, as the agent or as the bridge to the CLI's server.
    pub runs_in_patchwork: bool,
    /// The folder under Patchwork's launch cache it loads skills from, for a CLI that has one.
    pub skills_folder: Option<&'static str>,
    /// The tool it keeps a checklist with, which it is told to use.
    pub checklist_tool: Option<&'static str>,
    pub skill_call: SkillCall,
    /// Restores an unmet goal of its own when a session resumes.
    pub restores_own_goal: bool,
    /// Asks about calls through Patchwork's permission relay.
    pub relays_permissions: bool,
    /// Runs Patchwork's permission hook before its calls.
    pub runs_permission_hook: bool,
    /// Is told the tools it blocks at launch, and cannot be told about servers.
    pub blocks_at_launch: bool,
}

impl Capabilities {
    /// What an agent with none of the optional abilities can do.
    pub const BASE: Self = Self {
        acp: false,
        steer: Steer::Inline,
        stays_up: false,
        streams_text: false,
        exits_on_stdin_close: false,
        lists_open_calls: false,
        subagent_chats: false,
        restates_calls: false,
        patchwork_compacts: false,
        resume_points: false,
        takes_persona: true,
        closes_input: false,
        keeps_provider_keys: false,
        dispatches: false,
        patchwork_runs_commands: false,
        patchwork_runs_hooks: false,
        custom_agents: false,
        shell: ShellChoice::Command,
        runs_in_patchwork: false,
        skills_folder: None,
        checklist_tool: None,
        skill_call: SkillCall::ReadFile,
        restores_own_goal: false,
        relays_permissions: false,
        runs_permission_hook: false,
        blocks_at_launch: false,
    };
}

impl AgentKind {
    /// How this agent shows itself: its stored id, short name, title, logo, and CLI name.
    #[must_use]
    pub const fn identity(self) -> Identity {
        match self {
            Self::Claude => Identity {
                id: "claude",
                name: "Claude Code",
                title: "Claude Code",
                logo: Logo::File {
                    file: "claude-color.svg",
                    mono: false,
                },
                cli: Some("claude"),
            },
            Self::Codex => Identity {
                id: "codex",
                name: "Codex",
                title: "Codex",
                logo: Logo::File {
                    file: "openai.svg",
                    mono: true,
                },
                cli: Some("codex"),
            },
            Self::Antigravity => Identity {
                id: "antigravity",
                name: "Antigravity",
                title: "Antigravity CLI",
                logo: Logo::File {
                    file: "gemini-color.svg",
                    mono: false,
                },
                cli: Some("agy"),
            },
            Self::OpenCode => Identity {
                id: "opencode",
                name: "OpenCode",
                title: "OpenCode CLI",
                logo: Logo::File {
                    file: "opencode.svg",
                    mono: true,
                },
                cli: Some("opencode"),
            },
            Self::Qwen => Identity {
                id: "qwen",
                name: "Qwen Code",
                title: "Qwen Code",
                logo: Logo::File {
                    file: "qwen-color.svg",
                    mono: false,
                },
                cli: Some("qwen"),
            },
            Self::Kimi => Identity {
                id: "kimi",
                name: "Kimi Code",
                title: "Kimi Code",
                logo: Logo::File {
                    file: "kimi.svg",
                    mono: true,
                },
                cli: Some("kimi"),
            },
            Self::Auggie => Identity {
                id: "auggie",
                name: "Auggie",
                title: "Auggie",
                logo: Logo::Icon("robot"),
                cli: Some("auggie"),
            },
            Self::Api => Identity {
                id: "api",
                name: "API",
                title: "API",
                logo: Logo::ApiProvider,
                cli: None,
            },
        }
    }
}
