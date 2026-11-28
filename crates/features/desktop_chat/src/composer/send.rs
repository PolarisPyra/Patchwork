//! Sending the draft, or queueing it, and what fills it: attachments and restored text.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext as _, ClipboardEntry, Context, Focusable, PathPromptOptions, Window};
use patchwork_client::local::core::Core;
use patchwork_client::local::core::chats::{Attachment, GoalStatus, NewChat, is_long_paste};
use crate::host::{ChatEvent, ChatHost, Outgoing};
use super::draft::Composer;
use super::sigil_menu;

impl Composer {
    /// Sends the draft. While a turn runs it queues or steers, as the send menu picked;
    /// `alternate` (Ctrl+Enter) does the other one, or steers with the first queued message.
    pub(super) fn send(&mut self, alternate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).content.trim().to_string();
        let host = std::rc::Rc::clone(&self.host);
        // Nothing runs while the project's folder is missing.
        if host.folder_missing(cx) {
            return;
        }
        let working = host
            .chat(cx)
            .is_some_and(patchwork_client::local::core::chats::Chat::is_replying);
        // Same as clicking the queued message's Steer button. The draft is not sent.
        if alternate && working && host.has_queued(cx) && !host.editing_queued(cx) {
            cx.emit(ChatEvent::SteerFirstQueued);
            return;
        }
        // Handled once this update is over: saving the edit reads this composer, which cannot be
        // read from in here.
        if host.editing_queued(cx) {
            cx.emit(ChatEvent::UpdateQueued);
            return;
        }
        if self.attachments.is_empty()
            && let Some(command) = sigil_menu::local_command(&text)
        {
            return self.run_local(command, None, window, cx);
        }
        let queue_next = self
            .queue_next
            .unwrap_or_else(|| queue_by_default(host.as_ref(), cx));
        let queue = working && (alternate != queue_next);
        // An answer written here goes to the agent's question, not into the chat.
        // An answer is only text, so attached files go in by their paths rather than staying behind.
        if host.answering(cx) {
            if !text.is_empty() || !self.attachments.is_empty() {
                let attachments = std::mem::take(&mut self.attachments);
                let answer = patchwork_client::local::core::chats::answer_with_attachments(
                    &text,
                    &attachments,
                );
                host.answer(answer, cx);
                self.editor.update(cx, |e, cx| e.clear(window, cx));
                cx.notify();
            }
            return;
        }
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        let picks = self.picks(cx);
        let new = NewChat {
            id: None,
            project_id: AppState::get(cx).settings.window.active_project_id.clone(),
            agent: picks.agent,
            model: picks.model,
            access_mode: picks.access,
            effort: picks.effort,
            compact_window: picks.compact,
        };
        let attachments = std::mem::take(&mut self.attachments);
        // On an agent's page the message starts a run of the agent.
        if host.agent_shown(cx) {
            let kept = attachments.clone();
            if !host.send_to_agent(text, attachments, new, cx) {
                self.attachments = kept;
                return;
            }
            self.editor.update(cx, |e, cx| e.clear(window, cx));
            cx.notify();
            return;
        }
        host.send(
            Outgoing {
                text,
                attachments,
                new,
                role: picks.role,
                queue,
            },
            window,
            cx,
        );
        self.editor.update(cx, |e, cx| e.clear(window, cx));
        self.queue_next = None;
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.editor.focus_handle(cx), cx);
    }

    /// What is written and attached, unsent.
    pub fn draft(&self, cx: &App) -> (String, Vec<Attachment>) {
        (
            self.editor.read(cx).content.clone(),
            self.attachments.clone(),
        )
    }

    /// Replaces what is written and attached.
    pub fn set_draft(&mut self, text: &str, attachments: Vec<Attachment>, cx: &mut Context<Self>) {
        self.editor.update(cx, |e, cx| e.set_text(text, cx));
        self.attachments = attachments;
        self.recalled = None;
        self.draft_changed();
        cx.notify();
    }

    /// Puts a restored message back to edit or send again, ahead of anything written since.
    pub fn fill(
        &mut self,
        text: &str,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.put_back(text, attachments, cx);
        self.focus(window, cx);
    }

    /// Puts a message that was not sent back, ahead of anything written since.
    pub fn put_back(&mut self, text: &str, attachments: Vec<Attachment>, cx: &mut Context<Self>) {
        // The composer lists images with the other attachments, not inline in the text.
        let mut text = patchwork_views_chat::image_markdown::without_image_markdown(text);
        let written = self.editor.read(cx).content.trim().to_string();
        if !written.is_empty() {
            text = format!("{text}\n\n{written}");
        }
        self.editor.update(cx, |e, cx| e.set_text(&text, cx));
        // A file attached again since is kept once.
        self.attachments
            .retain(|a| !attachments.iter().any(|b| b.path == a.path));
        self.attachments.splice(0..0, attachments);
        self.recalled = None;
        self.draft_changed();
        cx.notify();
    }

    pub(super) fn attach(&mut self, folders: bool, cx: &mut Context<Self>) {
        patchwork_design_tasks::pick_paths(
            cx,
            PathPromptOptions {
                files: !folders,
                directories: folders,
                multiple: true,
                prompt: None,
            },
            move |this, paths, cx| this.attach_paths(&paths, cx),
        );
    }

    /// Attaches the files and folders at `paths`: picked, dropped on the chat, or pasted.
    pub fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.attach_with(move |core| Ok(core.attach_paths(&paths)), cx);
    }

    /// Attaches what `load` brings in, which it reads off the main thread.
    fn attach_with(
        &mut self,
        load: impl FnOnce(&Core) -> Result<Vec<Attachment>, patchwork_client::local::core::Error>
        + Send
        + 'static,
        cx: &mut Context<Self>,
    ) {
        let core = Arc::clone(AppState::get(cx).core());
        cx.spawn(async move |this, cx| {
            let loaded = cx.background_spawn(async move { load(&core) }).await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(attachments) => {
                        for attachment in attachments {
                            if !this.attachments.iter().any(|a| a.path == attachment.path) {
                                this.attachments.push(attachment);
                            }
                        }
                    }
                    Err(e) => AppState::set_error(cx, Some(e.to_string())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Attaches the pictures and files on the clipboard, and text too long for the field as a
    /// file. False when it holds shorter text, which is the editor's to paste.
    pub(super) fn paste_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        let (mut paths, mut pictures, mut texts) = (Vec::new(), Vec::new(), Vec::new());
        // Text copied along with a picture is still pasted as text.
        let mut written = false;
        for entry in cx
            .read_from_clipboard()
            .iter()
            .flat_map(gpui::ClipboardItem::entries)
        {
            match entry {
                ClipboardEntry::Image(image) => pictures.push(image.bytes.clone()),
                ClipboardEntry::ExternalPaths(files) => paths.extend(files.paths().iter().cloned()),
                // A file copied in a file manager comes as its address.
                ClipboardEntry::String(text) => {
                    let copied = copied_files(text.text());
                    if copied.is_empty() && is_long_paste(text.text()) {
                        texts.push(text.text().to_owned());
                    } else {
                        written |= copied.is_empty() && !text.text().trim().is_empty();
                    }
                    paths.extend(copied);
                }
            }
        }
        if paths.is_empty() && pictures.is_empty() && texts.is_empty() {
            return false;
        }
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.attach_with(
            move |core| {
                let mut attachments = core.attach_paths(&paths);
                for picture in &pictures {
                    attachments.push(core.attach_image(picture)?);
                }
                for text in &texts {
                    attachments.push(core.attach_text(text)?);
                }
                Ok(attachments)
            },
            cx,
        );
        !written
    }
}

/// The files `text` names when every line of it is a `file://` address, as a file manager copies
/// them.
fn copied_files(text: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Some(path) = line.trim().strip_prefix("file://") else {
            return Vec::new();
        };
        // Addresses write a space as `%20`, and any other byte the same way.
        let mut bytes = Vec::with_capacity(path.len());
        let mut rest = path.as_bytes();
        while let Some((&byte, after)) = rest.split_first() {
            let hex = (byte == b'%')
                .then(|| after.get(..2))
                .flatten()
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok());
            if let Some(decoded) = hex {
                bytes.push(decoded);
                rest = &after[2..];
            } else {
                bytes.push(byte);
                rest = after;
            }
        }
        files.push(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()));
    }
    files
}

/// Holds the next message for after the running turn, unless the setting makes Enter steer
/// a running reply. While a goal runs, follow-ups wait by default.
pub(super) fn queue_by_default(host: &dyn ChatHost, cx: &App) -> bool {
    let goal_active = host
        .chat(cx)
        .and_then(|t| t.goal.as_ref())
        .is_some_and(|g| g.status == GoalStatus::Active);
    goal_active || !AppState::get(cx).settings.steer_while_working
}
