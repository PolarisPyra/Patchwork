//! What a turn sends an agent CLI: the turn's spec, its prompt and images, its arguments, environment and stdin, and the lines that steer, interrupt or answer it.

// Rust guideline compliant 2026-11-23

use crate::Error;
use std::collections::HashMap;
use std::path::Path;
use serde_json::Value;
use crate::permissions::PermissionKind;
use crate::chats::{AccessMode, AttachmentKind, Attachment, image_media_type};
use super::{antigravity, api, claude, codex, opencode, AgentKind};
use super::Parsed;

/// What one turn runs with.
#[derive(Debug)]
pub struct TurnSpec<'a> {
    /// The agent the turn runs.
    pub agent: AgentKind,
    /// The project's folder, on the SSH host when [`Self::host`] is set.
    pub root: &'a str,
    /// The SSH host the project is on, for a remote project.
    pub host: Option<&'a str>,
    /// The chat the turn runs in, which Patchwork's own tools act for.
    pub chat_id: &'a str,
    pub session_id: Option<&'a str>,
    pub model: Option<&'a str>,
    /// The model that reads images, when `model` cannot. Only Patchwork's own agent uses it.
    pub vision_model: Option<&'a crate::settings::VisionModel>,
    pub text: &'a str,
    pub mode: AccessMode,
    /// Reasoning effort, e.g. "high"; None leaves the CLI's default.
    pub effort: Option<&'a str>,
    /// The context size at which the agent compacts mid-task; 0 leaves it to the agent.
    pub compact_window: u64,
    /// Patchwork's own agent runs without a limit on its steps of tool calls.
    pub unlimited_steps: bool,
    /// Tokens Patchwork's own agent may spend on the turn; 0 sets no limit.
    pub token_limit: u64,
    pub attachments: &'a [Attachment],
    /// The image attachments, read from disk by `load_images`.
    pub images: &'a [Image],
    /// Instructions added to the agent's own: Patchwork's in Codex's and the API agent's turn input,
    /// and only the user's system prompt for OpenCode and Antigravity.
    pub instructions: Option<&'a str>,
    /// Patchwork's skills folder, which the agent loads skills from.
    pub skills_dir: Option<&'a str>,
    /// The kinds the chat's permission rules guard, for the one agent told at launch what to
    /// ask about: `opencode run`. The rest ask Patchwork at each call (see `permissions::hook`).
    pub guarded: &'a [crate::permissions::Guarded],
    /// The custom agents the turn's CLI can start as its own subagents, by name.
    pub agents: &'a [crate::extensions::agents::Agent],
    /// The enabled hooks the agent runs on its own events.
    pub hooks: &'a [crate::extensions::hooks::AgentHook],
    /// The chat is a chat app's dispatcher: its agent gets Patchwork's chat tools and nothing
    /// of the computer, so no built-in tools, other servers, skills or hooks.
    pub dispatcher: bool,
}

/// An attached image, base64-encoded for the agent's input.
#[derive(Debug, Clone)]
pub struct Image {
    pub media_type: &'static str,
    pub data: String,
}

/// Largest image sent to an agent; the agents reject bigger ones.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Reads and encodes every image attachment.
///
/// # Errors
///
/// Returns an error when an image attachment is not a supported image type, cannot be read, or is
/// larger than 5 MB.
pub fn load_images(attachments: &[Attachment]) -> Result<Vec<Image>, Error> {
    use base64::Engine;
    attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image)
        .map(|a| {
            let media_type = image_media_type(&a.path).ok_or("Not an image")?;
            let size = std::fs::metadata(&a.path)
                .map_err(|e| format!("{}: {e}", a.name))?
                .len();
            if size > MAX_IMAGE_BYTES {
                return Err(format!("{} is larger than 5 MB", a.name).into());
            }
            let bytes = std::fs::read(&a.path).map_err(|e| format!("{}: {e}", a.name))?;
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok::<_, Error>(Image { media_type, data })
        })
        .collect()
}

/// The prompt the agent reads: the message, then the paths of what was attached,
/// so the agent can open files and folders with its own tools.
#[must_use]
pub fn prompt(spec: &TurnSpec) -> String {
    prompt_text(spec.text, spec.attachments)
}

/// `text` without the attachment list that `prompt_text` appends.
pub(crate) fn without_attachments(text: &str) -> &str {
    text.split_once("\n\nAttached:\n")
        .map_or(text, |(text, _)| text)
        .trim_end()
}

pub(super) fn prompt_text(text: &str, attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return text.to_string();
    }
    let lines: Vec<String> = attachments
        .iter()
        .map(|a| {
            let kind = match a.kind {
                AttachmentKind::Image => "image",
                AttachmentKind::File => "file",
                AttachmentKind::Folder => "folder",
            };
            format!("- {} ({kind})", a.path)
        })
        .collect();
    format!("{text}\n\nAttached:\n{}", lines.join("\n"))
        .trim_start()
        .to_string()
}

/// A message's content as a provider receives it: its text, attachments, and decoded images.
#[derive(Debug, Clone, Copy)]
pub struct Prompt<'a> {
    pub text: &'a str,
    pub attachments: &'a [Attachment],
    pub images: &'a [Image],
}

#[must_use]
pub fn steer_input(
    agent: AgentKind,
    prompt: Prompt<'_>,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    let Prompt {
        text,
        attachments,
        images,
    } = prompt;
    match agent {
        AgentKind::Claude => Some(claude::steer_message(
            &prompt_text(text, attachments),
            images,
        )),
        AgentKind::Codex => Some(codex::steer_input(
            session_id?,
            turn_id?,
            &prompt_text(text, attachments),
            attachments,
        )),
        // `agy` drops a reply it is given a prompt during and may ignore that prompt, so a
        // steer stops the reply instead, and the reader sends the steer once it has stopped.
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            Some(antigravity::interrupt(session_id?))
        }
        // `opencode run` reads one message per run; later ones start the next turn.
        AgentKind::OpenCode => opencode::uses_serve()
            .then(|| opencode::steer_line(&prompt_text(text, attachments), attachments)),
        AgentKind::Api => Some(api::steer_line(&prompt_text(text, attachments), images)),
    }
}

/// The stdin line that starts the next turn on an idle warm agent process.
#[must_use]
pub fn resume_input(
    agent: AgentKind,
    prompt: Prompt<'_>,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    if agent.speaks_acp() {
        let Prompt {
            text,
            attachments,
            images,
        } = prompt;
        return Some(antigravity::prompt_input(
            session_id?,
            &super::acp::own_command(agent, prompt_text(text, attachments)),
            attachments,
            images,
        ));
    }
    steer_input(agent, prompt, session_id, turn_id)
}

/// The stdin line that stops the agent's reply without ending its process, for agents that can.
/// Codex needs the id of the turn it is running.
#[must_use]
pub fn interrupt(
    agent: AgentKind,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    match agent {
        AgentKind::Claude => Some(claude::interrupt()),
        AgentKind::Codex => Some(codex::interrupt(session_id?, turn_id?)),
        AgentKind::Api => Some(api::interrupt()),
        // `opencode run` reads nothing once it starts; only the bridge to `opencode serve` can.
        AgentKind::OpenCode => opencode::uses_serve().then(opencode::interrupt_line),
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            Some(antigravity::interrupt(session_id?))
        }
    }
}

/// `/goal`, alone or with arguments, which runs as a turn of its own.
#[must_use]
pub fn is_goal_command(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("/goal")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// `/compact`, alone or with instructions for the summary.
#[must_use]
pub fn is_compact_command(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("/compact")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// What a `/goal` message asks: `/goal` alone shows the goal; `clear` (or one of Claude's aliases
/// for it), `pause`, and `resume` act on it; anything else is a new objective.
#[derive(Debug)]
pub enum GoalCommand<'a> {
    Show,
    Clear,
    Pause,
    Resume,
    Set(&'a str),
}

/// The words after `/goal`, or None when `text` is no `/goal` message.
#[must_use]
pub fn goal_argument(text: &str) -> Option<&str> {
    is_goal_command(text).then(|| text.trim()["/goal".len()..].trim())
}

/// What the `/goal` message `text` asks, or None when it is none.
#[must_use]
pub fn goal_command(text: &str) -> Option<GoalCommand<'_>> {
    let arg = goal_argument(text)?;
    Some(match arg.to_lowercase().as_str() {
        "" => GoalCommand::Show,
        "clear" | "stop" | "off" | "reset" | "none" | "cancel" => GoalCommand::Clear,
        "pause" => GoalCommand::Pause,
        "resume" => GoalCommand::Resume,
        _ => GoalCommand::Set(arg),
    })
}

/// Command-line arguments for one turn.
#[must_use]
pub fn turn_args(agent: AgentKind, spec: &TurnSpec) -> Vec<String> {
    match agent {
        AgentKind::Claude if spec.dispatcher => [
            claude::turn_args(spec),
            crate::provider::mcp::overrides::dispatcher_overrides(spec.chat_id),
        ]
        .concat(),
        AgentKind::Claude => [
            claude::turn_args(spec),
            crate::provider::mcp::overrides::overrides(agent, spec.root, spec.chat_id),
        ]
        .concat(),
        AgentKind::Codex => [
            codex::turn_args(),
            crate::provider::mcp::overrides::overrides(agent, spec.root, spec.chat_id),
            crate::provider::hooks::codex_args(spec),
            codex::compact_args(spec),
        ]
        .concat(),
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            antigravity::turn_args(spec)
        }
        AgentKind::OpenCode => opencode::turn_args(spec),
        AgentKind::Api => api::turn_args(),
    }
}

/// Environment for one turn's agent process, on top of the app's own.
#[must_use]
pub fn turn_env(agent: AgentKind, spec: &TurnSpec) -> Vec<(&'static str, String)> {
    match agent {
        // The bridge sets up its own server, which inherits this environment. 2.x takes the
        // system prompt from the bridge, as an instruction entry on the session.
        AgentKind::OpenCode if opencode::uses_serve() => opencode::config_content([
            opencode::skills_config(spec),
            opencode::agents_config(spec),
            opencode::todo_config(),
            crate::provider::mcp::overrides::opencode_config(spec.chat_id),
        ])
        .map(|config| ("OPENCODE_CONFIG_CONTENT", config))
        .into_iter()
        .collect(),
        AgentKind::OpenCode => std::iter::once(("PWD", spec.root.to_string()))
            .chain(
                opencode::config_content([
                    opencode::permission_config(spec),
                    opencode::agents_config(spec),
                    opencode::instructions_config(spec),
                    crate::provider::mcp::overrides::opencode_config(spec.chat_id),
                ])
                .map(|config| ("OPENCODE_CONFIG_CONTENT", config)),
            )
            .collect(),
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            antigravity::turn_env(spec)
        }
        AgentKind::Claude | AgentKind::Codex | AgentKind::Api => Vec::new(),
    }
}

/// The variables an agent's own CLI reads its account's key from, which it keeps.
fn own_key_vars(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::Claude => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        AgentKind::Codex => &["OPENAI_API_KEY", "CODEX_API_KEY"],
        AgentKind::Antigravity => &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        AgentKind::Qwen => &["DASHSCOPE_API_KEY", "OPENAI_API_KEY"],
        AgentKind::OpenCode | AgentKind::Kimi | AgentKind::Auggie | AgentKind::Api => &[],
    }
}

/// The environment variables kept from the agent's process: the ones the API providers read
/// their keys from, which are other providers' keys to this agent.
///
/// Patchwork's own agent is handed its key with the turn and needs none of them. A CLI keeps
/// the variables it signs in with, and so does the CLI that reads images for Patchwork's
/// agent. `OpenCode` keeps them all, as it reads the keys of its own providers from them.
#[must_use]
pub fn withheld_env(agent: AgentKind, vision: Option<AgentKind>) -> Vec<String> {
    if agent == AgentKind::OpenCode {
        return Vec::new();
    }
    let kept = |var: &str| {
        own_key_vars(agent).contains(&var)
            || vision.is_some_and(|vision| own_key_vars(vision).contains(&var))
    };
    api::providers()
        .into_iter()
        .filter_map(|provider| provider.key_env)
        .map(|var| var.trim().to_string())
        .filter(|var| !var.is_empty() && !kept(var))
        .collect()
}

/// Lines written to the agent's stdin when the turn starts. Empty means the
/// agent takes no input, and stdin is closed.
#[must_use]
pub fn turn_input(agent: AgentKind, spec: &TurnSpec) -> Vec<String> {
    match agent {
        AgentKind::Claude => vec![claude::user_message(&prompt(spec), spec.images)],
        AgentKind::Codex => codex::turn_input(spec),
        // The OpenCode bridge reads its turn from stdin, and answers and steers after it.
        AgentKind::OpenCode if opencode::uses_serve() => vec![opencode::bridge_request(spec)],
        // Patchwork's own agent reads its turn from stdin, and answers and steers after it.
        AgentKind::Api => vec![api::bridge_request(spec)],
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            antigravity::turn_input(spec)
        }
        // `opencode run` reads the prompt from stdin, which is closed after it (see `closes_input`).
        AgentKind::OpenCode => opencode::turn_input(spec),
    }
}

/// Whether the agent's stdin is closed once `turn_input` is written: `opencode run` reads it to
/// its end before it starts.
#[must_use]
pub fn closes_input(agent: AgentKind) -> bool {
    agent == AgentKind::OpenCode && !opencode::uses_serve()
}

/// Maps one line of the agent's JSONL output.
#[must_use]
pub fn parse_line(agent: AgentKind, v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let parsed = match agent {
        AgentKind::Claude => claude::parse(v, spec.root, message_id),
        AgentKind::Codex => codex::parse(v, spec, message_id),
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            antigravity::parse(v, spec, message_id)
        }
        AgentKind::OpenCode => opencode::parse(v, spec, message_id),
        AgentKind::Api => api::parse(v, message_id),
    };
    parsed
        .into_iter()
        .map(|p| unasked(agent, spec, p))
        .collect()
}

/// An approval the access mode would not have asked the user for, which the agent sent only
/// because a permission rule may cover it, as a check for the rules.
fn unasked(agent: AgentKind, spec: &TurnSpec, parsed: Parsed) -> Parsed {
    let Parsed::Approval {
        title,
        detail,
        reply,
        questions,
        target,
    } = parsed
    else {
        return parsed;
    };
    match target {
        Some(target) if questions.is_empty() && !mode_asks(agent, spec, &target) => Parsed::Check {
            title,
            detail,
            reply,
            target,
        },
        // Claude's full access still sends questions here, and calls no rule answers, which are
        // allowed at once.
        None if questions.is_empty()
            && agent == AgentKind::Claude
            && spec.mode == AccessMode::FullAccess =>
        {
            Parsed::Send(claude::approval_reply(&reply, true, &HashMap::new()))
        }
        target => Parsed::Approval {
            title,
            detail,
            reply,
            questions,
            target,
        },
    }
}

/// Whether the access mode has the agent ask the user about `target`.
fn mode_asks(agent: AgentKind, spec: &TurnSpec, target: &crate::permissions::Target) -> bool {
    match (agent, spec.mode) {
        (_, AccessMode::FullAccess) => false,
        // Claude and Antigravity read inside the workspace without asking, and Accept edits takes
        // edits inside it.
        (AgentKind::Claude | AgentKind::Antigravity, mode)
            if target.kind == PermissionKind::Folder
                || (mode == AccessMode::AutoAcceptEdits && target.kind == PermissionKind::Edit) =>
        {
            let root = Path::new(spec.root);
            !target
                .resources
                .iter()
                .all(|r| crate::system::files::paths::is_inside(root, r))
        }
        (AgentKind::OpenCode, mode) => opencode::mode_asks(mode, target.kind),
        _ => true,
    }
}

/// The stdin line answering an approval, from the `reply` the parser attached to it.
#[must_use]
pub fn approval_reply(
    agent: AgentKind,
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    match agent {
        AgentKind::Claude => claude::approval_reply(reply, allow, answers),
        AgentKind::Codex => codex::approval_reply(reply, allow, answers),
        AgentKind::OpenCode if opencode::uses_serve() => {
            opencode::approval_reply(reply, allow, answers)
        }
        AgentKind::Api => api::approval_reply(reply, allow, answers),
        AgentKind::Antigravity | AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            antigravity::approval_reply(reply, allow, answers)
        }
        // `opencode run` runs headless and never asks.
        AgentKind::OpenCode => String::new(),
    }
}
