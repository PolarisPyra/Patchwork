//! What goes to chat apps as a file because it is too long to read as messages: a long reply,
//! and the diff of what a reply changed.

// Rust guideline compliant 2026-10-09

use super::net::chunks;
use super::outbound::clip;
use super::platform::Platform;
use crate::Core;
use crate::orchestration::changes::turn_files::turn_file_diff;
use crate::threads::Role;

/// The most messages a reply is split into before it is sent as a file instead.
const MESSAGES: usize = 2;

/// The most characters of a reply's opening shown beside its file.
const CAPTION_LIMIT: usize = 300;

/// The most characters of a diff shown as a message where the chat app takes no files.
const DIFF_TEXT_LIMIT: usize = 6000;

impl Core {
    /// Sends `text` to `chat`, as the file `reply.md` with its opening beside it when it would
    /// take more than a couple of messages.
    pub(super) fn say_long(&self, platform: Platform, chat: &str, text: &str) {
        if chunks(text, platform.limit()).len() <= MESSAGES {
            self.say(platform, chat, text);
            return;
        }
        let opening = text.trim().split("\n\n").next().unwrap_or_default();
        let caption = clip(opening, CAPTION_LIMIT);
        if !self.say_file(platform, chat, "reply.md", text, &caption) {
            self.say(platform, chat, text);
        }
    }

    /// Sends the diff of the files the latest reply of `thread_id` changed, as `changes.patch`,
    /// and returns what to tell the chat when there is none to send.
    pub(super) fn send_diff(
        &self,
        platform: Platform,
        chat: &str,
        thread_id: Option<&str>,
    ) -> Option<String> {
        let Some(thread) = thread_id.and_then(|id| self.store.thread(id)) else {
            return Some("No chat yet. Your next message starts one.".to_string());
        };
        let Some(root) = thread
            .project_id
            .as_deref()
            .and_then(|id| self.project_root(id).ok())
        else {
            return Some("This chat is in no project, so it changes no files.".to_string());
        };
        let Some((turn, files)) = thread
            .messages
            .iter()
            .rev()
            .filter(|m| m.role == Role::Assistant && !m.files.is_empty())
            .find_map(|m| Some((m.turn.as_ref()?, &m.files)))
        else {
            return Some("No reply in this chat changed files.".to_string());
        };
        let mut diff = String::new();
        for path in files {
            match turn_file_diff(&root, turn, path) {
                Ok(file) => diff.push_str(&file),
                Err(error) => {
                    log::warn!(error:% = error; "could not diff a file for a chat app: {{error}}");
                }
            }
        }
        if diff.trim().is_empty() {
            return Some("Patchwork could not read the changes.".to_string());
        }
        let count = if files.len() == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", files.len())
        };
        let caption = format!("{}: {count} changed", thread.title);
        if !self.say_file(platform, chat, "changes.patch", &diff, &caption) {
            self.say(
                platform,
                chat,
                &format!("{caption}\n\n{}", clip(&diff, DIFF_TEXT_LIMIT)),
            );
        }
        None
    }

    /// Sends `text` to `chat` as the file `name` with `caption` beside it, and says whether the
    /// chat app took it.
    fn say_file(
        &self,
        platform: Platform,
        chat: &str,
        name: &str,
        text: &str,
        caption: &str,
    ) -> bool {
        let Some(connector) = self.gateway.connector(platform) else {
            return false;
        };
        match connector.send_file(chat, name, text.as_bytes(), caption) {
            Ok(()) => true,
            Err(error) => {
                log::debug!(platform = platform.id(), error:% = error; "could not send a file to {{platform}}: {{error}}");
                false
            }
        }
    }
}
