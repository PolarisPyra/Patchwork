//! Bringing back what was sent: Up on the first line shows this chat's earlier messages, newest
//! first, and Down on the last line steps back toward the draft that was being written.

// Rust guideline compliant 2026-11-10

use gpui::{Window, prelude::*};
use patchwork_server::threads::{Attachment, Role};

use super::state::Composer;

/// Where Up has stepped to among the sent messages.
pub(super) struct Recall {
    /// How far back, from 0 for the newest.
    index: usize,
    /// What was written and attached before the first Up, which Down past the newest brings back.
    draft: (String, Vec<Attachment>),
    /// The message shown, so an edit to it ends the stepping.
    shown: String,
}

impl Composer {
    /// The messages written in this chat with what they carried, newest first, each once in a row.
    fn sent(&self, cx: &Context<Self>) -> Vec<(String, Vec<Attachment>)> {
        let Some(workspace) = self.workspace.upgrade() else {
            return Vec::new();
        };
        let mut sent: Vec<(String, Vec<Attachment>)> = Vec::new();
        let messages = workspace.read(cx).thread.as_ref().map(|t| &t.messages);
        for m in messages.into_iter().flatten().rev() {
            if m.role != Role::User {
                continue;
            }
            let text = crate::chat::user::without_image_markdown(&m.text)
                .trim()
                .to_string();
            let message = (text, m.attachments.clone());
            let empty = message.0.is_empty() && message.1.is_empty();
            if !empty && sent.last() != Some(&message) {
                sent.push(message);
            }
        }
        sent
    }

    /// Up on the first line (`step` -1) or Down on the last (1).
    pub(super) fn recall(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.editor.read(cx).content.clone();
        let stepping = self.recalled.as_ref().filter(|r| r.shown == content);
        let (index, draft) = match (stepping, step < 0) {
            (Some(r), true) => (Some(r.index + 1), r.draft.clone()),
            (Some(r), false) => (r.index.checked_sub(1), r.draft.clone()),
            // Down with nothing brought back stays where it is.
            (None, false) => return,
            (None, true) => (Some(0), (content.clone(), self.attachments.clone())),
        };
        let sent = self.sent(cx);
        let (text, attachments) = match index {
            Some(i) => match sent.get(i) {
                Some(message) => message.clone(),
                // Past the oldest, the oldest stays.
                None => return,
            },
            None => draft.clone(),
        };
        // Pictures and files come back with the message, as they went with it.
        self.attachments = attachments;
        let len = content.len();
        self.editor.update(cx, |e, cx| {
            e.replace(0..len, &text, window, cx);
            // Up again goes on back without walking through the message's lines first.
            if step < 0 {
                e.move_to(0, window, cx);
            }
        });
        self.recalled = index.map(|index| Recall {
            index,
            draft,
            shown: text,
        });
        cx.notify();
    }
}
