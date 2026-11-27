//! "Export to…" in the task tree's menu: asks where to save a chat,
//! then writes its transcript there as Markdown.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_shared::agent_label;
use std::sync::Arc;
use gpui::{prelude::*};
use patchwork_backend::chats::{Message, Part, Role, Chat};

use patchwork_ui_markdown::{display_title, slug_file_name};
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

/// The chat's transcript as Markdown, as it is exported and as Ctrl+A in the chat copies it.
pub fn chat_markdown(chat: &Chat) -> String {
    let agent = agent_label(chat.agent);
    let mut out = vec![format!("# {}", display_title(&chat.title))];
    for msg in &chat.messages {
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
    pub(super) fn export_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        let chat = match self.core.chat(id) {
            Ok(chat) => chat,
            Err(e) => {
                return log::warn!(error:% = e; "could not read the chat to export it: {{error}}");
            }
        };
        let home = patchwork_platform::files::paths::home_dir().unwrap_or_default();
        let name = slug_file_name(&display_title(&chat.title), "chat");
        let picked = cx.prompt_for_new_path(&home, Some(&name));
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let path = match picked.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(e)) => {
                    log::warn!(error:% = e; "could not open the save dialog: {{error}}");
                    let _ = this.update(cx, |this, cx| {
                        this.error = Some(e.to_string());
                        cx.notify();
                    });
                    return;
                }
            };
            let path = path.to_string_lossy().into_owned();
            let saved = cx
                .background_spawn(async move { core.save_text_file(&path, &chat_markdown(&chat)) })
                .await;
            if let Err(e) = saved {
                log::warn!(error:% = e; "could not export the chat: {{error}}");
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
