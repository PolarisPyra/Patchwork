//! The orchestrator page's state: what became of each request sent, and the questions agents
//! are waiting on.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{AppContext, Context, Entity, SharedString, Subscription, Window};
use patchwork_client::local::core::orchestration::{Dispatched, Target};
use patchwork_desktop_chat::{ChatEvent, Composer};
use patchwork_model::chats::{Ask, asks};

use patchwork_design_backgrounds::Flight;
use patchwork_design_text::display_title;
use crate::host::{OrchestratorEvent, PageHost};

/// The most requests the page lists. Older ones leave it; their chats stay in the task list.
const MAX_REQUESTS: usize = 6;

/// What became of a request.
pub(super) enum Outcome {
    /// The core is starting its chat or sending it on.
    Routing,
    /// A chat has it, or one in each of several projects.
    Routed(Dispatched),
    /// No chat got it, and why.
    Failed(SharedString),
}

/// A request sent from the page.
pub(super) struct Request {
    pub(super) id: usize,
    pub(super) text: SharedString,
    pub(super) outcome: Outcome,
}

/// Something an agent is waiting to be told, with the name of its chat.
pub(super) struct Waiting {
    pub(super) ask: Ask,
    pub(super) chat: SharedString,
}

impl Waiting {
    /// The question in words, such as `Fix login redirect asks: run "cargo test"?`.
    pub(super) fn question(&self) -> SharedString {
        let Ask { title, detail, .. } = &self.ask;
        let asked = if detail.is_empty() {
            title.clone()
        } else {
            let mut title = title.chars();
            let lower: String = title
                .next()
                .into_iter()
                .flat_map(char::to_lowercase)
                .chain(title)
                .collect();
            format!("{lower} \"{detail}\"?")
        };
        format!("{} asks: {asked}", self.chat).into()
    }
}

/// The orchestrator page: the chat box, which sends each request to the chat it belongs in, and
/// the questions agents are waiting on.
pub struct Orchestrator {
    /// The chat box requests are written in, the same one chats have.
    pub(super) composer: Entity<Composer>,
    /// The requests sent since the app started, newest first.
    pub(super) requests: Vec<Request>,
    pub(super) next_id: usize,
    /// What agents are waiting to be told, in the order of the task list.
    pub(super) waiting: Vec<Waiting>,
    /// The approval shown above the chat box.
    pub(super) answering: Option<String>,
    /// The person put the questions aside for now, so the chat box sends requests.
    pub(super) declined: bool,
    /// The place picked with `@`. Without one a request starts a chat in the project open in
    /// the sidebar.
    pub(super) pinned: Option<Target>,
    /// Where the butterfly is, kept between frames.
    pub(super) flight: Rc<RefCell<Flight>>,
    /// The picture last drawn behind the page, under the one drawn now.
    pub(super) pictures: [Option<PathBuf>; 2],
    _subscriptions: [Subscription; 2],
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("requests", &self.requests.len())
            .field("waiting", &self.waiting.len())
            .finish_non_exhaustive()
    }
}

impl Orchestrator {
    /// The page, with a chat box of its own.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let host = Rc::new(PageHost(cx.weak_entity()));
        let composer = cx.new(|cx| Composer::new(host, window, cx));
        let subscriptions = [
            cx.subscribe(&composer, |_, _, event: &ChatEvent, cx| {
                if let ChatEvent::OpenSettings(page) = event {
                    cx.emit(OrchestratorEvent::OpenSettings(page));
                }
            }),
            // A chat's title and what it asks are read from the app's state.
            cx.observe_global::<AppState>(|this, cx| {
                this.read_chats(cx);
                this.settle();
                cx.notify();
            }),
        ];
        Self {
            composer,
            requests: Vec::new(),
            next_id: 0,
            waiting: Vec::new(),
            answering: None,
            declined: false,
            pinned: None,
            flight: Rc::default(),
            pictures: [None, None],
            _subscriptions: subscriptions,
        }
    }

    /// Puts the cursor in the chat box.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |c, cx| c.focus(window, cx));
    }

    /// Puts the cursor in the chat box with `text` added at its end, for a key typed outside it.
    pub fn type_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |c, cx| {
            c.editor.update(cx, |e, cx| {
                let end = e.content.len();
                e.replace(end..end, text, window, cx);
            });
            c.focus(window, cx);
        });
    }

    /// Notes what every chat's agent asks.
    fn read_chats(&mut self, cx: &mut Context<Self>) {
        let chats = &AppState::get(cx).chats;
        let waiting: Vec<Waiting> = asks(chats)
            .into_iter()
            .map(|ask| {
                let chat = chats
                    .iter()
                    .find(|t| t.id == ask.chat_id)
                    .map(|t| display_title(&t.title))
                    .unwrap_or_default();
                Waiting {
                    ask,
                    chat: chat.into(),
                }
            })
            .collect();
        // A question not seen before is offered even after the others were turned down.
        let known = |id: &str| self.waiting.iter().any(|w| w.ask.approval_id == id);
        if waiting.is_empty() || waiting.iter().any(|w| !known(&w.ask.approval_id)) {
            self.declined = false;
        }
        self.waiting = waiting;
    }

    /// Lists a request just sent, and gives its id.
    pub(super) fn list(&mut self, text: &str) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.requests.insert(
            0,
            Request {
                id,
                text: text.to_string().into(),
                outcome: Outcome::Routing,
            },
        );
        self.requests.truncate(MAX_REQUESTS);
        id
    }

    /// Records what became of request `id`, keeping one card per chat: a chat stays on the card
    /// of the newest request sent to it, whatever order the answers come back in, and a card left
    /// without chats leaves the page.
    pub(super) fn settle_request(&mut self, id: usize, mut outcome: Outcome) {
        if let Outcome::Routed(to) = &mut outcome {
            let newer: Vec<String> = self
                .requests
                .iter()
                .filter(|r| r.id > id)
                .filter_map(|r| match &r.outcome {
                    Outcome::Routed(d) => Some(d.chat_ids.iter().cloned()),
                    _ => None,
                })
                .flatten()
                .collect();
            to.chat_ids.retain(|t| !newer.contains(t));
            if to.chat_ids.is_empty() {
                self.requests.retain(|r| r.id != id);
                return;
            }
            self.requests
                .retain_mut(|request| match &mut request.outcome {
                    Outcome::Routed(older) if request.id < id => {
                        older.chat_ids.retain(|t| !to.chat_ids.contains(t));
                        !older.chat_ids.is_empty()
                    }
                    _ => true,
                });
        }
        if let Some(request) = self.requests.iter_mut().find(|r| r.id == id) {
            request.outcome = outcome;
        }
    }
}
