//! What a turn sends an agent CLI: the turn's spec, its prompt and images, its arguments, environment and stdin, and the lines that steer, interrupt or answer it.

// Rust guideline compliant 2026-11-23

use crate::Error;
use std::collections::HashMap;
use serde_json::Value;
use crate::chats::{AccessMode, AttachmentKind, Attachment, image_media_type};
use super::{AgentKind, AgentProvider, ProviderKind, api};
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

pub(crate) fn prompt_text(text: &str, attachments: &[Attachment]) -> String {
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

/// The stdin line that sends a message while the agent replies.
#[must_use]
pub fn steer_input(
    agent: AgentKind,
    prompt: Prompt<'_>,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    agent.provider().steer_input(prompt, session_id, turn_id)
}

/// The stdin line that starts the next turn on an idle warm agent process.
#[must_use]
pub fn resume_input(
    agent: AgentKind,
    prompt: Prompt<'_>,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    agent.provider().resume_input(prompt, session_id, turn_id)
}

/// The stdin line that stops the agent's reply without ending its process, for agents that can.
/// Codex needs the id of the turn it is running.
#[must_use]
pub fn interrupt(
    agent: AgentKind,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    agent.provider().interrupt(session_id, turn_id)
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
    agent.provider().turn_args(spec)
}

/// Environment for one turn's agent process, on top of the app's own.
#[must_use]
pub fn turn_env(agent: AgentKind, spec: &TurnSpec) -> Vec<(&'static str, String)> {
    agent.provider().turn_env(spec)
}

/// The environment variables kept from the agent's process: the ones the API providers read
/// their keys from, which are other providers' keys to this agent.
///
/// Patchwork's own agent is handed its key with the turn and needs none of them. A CLI keeps
/// the variables it signs in with, and so does the CLI that reads images for Patchwork's
/// agent. An agent that reads its own providers' keys from them keeps them all.
#[must_use]
pub fn withheld_env(agent: AgentKind, vision: Option<AgentKind>) -> Vec<String> {
    let provider = agent.provider();
    if provider.capabilities().keeps_provider_keys {
        return Vec::new();
    }
    let kept = |var: &str| {
        provider.own_key_vars().contains(&var)
            || vision.is_some_and(|vision| vision.provider().own_key_vars().contains(&var))
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
    agent.provider().turn_input(spec)
}

/// Whether the agent's stdin is closed once `turn_input` is written.
#[must_use]
pub fn closes_input(agent: AgentKind) -> bool {
    agent.capabilities().closes_input
}

/// Maps one line of the agent's JSONL output.
#[must_use]
pub fn parse_line(agent: AgentKind, v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let provider = agent.provider();
    provider
        .parse(v, spec, message_id)
        .into_iter()
        .map(|p| unasked(provider, spec, p))
        .collect()
}

/// An approval the access mode would not have asked the user for, which the agent sent only
/// because a permission rule may cover it, as a check for the rules.
fn unasked(provider: &dyn AgentProvider, spec: &TurnSpec, parsed: Parsed) -> Parsed {
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
    let full_access = spec.mode == AccessMode::FullAccess;
    match target {
        Some(target)
            if questions.is_empty() && (full_access || !provider.mode_asks(spec, &target)) =>
        {
            Parsed::Check {
                title,
                detail,
                reply,
                target,
            }
        }
        // Claude's full access still sends questions here, and calls no rule answers, which are
        // allowed at once.
        None if questions.is_empty()
            && full_access
            && let Some(line) = provider.full_access_reply(&reply) =>
        {
            Parsed::Send(line)
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

/// The stdin line answering an approval, from the `reply` the parser attached to it.
#[must_use]
pub fn approval_reply(
    agent: AgentKind,
    reply: &Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> String {
    agent.provider().approval_reply(reply, allow, answers)
}
