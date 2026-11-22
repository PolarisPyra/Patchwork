//! What goes to chat apps as a file because it is too long to read as messages: a long reply,
//! and the diff of what a reply changed.

// Rust guideline compliant 2026-10-09

use super::net::chunks;
use crate::provider::clip;
use super::platform::Platform;
use crate::{Core, Error};
use crate::orchestration::changes::turn_files::turn_file_diff;
use crate::chats::Role;

/// The most messages a reply is split into before it is sent as a file instead.
pub(super) const MESSAGES: usize = 2;

/// The most characters of a reply's opening shown beside its file.
const CAPTION_LIMIT: usize = 300;

/// The most characters of a diff shown as a message where the chat app takes no files.
const DIFF_TEXT_LIMIT: usize = 6000;

impl Core {
    /// Sends `text` to `chat`, as the file `reply.md` with its opening beside it when it would
    /// take more than a couple of messages.
    pub(super) fn say_long(&self, platform: Platform, conversation: &str, text: &str) {
        if chunks(text, platform.limit()).len() <= MESSAGES {
            self.say(platform, conversation, text);
            return;
        }
        let opening = text.trim().split("\n\n").next().unwrap_or_default();
        let caption = clip(opening, CAPTION_LIMIT);
        if !self.say_file(platform, conversation, "reply.md", text, &caption) {
            self.say(platform, conversation, text);
        }
    }

    /// Sends the diff of the files the latest reply of `chat_id` changed, as `changes.patch`,
    /// and returns what to tell the chat when there is none to send.
    pub(crate) fn send_diff(
        &self,
        platform: Platform,
        conversation: &str,
        chat_id: Option<&str>,
    ) -> Option<String> {
        let Some(chat) = chat_id.and_then(|id| self.store.chat(id)) else {
            return Some("No chat yet. Your next message starts one.".to_string());
        };
        let Some(root) = chat
            .project_id
            .as_deref()
            .and_then(|id| self.project_root(id).ok())
        else {
            return Some("This chat is in no project, so it changes no files.".to_string());
        };
        let Some((turn, files)) = chat
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
        let caption = format!("{}: {count} changed", chat.title);
        if !self.say_file(platform, conversation, "changes.patch", &diff, &caption) {
            self.say(
                platform,
                conversation,
                &format!("{caption}\n\n{}", clip(&diff, DIFF_TEXT_LIMIT)),
            );
        }
        None
    }

    /// Sends the file `name` to every conversation that talks to `chat_id`, with `caption`
    /// beside it, and returns how many took it.
    ///
    /// # Errors
    ///
    /// Returns an error when no conversation talks to the chat, or none of their chat apps
    /// took the file; the error is the last chat app's.
    pub(crate) fn send_file_to_chats(
        &self,
        chat_id: &str,
        name: &str,
        bytes: &[u8],
        caption: &str,
    ) -> Result<usize, Error> {
        let mut took = 0;
        let mut refused: Option<Error> = None;
        for bound in self.bound_conversations(chat_id) {
            let sent = self
                .gateway
                .connector(bound.platform)
                .ok_or_else(|| Error::from("That chat app is not connected"))
                .and_then(|c| c.send_file(&bound.conversation, name, bytes, caption));
            match sent {
                Ok(()) => took += 1,
                Err(error) => refused = Some(error),
            }
        }
        match (took, refused) {
            (0, Some(error)) => Err(error),
            (0, None) => Err("No chat app conversation talks to this chat".into()),
            (took, _) => Ok(took),
        }
    }

    /// Sends `text` to `chat` as the file `name` with `caption` beside it, and says whether the
    /// chat app took it.
    fn say_file(
        &self,
        platform: Platform,
        conversation: &str,
        name: &str,
        text: &str,
        caption: &str,
    ) -> bool {
        let Some(connector) = self.gateway.connector(platform) else {
            return false;
        };
        match connector.send_file(conversation, name, text.as_bytes(), caption) {
            Ok(()) => true,
            Err(error) => {
                log::debug!(platform = platform.id(), error:% = error; "could not send a file to {{platform}}: {{error}}");
                false
            }
        }
    }
}
