//! Find in chat: a search button over the chat that widens into a find field.
//!
//! It counts the matches
//! in the chat and steps through them with Enter, Shift+Enter or its arrows; the transcript marks
//! them and scrolls to the current one. Ctrl+F opens it, and Esc closes it.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    Div, Entity, FocusHandle, Focusable, KeyDownEvent, ListOffset, Role, Stateful, TextLayout,
    Window, div, point, prelude::*, px,
};
use patchwork_client::local::core::chats::{Message, Role as MessageRole, Chat};

use patchwork_design_text::{markdown::FindIn, markdown::find_in};
use patchwork_design_editor::{Changed, Submitted, SubmittedBack, TextArea};
use patchwork_views_chat::{turn_contents::Step, turn_contents::steps_of};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_sm, fs_xs, radius, radius_sm, Theme};
use patchwork_design_components::button;
use patchwork_design_icons::icon;
use crate::ChatView;

/// Room kept above a revealed match, for the controls floating over the transcript's top.
const TOP_ROOM: f32 = 80.;

#[derive(Default)]
pub struct FindState {
    field: Option<Entity<TextArea>>,
    open: bool,
    /// What had the keys when the field opened, which gets them back when it closes.
    came_from: Option<FocusHandle>,
    current: usize,
    /// The current match changed and has not been scrolled to yet.
    reveal: bool,
    /// Where the current match was last drawn.
    target: Rc<RefCell<Option<(TextLayout, usize)>>>,
}

/// One place the query was found: the message's row, the message, and which of its matches.
struct Match {
    row: usize,
    id: String,
    nth: usize,
}

/// The texts of a message as the transcript draws them, in order: the user's text, or the text
/// steps of a reply. The transcript marks the same texts, so the nth match here is the nth there.
fn texts_of(m: &Message) -> Vec<Cow<'_, str>> {
    if m.role == MessageRole::User {
        return vec![Cow::Owned(
            patchwork_views_chat::image_markdown::without_image_markdown(&m.text),
        )];
    }
    steps_of(m, false)
        .into_iter()
        .filter_map(|s| match s {
            Step::Text(text) => Some(Cow::Borrowed(text)),
            _ => None,
        })
        .collect()
}

/// Every match of `needle` in the rows, in order.
fn matches(chat: &Chat, needle: &str) -> Vec<Match> {
    let mut found = Vec::new();
    for (row, m) in chat.messages.iter().filter(|m| !m.queued).enumerate() {
        let count: usize = texts_of(m).iter().map(|t| find_in(t, needle).len()).sum();
        found.extend((0..count).map(|nth| Match {
            row,
            id: m.id.clone(),
            nth,
        }));
    }
    found
}

/// What the transcript marks while the field is open: the query, the current match, by message
/// id and which of its matches, and where that match is drawn.
pub struct Marks {
    needle: String,
    current: Option<(String, usize)>,
    target: Rc<RefCell<Option<(TextLayout, usize)>>>,
}

impl Marks {
    /// What to mark in message `id`, and where its current match is drawn.
    #[must_use]
    pub fn row(&self, id: &str) -> FindIn {
        FindIn {
            needle: self.needle.clone(),
            current: self
                .current
                .as_ref()
                .filter(|(m, _)| m == id)
                .map(|(_, nth)| *nth),
            seen: Cell::new(0),
            target: Rc::clone(&self.target),
        }
    }
}

impl<W: ChatWindow> ChatView<W> {
    fn find_query(&self, cx: &gpui::App) -> String {
        self.find
            .field
            .as_ref()
            .map(|f| f.read(cx).content.to_ascii_lowercase())
            .unwrap_or_default()
    }

    fn find_matches(&self, chat: Option<&Chat>, cx: &gpui::App) -> Vec<Match> {
        match chat {
            Some(chat) if self.find.open => matches(chat, &self.find_query(cx)),
            _ => Vec::new(),
        }
    }

    /// How many matches the workspace's open chat has.
    fn match_count(&self, cx: &gpui::App) -> usize {
        self.workspace
            .upgrade()
            .map_or(0, |w| self.find_matches(w.read(cx).chat.as_ref(), cx).len())
    }

    /// Opens the field, or focuses it when open, with its text selected so typing replaces the
    /// last search.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = if let Some(field) = &self.find.field {
            field.clone()
        } else {
            let field = cx.new(|cx| {
                let mut field = TextArea::new("Find", "Find in chat", cx);
                field.single_line = true;
                field.placeholder_color = Some(patchwork_design_theme::input_placeholder());
                field
            });
            // The newest match first, since a chat is read from its end.
            cx.subscribe(&field, |this, _, _: &Changed, cx| {
                this.find.current = this.match_count(cx).saturating_sub(1);
                this.find.reveal = true;
                cx.notify();
            })
            .detach();
            cx.subscribe(&field, |this, _, _: &Submitted, cx| {
                let count = this.match_count(cx);
                this.step_find(1, count, cx);
            })
            .detach();
            cx.subscribe(&field, |this, _, _: &SubmittedBack, cx| {
                let count = this.match_count(cx);
                this.step_find(-1, count, cx);
            })
            .detach();
            self.find.field = Some(field.clone());
            field
        };
        if !self.find.open {
            self.find.open = true;
            self.find.reveal = true;
            self.find.came_from = window.focused(cx);
        }
        window.focus(&field.read(cx).focus_handle(cx), cx);
        field.update(cx, |f, cx| f.select_everything(window, cx));
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        // Back to where the keys were, not into the composer: closing a search is not a wish to
        // write a message.
        let back = self
            .find
            .came_from
            .take()
            .unwrap_or_else(|| self.home_focus.clone());
        window.focus(&back, cx);
        cx.notify();
    }

    /// Moves `delta` matches on, of `count`, wrapping around.
    fn step_find(&mut self, delta: isize, count: usize, cx: &mut Context<Self>) {
        if count == 0 {
            return;
        }
        let at = self.find.current.min(count - 1) as isize;
        self.find.current = (at + delta).rem_euclid(count as isize) as usize;
        self.find.reveal = true;
        cx.notify();
    }

    /// What the rows of `chat` mark. None while the field is closed or empty.
    pub fn find_marks(&self, chat: &Chat, cx: &gpui::App) -> Option<Marks> {
        // Drawn again below; a match not drawn this time leaves none.
        self.find.target.borrow_mut().take();
        let needle = self.find_query(cx);
        if !self.find.open || needle.trim().is_empty() {
            return None;
        }
        let found = self.find_matches(Some(chat), cx);
        let current = found
            .get(self.find.current.min(found.len().saturating_sub(1)))
            .map(|m| (m.id.clone(), m.nth));
        Some(Marks {
            needle,
            current,
            target: Rc::clone(&self.find.target),
        })
    }
}

/// The window's find bar methods, for any [`ChatWindow`].
pub trait FindBarExt: ChatWindow {
    /// Opens the find field over the open chat, or focuses it when open.
    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_view()
            .clone()
            .update(cx, |v, cx| v.open_find(window, cx));
    }

    /// Once the transcript has drawn a new current match, scrolls it to the middle of the view if
    /// it is out of sight. A match the text did not draw, as in a code block, brings its row in.
    fn reveal_find(&mut self, v: &mut ChatView<Self>, window: &mut Window, cx: &mut Context<Self>) {
        if !v.find.reveal || !v.find.open {
            return;
        }
        v.find.reveal = false;
        let row = {
            let needle = v.find_query(cx);
            v.find_matches(self.chat.as_ref(), cx)
                .get(v.find.current)
                .map(|m| self.item_of_match(m.row, m.nth, &needle, cx))
        };
        let target = Rc::clone(&v.find.target);
        cx.on_next_frame(window, move |this, _, cx| {
            let list = &this.transcript.list;
            let at = target
                .borrow()
                .clone()
                .and_then(|(layout, i)| layout.position_for_index(i));
            match at {
                Some(at) => {
                    let view = list.viewport_bounds();
                    if at.y < view.top() + px(TOP_ROOM) || at.y > view.bottom() - px(24.) {
                        list.scroll_by(at.y - view.center().y);
                    }
                }
                // Rows out of sight have no height yet, so the row goes to the top first, and the
                // match it then draws is centred on the next pass.
                None => {
                    if let Some(row) = row
                        && list.logical_scroll_top().item_ix != row
                    {
                        list.scroll_to(ListOffset {
                            item_ix: row,
                            offset_in_item: px(0.),
                        });
                        this.chat_view()
                            .clone()
                            .update(cx, |v, _| v.find.reveal = true);
                    }
                }
            }
            cx.notify();
        });
    }

    /// Moves `delta` matches on in the open chat.
    fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        let chat = self.chat.as_ref();
        self.chat_view().clone().update(cx, |v, cx| {
            let count = v.find_matches(chat, cx).len();
            v.step_find(delta, count, cx);
        });
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_view()
            .clone()
            .update(cx, |v, cx| v.close_find(window, cx));
    }
}

impl<W: ChatWindow> FindBarExt for W {}

fn small_button(
    id: &'static str,
    label: &'static str,
    name: IconName,
    enabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (fg, hover) = (t.text, t.bg_hover);
    let b = button(id, label)
        .flex_none()
        .size(px(20.))
        .justify_center()
        .rounded(px(radius_sm()))
        .text_color(t.text_muted)
        .child(icon(name, 13.));
    if enabled {
        b.hover(move |s| s.bg(hover).text_color(fg))
    } else {
        b.opacity(0.4).cursor_default()
    }
}

/// The search button over the chat, widened into the field while open.
pub fn find_field<W: ChatWindow>(
    w: &mut W,
    v: &ChatView<W>,
    t: &Theme,
    cx: &mut Context<W>,
) -> Stateful<Div> {
    let open = v.find.open;
    let fg = t.text;
    let toggle = button(
        "find-toggle",
        if open {
            "Focus find field"
        } else {
            "Find in chat"
        },
    )
    .aria_expanded(open)
    .flex_none()
    .h_full()
    .w(px(26.))
    .justify_center()
    .text_color(t.text_muted)
    .hover(move |s| s.text_color(fg))
    .child(icon(IconName::MagnifyingGlass, 14.))
    .on_click(cx.listener(|w, _, window, cx| w.open_find(window, cx)));
    let frame = div()
        .id("find")
        .role(Role::Search)
        .aria_label("Find in chat")
        .flex()
        .h(px(28.))
        .w(px(if open { 288. } else { 28. }))
        .items_center()
        .overflow_hidden()
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .shadow(vec![gpui::BoxShadow {
            color: gpui::hsla(0., 0., 0., 0.12),
            offset: point(px(0.), px(1.)),
            blur_radius: px(3.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(toggle);
    let Some(field) = v.find.field.clone().filter(|_| open) else {
        return frame;
    };
    let query = v.find_query(cx);
    let count = v.find_matches(w.chat.as_ref(), cx).len();
    let at = v.find.current.min(count.saturating_sub(1));
    let status = if query.is_empty() {
        String::new()
    } else if count == 0 {
        "No results".into()
    } else {
        format!("{} of {count}", at + 1)
    };
    let any = count > 0;
    frame
        // Esc closes only the field, not what is behind it.
        .on_key_down(cx.listener(|w, e: &KeyDownEvent, window, cx| {
            if e.keystroke.key == "escape" {
                cx.stop_propagation();
                w.close_find(window, cx);
            }
        }))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(2.))
                .pr(px(3.))
                .child(div().flex_1().min_w_0().text_size(px(fs_sm())).child(field))
                .child(
                    div()
                        .id("find-status")
                        .role(Role::Status)
                        .aria_label(status.clone())
                        .flex_none()
                        .px(px(4.))
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .child(status),
                )
                .child({
                    let b =
                        small_button("find-previous", "Previous match", IconName::CaretUp, any, t);
                    if any {
                        b.on_click(cx.listener(|w, _, _, cx| w.step_find(-1, cx)))
                    } else {
                        b
                    }
                })
                .child({
                    let b = small_button("find-next", "Next match", IconName::CaretDown, any, t);
                    if any {
                        b.on_click(cx.listener(|w, _, _, cx| w.step_find(1, cx)))
                    } else {
                        b
                    }
                })
                .child(
                    small_button("find-close", "Close find", IconName::X, true, t)
                        .on_click(cx.listener(|w, _, window, cx| w.close_find(window, cx))),
                ),
        )
}
