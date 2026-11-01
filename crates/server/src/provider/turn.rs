//! What a turn sends an agent CLI: the turn's spec, its prompt and images, its arguments, environment and stdin, and the lines that steer, interrupt or answer it.

use std::collections::HashMap;
use std::path::Path;
use serde_json::Value;
use crate::threads::{AccessMode, AttachmentKind, Attachment, image_media_type};
use super::{antigravity, api, claude, codex, opencode, AgentKind};
use super::Parsed;

/// What one turn runs with.
pub struct TurnSpec<'a> {
    pub root: &'a str,
    pub session_id: Option<&'a str>,
    pub model: Option<&'a str>,
    /// The model that reads images, when `model` cannot. Only Patchwork's own agent uses it.
    pub vision_model: Option<&'a str>,
    pub text: &'a str,
    pub mode: AccessMode,
    /// Plan mode: the agent plans and proposes a plan, without changing files.
    pub plan: bool,
    /// Reasoning effort, e.g. "high"; None leaves the CLI's default.
    pub effort: Option<&'a str>,
    /// The context size at which the agent compacts mid-task; 0 leaves it to the agent.
    pub compact_window: u64,
    pub attachments: &'a [Attachment],
    /// The image attachments, read from disk by `load_images`.
    pub images: &'a [Image],
    /// Instructions added to the agent's own: Patchwork's in Codex's and the API agent's turn input,
    /// and only the user's system prompt for OpenCode and Antigravity.
    pub instructions: Option<&'a str>,
    /// Patchwork's skills folder, which the agent loads skills from.
    pub skills_dir: Option<&'a str>,
    /// The kinds the thread's permission rules guard, for the one agent told at launch what to
    /// ask about: `opencode run`. The rest ask Patchwork at each call (see `permissions::hook`).
    pub guarded: &'a [crate::permissions::Guarded],
    /// The custom agents the turn's CLI can start as its own subagents, by name.
    pub agents: &'a [crate::agents::Agent],
    /// The enabled plugins' hooks the agent runs on its own events.
    pub hooks: &'a [crate::plugins::AgentHook],
}

/// An attached image, base64-encoded for the agent's input.
#[derive(Clone)]
pub struct Image {
    pub media_type: &'static str,
    pub data: String,
}

/// Largest image sent to an agent; the agents reject bigger ones.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Reads and encodes every image attachment.
pub fn load_images(attachments: &[Attachment]) -> Result<Vec<Image>, String> {
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
                return Err(format!("{} is larger than 5 MB", a.name));
            }
            let bytes = std::fs::read(&a.path).map_err(|e| format!("{}: {e}", a.name))?;
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(Image { media_type, data })
        })
        .collect()
}

/// The prompt the agent reads: the message, then the paths of what was attached,
/// so the agent can open files and folders with its own tools.
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

/// The stdin line that adds a message to a turn that is already running, or None when
/// the agent cannot take one now and the message waits for the turn to end. Antigravity reads
/// its whole prompt up front; Codex needs the turn's id, which it reports once started.
/// A message's content as a provider receives it: its text, attachments, and decoded images.
pub struct Prompt<'a> {
    pub text: &'a str,
    pub attachments: &'a [Attachment],
    pub images: &'a [Image],
}

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
        AgentKind::Antigravity => None,
        // `opencode run` reads one message per run; later ones start the next turn.
        AgentKind::OpenCode => opencode::uses_serve()
            .then(|| opencode::steer_line(&prompt_text(text, attachments), attachments)),
        AgentKind::Api => Some(api::steer_line(&prompt_text(text, attachments), images)),
    }
}

/// The stdin line that pauses the agent's goal, for agents that pursue it on their own.
pub fn goal_pause(agent: AgentKind, session_id: &str) -> Option<String> {
    (agent == AgentKind::Codex).then(|| codex::goal_pause(session_id))
}

/// The stdin line that stops the agent's reply without ending its process, for agents that can.
/// Codex needs the id of the turn it is running.
pub fn interrupt(
    agent: AgentKind,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> Option<String> {
    match agent {
        AgentKind::Claude => Some(claude::interrupt()),
        AgentKind::Codex => Some(codex::interrupt(session_id?, turn_id?)),
        AgentKind::Antigravity | AgentKind::OpenCode | AgentKind::Api => None,
    }
}

/// The stdin line that stops one subagent, for agents that can.
pub fn stop_subagent(agent: AgentKind, task_id: &str) -> Option<String> {
    (agent == AgentKind::Claude).then(|| claude::stop_task(task_id))
}

/// `/goal`, alone or with arguments, which runs as a turn of its own.
pub fn is_goal_command(text: &str) -> bool {
    text.trim_start()
        .strip_prefix("/goal")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// What a `/goal` message asks: `/goal` alone shows the goal; `clear` (or one of Claude's aliases
/// for it), `pause`, and `resume` act on it; anything else is a new objective.
pub enum GoalCommand<'a> {
    Show,
    Clear,
    Pause,
    Resume,
    Set(&'a str),
}

/// The words after `/goal`, or None when `text` is no `/goal` message.
pub fn goal_argument(text: &str) -> Option<&str> {
    is_goal_command(text).then(|| text.trim()["/goal".len()..].trim())
}

/// What the `/goal` message `text` asks, or None when it is none.
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
pub fn turn_args(agent: AgentKind, spec: &TurnSpec) -> Vec<String> {
    match agent {
        AgentKind::Claude => [
            claude::turn_args(spec),
            crate::provider::mcp::clis::overrides(agent, spec.root),
        ]
        .concat(),
        AgentKind::Codex => [
            codex::turn_args(),
            crate::provider::mcp::clis::overrides(agent, spec.root),
            crate::provider::hooks::codex_args(spec),
            codex::agents_args(spec),
            codex::compact_args(spec),
        ]
        .concat(),
        AgentKind::Antigravity => antigravity::turn_args(spec),
        AgentKind::OpenCode => opencode::turn_args(spec),
        AgentKind::Api => api::turn_args(),
    }
}

/// Environment for one turn's agent process, on top of the app's own.
pub fn turn_env(agent: AgentKind, spec: &TurnSpec) -> Vec<(&'static str, String)> {
    match agent {
        // The bridge sets up its own server, which inherits this environment. 2.x takes the
        // system prompt from the bridge, as an instruction entry on the session.
        AgentKind::OpenCode if opencode::uses_serve() => opencode::config_content([
            opencode::skills_config(spec),
            opencode::agents_config(spec),
            opencode::todo_config(),
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
                ])
                .map(|config| ("OPENCODE_CONFIG_CONTENT", config)),
            )
            .collect(),
        AgentKind::Antigravity => crate::provider::hooks::antigravity_env(spec),
        AgentKind::Claude | AgentKind::Codex | AgentKind::Api => Vec::new(),
    }
}

/// Lines written to the agent's stdin when the turn starts. Empty means the
/// agent takes no input, and stdin is closed.
pub fn turn_input(agent: AgentKind, spec: &TurnSpec) -> Vec<String> {
    match agent {
        // Claude only runs `/goal` from a text-only message, so a goal names its attachments by
        // path and Claude opens them with its own tools.
        AgentKind::Claude if is_goal_command(spec.text) => {
            vec![claude::user_message(&prompt(spec), &[])]
        }
        AgentKind::Claude => vec![claude::user_message(&prompt(spec), spec.images)],
        AgentKind::Codex => codex::turn_input(spec),
        // The OpenCode bridge reads its turn from stdin, and answers and steers after it.
        AgentKind::OpenCode if opencode::uses_serve() => vec![opencode::bridge_request(spec)],
        // Patchwork's own agent reads its turn from stdin, and answers and steers after it.
        AgentKind::Api => vec![api::bridge_request(spec)],
        // Both read the prompt from their arguments, and stdin is closed so they do not wait on it.
        AgentKind::Antigravity | AgentKind::OpenCode => vec![],
    }
}

/// Maps one line of the agent's JSONL output.
pub fn parse_line(agent: AgentKind, v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let parsed = match agent {
        AgentKind::Claude => claude::parse(v, spec.root, message_id),
        AgentKind::Codex => codex::parse(v, spec, message_id),
        AgentKind::Antigravity => antigravity::parse(v, spec, message_id),
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
        // Claude's full access still sends questions here, and calls no rule answers, such as
        // leaving plan mode, which are allowed at once.
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
        // Accept edits takes edits inside the folder without asking.
        (AgentKind::Claude, AccessMode::AutoAcceptEdits)
            if target.kind == crate::permissions::PermissionKind::Edit =>
        {
            let root = Path::new(spec.root);
            !target
                .resources
                .iter()
                .all(|r| crate::system::fs::resolve(root, r).starts_with(root))
        }
        // Claude reads in its own folder without asking.
        (AgentKind::Claude, _) if target.kind == crate::permissions::PermissionKind::Folder => {
            let root = Path::new(spec.root);
            !target
                .resources
                .iter()
                .all(|r| crate::system::fs::resolve(root, r).starts_with(root))
        }
        (AgentKind::OpenCode, mode) => opencode::mode_asks(mode, target.kind),
        _ => true,
    }
}

/// The stdin line answering an approval, from the `reply` the parser attached to it.
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
        // Antigravity and `opencode run` run headless and never ask.
        AgentKind::Antigravity | AgentKind::OpenCode => String::new(),
    }
}
