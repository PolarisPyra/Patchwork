//! What the pill does with its keys: it sends a request to the place its chip names, answers the
//! agent it is showing the question of, and picks a project or chat from the `@` list.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AppContext, Context, Window};
use patchwork_core::Core;
use patchwork_core::orchestration::Target;
use patchwork_core::chats::{AskKind, NewChat};

use super::layout::Place;
use super::requests::{Orchestrator, Outcome, PLACEHOLDER, Picker, Waiting};

/// The most projects and chats the `@` list offers.
const MAX_ENTRIES: usize = 7;

/// An entry of the `@` list: the place it pins, as the list shows it.
pub(super) struct Entry {
    pub(super) target: Target,
    pub(super) place: Place,
}

impl Orchestrator {
    /// The question the pill is answering.
    pub(super) fn asked(&self) -> Option<(usize, &Waiting)> {
        let id = self.answering.as_ref()?;
        self.waiting
            .iter()
            .enumerate()
            .find(|(_, w)| &w.ask.approval_id == id)
    }

    /// The field's text without the space around it.
    fn text(&self, cx: &Context<Self>) -> String {
        self.field.read(cx).content.trim().to_string()
    }

    /// The core, and the chat a request starts when it names none: in the project picked under
    /// the pill.
    fn launch(&self, cx: &Context<Self>) -> Option<(Arc<Core>, NewChat)> {
        let workspace = self.workspace.upgrade()?;
        let workspace = workspace.read(cx);
        let composer = self.composer.read(cx);
        let project = composer.launch_project(&workspace.settings);
        let new = composer.launch_chat(project, &workspace.settings);
        Some((Arc::clone(&workspace.core), new))
    }

    /// Turns the pill to the question at `index`, keeping a request half written for later.
    fn answer(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(waiting) = self.waiting.get(index) else {
            return;
        };
        self.answering = Some(waiting.ask.approval_id.clone());
        let question = waiting.question();
        let written = self.text(cx);
        if self.draft.is_none() && !written.is_empty() {
            self.draft = Some(written);
        }
        self.picker = None;
        self.field.update(cx, |field, cx| {
            field.placeholder = question;
            if !field.content.is_empty() {
                field.clear(window, cx);
            }
        });
    }

    /// Turns the pill back to requests, with the one that was being written.
    fn leave(&mut self, cx: &mut Context<Self>) {
        self.answering = None;
        let draft = self.draft.take().unwrap_or_default();
        self.field.update(cx, |field, cx| {
            field.placeholder = PLACEHOLDER.into();
            field.set_text(&draft, cx);
        });
    }

    /// Brings the pill in line with what is waiting: an empty pill shows the first question, and
    /// one whose question was answered moves on.
    pub(super) fn settle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let empty = self.field.read(cx).content.is_empty();
        if self.answering.is_some() {
            if self.asked().is_none() {
                if self.waiting.is_empty() || self.draft.is_some() || !empty {
                    self.leave(cx);
                } else {
                    self.answer(0, window, cx);
                }
            }
        } else if !self.waiting.is_empty() && !self.declined && empty && self.pinned.is_none() {
            self.answer(0, window, cx);
        }
        // Tab and the arrows belong to the pill only while it has something to do with them.
        let keys = self.picker.is_some() || !self.waiting.is_empty();
        self.field.update(cx, |field, _| field.menu_keys = keys);
    }

    /// The field's text changed: the `@` list follows it.
    pub(super) fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.answering.is_none() {
            self.read_picker(cx);
        }
        self.settle(window, cx);
        cx.notify();
    }

    /// Opens, moves or closes the `@` list for the word the text ends with.
    fn read_picker(&mut self, cx: &Context<Self>) {
        let content = &self.field.read(cx).content;
        let at = content.rfind('@').filter(|at| {
            let before = content[..*at].chars().next_back();
            before.is_none_or(char::is_whitespace) && !content[*at..].contains(char::is_whitespace)
        });
        let highlighted = self.picker.as_ref().map_or(0, |p| p.highlighted);
        self.picker = at.map(|at| Picker { at, highlighted });
        let entries = self.entries(cx).len();
        match &mut self.picker {
            Some(picker) if entries > 0 => picker.highlighted = picker.highlighted.min(entries - 1),
            _ => self.picker = None,
        }
    }

    /// The projects and chats the `@` list offers for what is written after the `@`.
    pub(super) fn entries(&self, cx: &Context<Self>) -> Vec<Entry> {
        let (Some(picker), Some(workspace)) = (&self.picker, self.workspace.upgrade()) else {
            return Vec::new();
        };
        let content = &self.field.read(cx).content;
        let wanted = content
            .get(picker.at + 1..)
            .unwrap_or_default()
            .to_lowercase();
        let workspace = workspace.read(cx);
        let projects = workspace.settings.projects.iter();
        let projects = projects.map(|project| Target::New(Some(project.id.clone())));
        let chats = workspace
            .chats
            .iter()
            .filter(|t| t.parent.is_none() && t.archived_at.is_none())
            .map(|chat| Target::Chat(chat.id.clone()));
        std::iter::once(Target::New(None))
            .chain(projects)
            .chain(chats)
            .map(|target| Entry {
                place: self.place(&target, cx),
                target,
            })
            .filter(|entry| entry.place.spoken.to_lowercase().contains(&wanted))
            .take(MAX_ENTRIES)
            .collect()
    }

    /// Pins the `@` list's entry at `index` and takes its word out of the text.
    pub(super) fn pin(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(entry), Some(picker)) =
            (self.entries(cx).into_iter().nth(index), self.picker.take())
        else {
            return;
        };
        self.pinned = Some(entry.target);
        self.field.update(cx, |field, cx| {
            let end = field.content.len();
            field.replace(picker.at..end, "", window, cx);
        });
        self.focus(window, cx);
        cx.notify();
    }

    /// Up or Down: moves the `@` list's highlight.
    pub(super) fn arrow(&mut self, by: isize, cx: &mut Context<Self>) {
        let entries = self.entries(cx).len();
        if let Some(picker) = self.picker.as_mut().filter(|_| entries > 0) {
            picker.highlighted = picker
                .highlighted
                .wrapping_add_signed(by + entries.cast_signed())
                % entries;
        }
        cx.notify();
    }

    /// Tab: picks from the `@` list, or turns to the next question.
    pub(super) fn tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = &self.picker {
            self.pin(picker.highlighted, window, cx);
        } else if let Some((index, _)) = self.asked() {
            // Past the last question is the request that was being written.
            if index + 1 == self.waiting.len() && self.draft.is_some() {
                self.declined = true;
                self.leave(cx);
            } else {
                self.answer((index + 1) % self.waiting.len(), window, cx);
            }
        } else if !self.waiting.is_empty() {
            self.answer(0, window, cx);
        }
        self.settle(window, cx);
        cx.notify();
    }

    /// Escape: closes the `@` list, denies a permission, or leaves a question waiting. Says
    /// whether the key did anything here.
    pub(super) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.picker.take().is_some() {
        } else if let Some((_, waiting)) = self.asked() {
            if waiting.ask.kind == AskKind::Permission {
                self.respond(false, None, cx);
            } else if self.field.read(cx).content.is_empty() {
                self.declined = true;
                self.leave(cx);
            } else {
                self.field.update(cx, |field, cx| field.clear(window, cx));
            }
        } else if self.pinned.take().is_none() {
            return false;
        }
        self.settle(window, cx);
        cx.notify();
        true
    }

    /// Enter: picks from the `@` list, answers the question shown, or sends the request.
    pub(super) fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(picker) = &self.picker {
            return self.pin(picker.highlighted, window, cx);
        }
        let Some((_, waiting)) = self.asked() else {
            return self.submit(window, cx);
        };
        match &waiting.ask.kind {
            AskKind::Permission => self.respond(true, None, cx),
            AskKind::Question(_) => {
                let words = self.text(cx);
                if !words.is_empty() {
                    self.respond(true, Some(words), cx);
                    self.field.update(cx, |field, cx| field.clear(window, cx));
                }
            }
            AskKind::InChat => {
                // The chat's own composer answers it, so the chat opens as usual.
                let chat_id = waiting.ask.chat_id.clone();
                let _ = self.workspace.update(cx, |w, cx| w.open_chat(&chat_id, cx));
            }
        }
        cx.notify();
    }

    /// Opens a card's chat read-only, as a subagent shows: the pill sends it what comes next.
    pub(super) fn open(&self, chat_id: &str, cx: &mut Context<Self>) {
        let _ = self.workspace.update(cx, |w, cx| w.peek_chat(chat_id, cx));
    }

    /// Answers the question shown: allows or denies it, with `words` as the answer to an open
    /// question. The answer goes to the agent's chat, not through the router.
    fn respond(&mut self, allow: bool, words: Option<String>, cx: &mut Context<Self>) {
        let (Some((_, waiting)), Some(workspace)) = (self.asked(), self.workspace.upgrade()) else {
            return;
        };
        let ask = &waiting.ask;
        let answers: HashMap<String, Vec<String>> = match (&ask.kind, words) {
            (AskKind::Question(question), Some(words)) => {
                HashMap::from([(question.clone(), vec![words])])
            }
            _ => HashMap::new(),
        };
        let core = Arc::clone(&workspace.read(cx).core);
        if let Err(e) = core.respond_approval(&ask.chat_id, &ask.approval_id, allow, answers, None)
        {
            log::warn!(error:% = e; "could not answer an agent from the orchestrator: {{error}}");
        }
    }

    /// Sends what the field holds, with what is attached, to the place pinned with `@`, or to a
    /// new chat in the project picked under the pill, and lists it.
    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let Some((core, new)) = self.launch(cx).filter(|_| !text.is_empty()) else {
            return;
        };
        let target = self
            .pinned
            .take()
            .unwrap_or_else(|| Target::New(new.project_id.clone()));
        self.picker = None;
        let attachments = std::mem::take(&mut self.attachments);
        let id = self.list(&text);
        self.field.update(cx, |field, cx| field.clear(window, cx));
        let task = cx
            .background_spawn(async move { core.dispatch_to(&text, &target, &new, &attachments) });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                let outcome = match outcome {
                    Ok(dispatched) => Outcome::Routed(dispatched),
                    Err(e) => {
                        log::warn!(error:% = e; "could not dispatch a request: {{error}}");
                        Outcome::Failed(e.to_string().into())
                    }
                };
                this.settle_request(id, outcome);
                cx.notify();
            })
        })
        .detach();
    }
}
