//! What the pill does with its keys: it sends a request to the place its chip names, answers the
//! agent it is showing the question of, and picks a project or chat from the `@` list.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Context, Window};
use patchwork_server::Server;
use patchwork_server::orchestration::Target;
use patchwork_server::threads::{AskKind, NewThread};

use super::render::Place;
use super::state::{Orchestrator, Outcome, PLACEHOLDER, Picker, Waiting};

/// How long after the last key the router is asked where the request would go.
const PAUSE: Duration = Duration::from_millis(350);

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

    /// The route shown was read from the text the field holds now.
    pub(super) fn route_is_fresh(&self, cx: &Context<Self>) -> bool {
        self.routing.is_some() && self.routed_text == self.field.read(cx).content.trim()
    }

    /// The place Enter sends the request to, when one is known.
    pub(super) fn target(&self, cx: &Context<Self>) -> Option<&Target> {
        self.pinned.as_ref().or_else(|| {
            let routing = self.routing.as_ref().filter(|_| self.route_is_fresh(cx))?;
            routing.targets.get(self.pick)
        })
    }

    /// The server, and the chat a request starts when it names none.
    fn launch(&self, cx: &Context<Self>) -> Option<(Arc<Server>, NewThread)> {
        let workspace = self.workspace.upgrade()?;
        let workspace = workspace.read(cx);
        let project = workspace.settings.active_project_id.clone();
        let new = workspace
            .composer
            .read(cx)
            .new_thread(project, &workspace.settings);
        Some((Arc::clone(&workspace.server), new))
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
        let keys = self.picker.is_some()
            || !self.waiting.is_empty()
            || !self.field.read(cx).content.is_empty();
        self.field.update(cx, |field, _| field.menu_keys = keys);
    }

    /// The field's text changed: the `@` list follows it, and the route is read again.
    pub(super) fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.answering.is_none() {
            self.read_picker(cx);
            self.preview(cx);
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
            .threads
            .iter()
            .filter(|t| t.parent.is_none() && t.archived_at.is_none())
            .map(|thread| Target::Chat(thread.id.clone()));
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

    /// Asks the router where the request would go, once the typing has paused. A reading asked
    /// for before the latest key is dropped when it comes back.
    fn preview(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let text = self.text(cx);
        let idle = text.is_empty() || self.pinned.is_some() || self.picker.is_some();
        let Some((server, new)) = self.launch(cx).filter(|_| !idle) else {
            self.deciding = false;
            return;
        };
        self.deciding = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PAUSE).await;
            let current = this.read_with(cx, |this, _| this.generation == generation);
            if !current.unwrap_or(false) {
                return;
            }
            let asked = text.clone();
            let routing = cx
                .background_executor()
                .spawn(async move { server.route(&asked, &new) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.deciding = false;
                this.routing = routing.ok();
                this.routed_text = text;
                this.pick = 0;
                cx.notify();
            });
        })
        .detach();
    }

    /// Up or Down: moves the `@` list's highlight, or the chip to the next place.
    pub(super) fn arrow(&mut self, by: isize, cx: &mut Context<Self>) {
        let entries = self.entries(cx).len();
        if let Some(picker) = self.picker.as_mut().filter(|_| entries > 0) {
            picker.highlighted = picker
                .highlighted
                .wrapping_add_signed(by + entries.cast_signed())
                % entries;
        } else if self.answering.is_none() && self.route_is_fresh(cx) {
            let places = self.routing.as_ref().map_or(0, |r| r.targets.len());
            if places > 0 {
                self.pick = self.pick.wrapping_add_signed(by + places.cast_signed()) % places;
            }
        }
        cx.notify();
    }

    /// Tab: picks from the `@` list, turns to the next question, or moves the chip on.
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
        } else {
            self.arrow(1, cx);
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
                let thread_id = waiting.ask.thread_id.clone();
                self.open(&thread_id, cx);
            }
        }
        cx.notify();
    }

    /// Opens a chat on the Chats page.
    pub(super) fn open(&self, thread_id: &str, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |w, cx| w.open_thread(thread_id, cx));
        }
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
        let server = Arc::clone(&workspace.read(cx).server);
        if let Err(e) =
            server.respond_approval(&ask.thread_id, &ask.approval_id, allow, answers, None)
        {
            log::warn!(error:% = e; "could not answer an agent from the orchestrator: {{error}}");
        }
    }

    /// Sends what the field holds to the place the chip names, or to the place the router finds
    /// when it has not answered yet, and lists it.
    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let Some((server, new)) = self.launch(cx).filter(|_| !text.is_empty()) else {
            return;
        };
        let prompt = match (&self.pinned, &self.routing) {
            (None, Some(routing)) if self.route_is_fresh(cx) => routing.prompt.clone(),
            _ => text.clone(),
        };
        let target = self.target(cx).cloned();
        self.generation += 1;
        self.deciding = false;
        self.routing = None;
        self.pinned = None;
        self.picker = None;
        self.pick = 0;
        let id = self.list(&text);
        self.field.update(cx, |field, cx| field.clear(window, cx));
        // Without a place picked the server asks a model which chat is meant, which takes a
        // moment.
        let task = cx.background_spawn(async move {
            if let Some(target) = target {
                return server.dispatch_to(&prompt, &target, &new);
            }
            let routing = server.route(&text, &new)?;
            let target = routing.targets.first().ok_or("No chat to send it to")?;
            server.dispatch_to(&routing.prompt, target, &new)
        });
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
                if let Some(request) = this.requests.iter_mut().find(|r| r.id == id) {
                    request.outcome = outcome;
                }
                cx.notify();
            })
        })
        .detach();
    }
}
