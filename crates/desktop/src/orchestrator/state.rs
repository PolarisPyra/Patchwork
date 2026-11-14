//! The orchestrator page's state: the request being written, what became of each one sent, the
//! and the questions agents are waiting on.

// Rust guideline compliant 2026-10-09

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AppContext, Context, Entity, Focusable, SharedString, Subscription, WeakEntity, Window, hsla,
};
use patchwork_server::orchestration::{Dispatched, Routing, Target};
use patchwork_server::threads::{Ask, asks};

use super::butterfly::Flight;
use crate::ui::display_title;
use crate::ui::text_area::{Arrow, BackspaceEmpty, Changed, Submitted, Tabbed, TextArea};
use crate::workspace::Workspace;

/// The most requests the page lists. Older ones leave it; their chats stay in the task list.
const MAX_REQUESTS: usize = 6;

/// What the field shows while it is empty and nothing is waiting.
pub(super) const PLACEHOLDER: &str = "Do something in a project";

/// What became of a request.
pub(super) enum Outcome {
    /// The server is finding its chat.
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
    /// The question as the pill words it, such as `Fix login redirect asks: run "cargo test"?`.
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

/// The list of projects and chats opened by `@`.
pub(super) struct Picker {
    /// Where the `@` is in the field's text.
    pub(super) at: usize,
    pub(super) highlighted: usize,
}

/// The orchestrator page: one field that sends each request to the chat it belongs in, and
/// answers the agents that are waiting.
pub struct Orchestrator {
    pub(super) workspace: WeakEntity<Workspace>,
    pub(super) field: Entity<TextArea>,
    /// The requests sent since the app started, newest first.
    pub(super) requests: Vec<Request>,
    pub(super) next_id: usize,
    /// What agents are waiting to be told, in the order of the task list.
    pub(super) waiting: Vec<Waiting>,
    /// The approval the pill is answering, when it is answering one.
    pub(super) answering: Option<String>,
    /// The person turned the questions down for now, so the pill stays on requests.
    pub(super) declined: bool,
    /// The request being written when the pill turned to a question.
    pub(super) draft: Option<String>,
    /// Where the request would go, the text that was read to find out, and the place picked.
    pub(super) routing: Option<Routing>,
    pub(super) routed_text: String,
    pub(super) pick: usize,
    /// The place picked with `@`, which the router does not change.
    pub(super) pinned: Option<Target>,
    /// The router is reading the request.
    pub(super) deciding: bool,
    /// Counts the readings asked for, so a late answer to an old one is dropped.
    pub(super) generation: u64,
    pub(super) picker: Option<Picker>,
    /// Where the butterfly is, kept between frames.
    pub(super) flight: Rc<RefCell<Flight>>,
    _subscriptions: [Subscription; 6],
}

impl Orchestrator {
    pub fn new(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextArea::new(PLACEHOLDER, "Request", cx);
            field.single_line = true;
            // The page's render sets the text's colors, which follow the backdrop.
            field.selection_color = hsla(0.55, 0.9, 0.65, 0.35);
            field
        });
        let subscriptions = [
            cx.subscribe_in(&field, window, |this, _, _: &Submitted, window, cx| {
                this.enter(window, cx);
            }),
            cx.subscribe_in(&field, window, |this, _, _: &Changed, window, cx| {
                this.changed(window, cx);
            }),
            cx.subscribe_in(&field, window, |this, _, arrow: &Arrow, _, cx| {
                this.arrow(arrow.0, cx);
            }),
            cx.subscribe_in(&field, window, |this, _, _: &Tabbed, window, cx| {
                this.tab(window, cx);
            }),
            cx.subscribe_in(&field, window, |this, _, _: &BackspaceEmpty, _, cx| {
                this.pinned = None;
                cx.notify();
            }),
            // A chat's title and what it asks are read from the workspace.
            cx.observe_in(workspace, window, |this, _, window, cx| {
                this.read_chats(cx);
                this.settle(window, cx);
                cx.notify();
            }),
        ];
        Self {
            workspace: workspace.downgrade(),
            field,
            requests: Vec::new(),
            next_id: 0,
            waiting: Vec::new(),
            answering: None,
            declined: false,
            draft: None,
            routing: None,
            routed_text: String::new(),
            pick: 0,
            pinned: None,
            deciding: false,
            generation: 0,
            picker: None,
            flight: Rc::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Puts the cursor in the field.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.field.read(cx).focus_handle(cx), cx);
    }

    /// Puts the cursor in the field with `text` added at its end, for a key typed outside it.
    pub fn type_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.field.update(cx, |field, cx| {
            let end = field.content.len();
            field.replace(end..end, text, window, cx);
        });
        self.focus(window, cx);
    }

    /// Notes what every chat's agent asks.
    fn read_chats(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let threads = &workspace.read(cx).threads;
        let waiting: Vec<Waiting> = asks(threads)
            .into_iter()
            .map(|ask| {
                let chat = threads
                    .iter()
                    .find(|t| t.id == ask.thread_id)
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
}
