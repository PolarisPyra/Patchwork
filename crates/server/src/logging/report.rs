//! A chat's report, which its Open logs writes at the top of the chat's file in the launch's
//! folder, above its log: what Patchwork runs on, the chat's settings, each reply's outcome, this
//! launch's warnings and errors, and the chat's logs from the kept launches before this one.
//! One file holds what it takes to work out what went wrong in the chat, and like the logs it has
//! no secrets and names no user.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::provider::{self, AgentKind};
use crate::system::fs::home_dir;
use crate::threads::{Message, StepStatus, Thread};
use crate::{Res, Server};

use super::render::local_time;
use super::scrub::{cut, redact, scrubbed};
use super::system::{report, shown};
use super::thread_log::log_part;
use super::{session, Json, THREAD_LOG};

/// This launch's warning and error lines past this many are left out, oldest first.
const MAX_PROBLEMS: usize = 300;

/// A failed tool's output is cut to this.
const MAX_TOOL_OUTPUT: usize = 600;

impl Server {
    /// Writes the report of `thread_id` and returns its path.
    pub fn thread_report(&self, thread_id: &str) -> Res<PathBuf> {
        let thread = self
            .store
            .thread(thread_id)
            .ok_or(crate::threads::NOT_FOUND)?;
        let project = thread
            .project_id
            .as_deref()
            .map(|id| self.project_root(id).map(|path| shown(Path::new(&path))));
        let settings = serde_json::to_value(self.store.settings()).map_err(|e| e.to_string())?;
        let mut text = String::new();
        let _ = writeln!(
            text,
            "Patchwork chat report, {}",
            local_time(crate::system::now_ms())
        );
        section(&mut text, "System", &pretty(report().clone()));
        section(&mut text, "Chat", &chat(&thread, project));
        section(&mut text, "Replies", &replies(&thread.messages));
        section(&mut text, "This launch's warnings and errors", &problems());
        section(&mut text, "Settings", &pretty(settings));
        let earlier = earlier(thread_id);
        if !earlier.is_empty() {
            section(&mut text, "Earlier launches", &earlier);
        }
        // Settings, logs and errors name folders in full; the report names no user.
        if let Some(home) = home_dir().map(|home| home.display().to_string())
            && home.len() > 1
        {
            text = text.replace(&home, "~");
        }

        let path = THREAD_LOG.report(&thread.id, text)?;
        log::info!("wrote a chat's report: {}", path.display());
        Ok(path)
    }
}

fn section(text: &mut String, title: &str, body: &str) {
    let _ = write!(text, "\n== {title} ==\n{}\n", body.trim_end());
}

/// `value` as indented JSON, secrets removed and long strings cut.
fn pretty(value: serde_json::Value) -> String {
    serde_json::from_value::<Json>(value)
        .map(scrubbed)
        .and_then(|json| serde_json::to_string_pretty(&json))
        .unwrap_or_else(|e| format!("could not be written: {e}"))
}

/// The chat's settings and state, with the CLI its agent runs now. Patchwork runs its own agent,
/// which has no CLI to ask: its command is Patchwork itself, and asking it would start the app.
fn chat(thread: &Thread, project: Option<Res<String>>) -> String {
    let (cli, version) = if thread.agent == AgentKind::Api {
        ("built in".to_string(), Some(crate::VERSION.to_string()))
    } else {
        let cli = provider::agent_command(thread.agent)
            .map(|command| shown(Path::new(command.get_program())))
            .unwrap_or_else(|e| e);
        (cli, provider::cli_version(thread.agent))
    };
    let project = match project {
        Some(Ok(path)) => path,
        Some(Err(e)) => format!("unavailable: {e}"),
        None => "none (a chat)".into(),
    };
    pretty(serde_json::json!({
        "id": thread.id,
        "title": thread.title,
        "project": project,
        "agent": format!("{:?}", thread.agent),
        "cli": cli,
        "cliVersion": version,
        "model": thread.model,
        "accessMode": thread.access_mode,
        "effort": thread.effort,
        "compactWindow": thread.compact_window,
        "sessionId": thread.session_id,
        "createdAt": local_time(thread.created_at),
        "goal": thread.goal.as_ref().map(|goal| serde_json::json!({
            "status": goal.status,
            "checks": goal.checks,
            "tokensUsed": goal.tokens_used,
        })),
        "parent": thread.parent,
        "archived": thread.archived_at.is_some(),
        "planMode": thread.plan_mode,
        "role": thread.role,
        "importedFrom": thread.imported_from.as_deref().map(|path| shown(Path::new(path))),
        "contextPending": thread.context_pending,
        "messages": thread.messages.len(),
    }))
}

/// One line per message, with the failed tools and the errors under it. No one's words are in it.
fn replies(messages: &[Message]) -> String {
    let mut text = String::new();
    for message in messages {
        let short: String = message.id.chars().take(8).collect();
        let _ = write!(
            text,
            "{} {short} {:?} {:?}",
            local_time(message.created_at),
            message.role,
            message.status
        );
        if let Some(end) = message.finished_at {
            let _ = write!(
                text,
                " in {:.1}s",
                end.saturating_sub(message.created_at) as f64 / 1000.0
            );
        }
        let failed: Vec<_> = message
            .tools
            .iter()
            .filter(|t| t.status == StepStatus::Error)
            .collect();
        let _ = write!(
            text,
            ", {} chars, {} tools ({} failed), {} files, {} approvals",
            message.text.chars().count(),
            message.tools.len(),
            failed.len(),
            message.files.len(),
            message.approvals.len()
        );
        for (on, what) in [
            (message.steer, "steer"),
            (message.queued, "queued"),
            (message.cut_off, "cut off"),
            (!message.background.is_empty(), "background tasks"),
            (!message.attachments.is_empty(), "attachments"),
        ] {
            if on {
                let _ = write!(text, ", {what}");
            }
        }
        if let Some(usage) = &message.usage {
            let _ = write!(
                text,
                ", tokens in {} out {} context {}",
                usage.input + usage.cache_read + usage.cache_write,
                usage.output,
                usage.context
            );
        }
        if let Some(checks) = &message.checks {
            let _ = write!(text, ", checks {:?}", checks.status);
        }
        text.push('\n');
        for tool in failed {
            let output = tool.output.as_deref().unwrap_or("no output");
            let _ = writeln!(
                text,
                "    tool {} {} failed: {}",
                tool.name,
                tool.id,
                cut(output, MAX_TOOL_OUTPUT).replace('\n', "\\n")
            );
        }
        if let Some(refusal) = &message.refusal {
            let _ = writeln!(
                text,
                "    refused ({:?}): {}",
                refusal.category, refusal.explanation
            );
        }
        if let Some(error) = &message.error {
            let _ = writeln!(text, "    error: {}", error.replace('\n', "\\n"));
        }
    }
    redact(&text)
}

/// The chat's logs from the kept launches before this one, oldest first, each under its launch.
fn earlier(thread_id: &str) -> String {
    let mut text = String::new();
    for path in THREAD_LOG.earlier(thread_id) {
        let Ok(file) = std::fs::read_to_string(&path) else {
            continue;
        };
        let launch = path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = write!(text, "-- {launch} --\n{}", log_part(&file));
    }
    text
}

/// The warning and error lines of this launch's own log and the window's.
fn problems() -> String {
    let Some(dir) = session() else {
        return String::new();
    };
    let mut lines: Vec<String> = Vec::new();
    for name in ["main.log", "window.log"] {
        let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
            continue;
        };
        lines.extend(
            text.lines()
                .filter(|line| line.contains("[warning]") || line.contains("[error]"))
                .map(|line| format!("{name}: {}", redact(line))),
        );
    }
    lines.sort_by(|a, b| time_of(a).cmp(time_of(b)));
    let skip = lines.len().saturating_sub(MAX_PROBLEMS);
    if lines.is_empty() {
        return "none".into();
    }
    lines[skip..].join("\n")
}

/// The time a `<file>: <time> [level] …` line starts with, which sorts as text.
fn time_of(line: &str) -> &str {
    line.split_once(": ")
        .map_or(line, |(_, rest)| rest.get(..23).unwrap_or(rest))
}
