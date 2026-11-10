//! "Export to…" in the task tree's menu: asks where to save a chat,
//! then writes its transcript there as Markdown.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{prelude::*};
use patchwork_server::threads::{Message, Part, Role, Thread};

use crate::ui::{agent_label, display_title, slug_file_name};
use crate::workspace::Workspace;

/// A reply as Markdown: its text and plans, with each tool call as a list line.
fn reply_markdown(msg: &Message) -> String {
    if msg.parts.is_empty() {
        return msg.text.trim().to_string();
    }
    let mut blocks: Vec<String> = Vec::new();
    for part in &msg.parts {
        match part {
            Part::Text { text } | Part::Plan { text } if !text.trim().is_empty() => {
                blocks.push(text.trim().to_string());
            }
            Part::Tool { id } => {
                let Some(tool) = msg.tools.iter().find(|t| &t.id == id) else {
                    continue;
                };
                let summary = if tool.summary.is_empty() {
                    String::new()
                } else {
                    format!(" {}", tool.summary)
                };
                let line = format!("- `{}`{summary}", tool.name);
                // Consecutive tool calls form one list.
                match blocks.last_mut() {
                    Some(last) if last.starts_with("- `") => {
                        last.push('\n');
                        last.push_str(&line);
                    }
                    _ => blocks.push(line),
                }
            }
            _ => {}
        }
    }
    blocks.join("\n\n")
}

/// The thread's transcript as Markdown.
fn thread_markdown(thread: &Thread) -> String {
    let agent = agent_label(thread.agent);
    let mut out = vec![format!("# {}", display_title(&thread.title))];
    for msg in &thread.messages {
        let user = msg.role == Role::User;
        let body = if user {
            msg.text.trim().to_string()
        } else {
            reply_markdown(msg)
        };
        let error = msg
            .error
            .as_ref()
            .map(|e| format!("> {}", e.trim().replace('\n', "\n> ")))
            .unwrap_or_default();
        if body.is_empty() && error.is_empty() {
            continue;
        }
        out.push(format!("## {}", if user { "You" } else { agent }));
        out.extend([body, error].into_iter().filter(|b| !b.is_empty()));
    }
    format!("{}\n", out.join("\n\n"))
}

impl Workspace {
    pub(super) fn export_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        let thread = match self.server.thread(id) {
            Ok(thread) => thread,
            Err(e) => {
                return log::warn!(error:% = e; "could not read the chat to export it: {{error}}");
            }
        };
        let home = patchwork_server::system::fs::home_dir().unwrap_or_default();
        let name = slug_file_name(&display_title(&thread.title), "chat");
        let picked = cx.prompt_for_new_path(&home, Some(&name));
        let server = Arc::clone(&self.server);
        cx.background_spawn(async move {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let path = path.to_string_lossy().into_owned();
            if let Err(e) = server.save_text_file(&path, &thread_markdown(&thread)) {
                log::warn!(error:% = e; "could not export the chat: {{error}}");
            }
        })
        .detach();
    }
}
