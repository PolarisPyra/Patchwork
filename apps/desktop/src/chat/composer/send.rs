//! Sending the draft, or queueing it, and what fills it: attachments and restored text.

use gpui::{Context, Focusable, PathPromptOptions, Window};
use patchwork_server::threads::{Attachment, AttachmentKind, GoalStatus, NewThread};
use crate::workspace::{Workspace, is_working};
use super::state::Composer;
use super::{sigil_menu};

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
            workspace.update(cx, |w, cx| w.steer_first_queued(cx));
            return;
        }
        if w.editing_queued() {
            workspace.update(cx, |w, cx| w.update_queued(text, cx));
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
            return self.run_local(command, window, cx);
        }
        // Mid-turn, /goal clear is the Stop button: the agent's session drops the goal once it
        // exits.
        if working
            && w.thread.as_ref().is_some_and(|t| t.goal.is_some())
            && crate::chat::rail::is_goal_clear(&text)
            && self.attachments.is_empty()
        {
            self.editor.update(cx, |e, cx| e.clear(window, cx));
            workspace.update(cx, |w, cx| w.stop(cx));
            return;
        }
        let queue_next = self.queue_next.unwrap_or_else(|| Self::queue_by_default(w));
        let queue = working && (alternate != queue_next);
        if queue && !self.can_queue {
            return;
        }
        // An answer written here goes to the agent's question, not into the chat.
        if workspace.read(cx).answering_in_composer() {
            if !text.is_empty() {
                workspace.update(cx, |w, cx| w.answer_question(text, cx));
                self.editor.update(cx, |e, cx| e.clear(window, cx));
            }
            return;
        }
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        // Resuming a large session whose cache has expired re-reads all of it at full price.
        if ask_resume && workspace.update(cx, |w, cx| w.ask_before_resume(cx)) {
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
            w.send_with(text, attachments, Some(new), picks.role, queue, cx)
        });
        self.editor.update(cx, |e, cx| e.clear(window, cx));
        self.queue_next = None;
        cx.notify();
    }

    /// Sends the draft once the resume prompt is answered with Continue.
    pub fn send_unasked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(false, false, window, cx);
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

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.editor.focus_handle(cx), cx);
    }

    /// Puts a restored message back to edit or send again, unless something is written already.
    pub fn fill_if_empty(
        &mut self,
        text: &str,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor.read(cx).content.trim().is_empty() || !self.attachments.is_empty() {
            return;
        }
        // The composer lists images with the other attachments, not inline in the text.
        let text = crate::chat::user::without_image_markdown(text);
        self.editor.update(cx, |e, cx| e.set_text(&text, cx));
        self.attachments = attachments;
        self.focus(window, cx);
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
            this.update(cx, |this, cx| {
                for path in paths {
                    let kind = if folders {
                        AttachmentKind::Folder
                    } else if patchwork_server::threads::image_media_type(&path.to_string_lossy())
                        .is_some()
                    {
                        AttachmentKind::Image
                    } else {
                        AttachmentKind::File
                    };
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let path = path.to_string_lossy().into_owned();
                    if !this.attachments.iter().any(|a| a.path == path) {
                        this.attachments.push(Attachment { kind, path, name });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
