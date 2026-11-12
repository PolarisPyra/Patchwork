//! Sending the draft, or queueing it, and what fills it: attachments and restored text.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, AppContext as _, ClipboardEntry, Context, Focusable, PathPromptOptions, Window};
use patchwork_server::Server;
use patchwork_server::threads::{Attachment, GoalStatus, NewThread};
use crate::workspace::{Workspace, is_working};
use super::state::Composer;
use super::sigil_menu;

impl Composer {
    /// Sends the draft. While a turn runs it queues or steers, as the send menu picked;
    /// `alternate` (Ctrl+Enter) does the other one, or steers with the first queued message.
    /// `ask_resume` asks first when the session has gone cold, unless that was just answered.
    pub(super) fn send(
        &mut self,
        alternate: bool,
        ask_resume: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.editor.read(cx).content.trim().to_string();
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        // Nothing runs while the project's folder is missing.
        if workspace.read(cx).folder_missing() {
            return;
        }
        let w = workspace.read(cx);
        let working = w.thread.as_ref().is_some_and(is_working);
        // Same as clicking the queued message's Steer button. The draft is not sent.
        if alternate && working && !w.queued().is_empty() && !w.editing_queued() {
            workspace.update(cx, crate::workspace::Workspace::steer_first_queued);
            return;
        }
        // Deferred: saving the edit reads this composer, which cannot be read from in here.
        if w.editing_queued() {
            cx.defer(move |cx| workspace.update(cx, Workspace::update_queued));
            return;
        }
        // With a plan ready, sending nothing implements it; anything written refines it.
        if text.is_empty() && self.attachments.is_empty() && w.ready_plan().is_some() {
            workspace.update(cx, |w, cx| w.implement_plan(false, cx));
            return;
        }
        if self.attachments.is_empty()
            && let Some(command) = sigil_menu::local_command(&text)
        {
            return self.run_local(command, None, window, cx);
        }
        // Mid-turn, /goal clear is the Stop button: the agent's session drops the goal once it
        // exits.
        if working
            && w.thread.as_ref().is_some_and(|t| t.goal.is_some())
            && crate::chat::rail::is_goal_clear(&text)
            && self.attachments.is_empty()
        {
            self.editor.update(cx, |e, cx| e.clear(window, cx));
            workspace.update(cx, crate::workspace::Workspace::stop);
            return;
        }
        let queue_next = self.queue_next.unwrap_or_else(|| queue_by_default(w));
        let queue = working && (alternate != queue_next);
        // An answer written here goes to the agent's question, not into the chat.
        // An answer is only text, so attached files go in by their paths rather than staying behind.
        if workspace.read(cx).answering_in_composer() {
            if !text.is_empty() || !self.attachments.is_empty() {
                let attachments = std::mem::take(&mut self.attachments);
                let answer =
                    patchwork_server::threads::answer_with_attachments(&text, &attachments);
                workspace.update(cx, |w, cx| w.answer_question(answer, cx));
                self.editor.update(cx, |e, cx| e.clear(window, cx));
                cx.notify();
            }
            return;
        }
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        // Resuming a large session whose cache has expired re-reads all of it at full price.
        if ask_resume && workspace.update(cx, crate::workspace::Workspace::ask_before_resume) {
            return;
        }
        let picks = self.picks(workspace.read(cx));
        let new = NewThread {
            id: None,
            project_id: workspace.read(cx).settings.active_project_id.clone(),
            agent: picks.agent,
            model: picks.model,
            access_mode: picks.access,
            effort: picks.effort,
            compact_window: picks.compact,
            plan_mode: self.draft.plan_mode,
        };
        let attachments = std::mem::take(&mut self.attachments);
        workspace.update(cx, |w, cx| {
            // A granular chat is asked what it runs with before its first message goes, unless
            // its project says.
            if w.thread.is_none() && w.server.asks_granular(new.project_id.as_deref()) {
                let first = (text, attachments, queue);
                w.start_granular_chat(new, picks.role, first, window, cx);
            } else {
                w.send_with(text, attachments, Some(new), picks.role, queue, cx);
            }
        });
        self.editor.update(cx, |e, cx| e.clear(window, cx));
        self.queue_next = None;
        cx.notify();
    }

    /// Sends the draft once the resume prompt is answered with Continue.
    pub fn send_unasked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(false, false, window, cx);
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
        let mut text = crate::chat::user::without_image_markdown(text);
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
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.attach_paths(&paths, cx));
        })
        .detach();
    }

    /// Attaches the files and folders at `paths`: picked, dropped on the chat, or pasted.
    pub(crate) fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.attach_with(move |server| Ok(server.attach_paths(&paths)), cx);
    }

    /// Attaches what `load` brings in, which it reads off the main thread.
    fn attach_with(
        &mut self,
        load: impl FnOnce(&Server) -> Result<Vec<Attachment>, patchwork_server::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        // The server is read once this update is over: a drop on the chat attaches from inside
        // the workspace's own update, where reading the workspace panics.
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let Ok(server) = workspace.read_with(cx, |w, _| Arc::clone(&w.server)) else {
                return;
            };
            let loaded = cx.background_spawn(async move { load(&server) }).await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(attachments) => {
                        for attachment in attachments {
                            if !this.attachments.iter().any(|a| a.path == attachment.path) {
                                this.attachments.push(attachment);
                            }
                        }
                    }
                    Err(e) => {
                        if let Some(workspace) = this.workspace.upgrade() {
                            workspace.update(cx, |w, cx| {
                                w.error = Some(e.to_string());
                                cx.notify();
                            });
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Attaches the pictures and files on the clipboard. False when it holds only text, which
    /// is the editor's to paste.
    pub(super) fn paste_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        let (mut paths, mut pictures) = (Vec::new(), Vec::new());
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
                    if copied.is_empty() && !text.text().trim().is_empty() {
                        return false;
                    }
                    paths.extend(copied);
                }
            }
        }
        if paths.is_empty() && pictures.is_empty() {
            return false;
        }
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.attach_with(
            move |server| {
                let mut attachments = server.attach_paths(&paths);
                for picture in &pictures {
                    attachments.push(server.attach_image(picture)?);
                }
                Ok(attachments)
            },
            cx,
        );
        true
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
pub(super) fn queue_by_default(w: &Workspace) -> bool {
    let goal_active = w
        .thread
        .as_ref()
        .and_then(|t| t.goal.as_ref())
        .is_some_and(|g| g.status == GoalStatus::Active);
    goal_active || !w.settings.steer_while_working
}
