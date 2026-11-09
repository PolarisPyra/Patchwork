//! Find in chat (`find/FindField.tsx`, `find/matches.ts`, `find/useFindHighlight.ts`): a search
//! button over the chat that widens into a find field. It counts the matches in the thread and
//! steps through them with Enter, Shift+Enter or its arrows; the transcript marks them and scrolls
//! to the current one. Ctrl+F opens it, and Esc closes it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    Context, Div, Entity, Focusable, KeyDownEvent, ListOffset, Role, Stateful, TextLayout, Window,
    div, point, prelude::*, px,
};
use patchwork_server::threads::{Message, Role as MessageRole, Thread};

use crate::chat::markdown::{FindIn, find_in};
use crate::chat::text_area::{Changed, Submitted, SubmittedBack, TextArea};
use crate::chat::timeline::{Step, steps_of};
use crate::theme::{FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{button, icon};
use crate::workspace::Workspace;

/// Room kept above a revealed match, for the controls floating over the transcript's top.
const TOP_ROOM: f32 = 80.;

#[derive(Default)]
pub struct FindState {
    field: Option<Entity<TextArea>>,
    open: bool,
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
fn texts_of(m: &Message) -> Vec<String> {
    if m.role == MessageRole::User {
        return vec![crate::chat::user::without_image_markdown(&m.text)];
    }
    steps_of(m)
        .into_iter()
        .filter_map(|s| match s {
            Step::Text(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// Every match of `needle` in the rows, in order.
fn matches(thread: &Thread, needle: &str) -> Vec<Match> {
    let mut found = Vec::new();
    for (row, m) in thread.messages.iter().filter(|m| !m.queued).enumerate() {
        let count: usize = texts_of(m).iter().map(|t| find_in(t, needle).len()).sum();
        found.extend((0..count).map(|nth| Match {
            row,
            id: m.id.clone(),
            nth,
        }));
    }
    found
}

impl Workspace {
    fn find_query(&self, cx: &gpui::App) -> String {
        self.find
            .field
            .as_ref()
            .map(|f| f.read(cx).content.to_ascii_lowercase())
            .unwrap_or_default()
    }

    fn find_matches(&self, cx: &gpui::App) -> Vec<Match> {
        match &self.thread {
            Some(thread) if self.find.open => matches(thread, &self.find_query(cx)),
            _ => Vec::new(),
        }
    }

    /// Opens the field, or focuses it when open, with its text selected so typing replaces the
    /// last search.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = match &self.find.field {
            Some(field) => field.clone(),
            None => {
                let t = *theme(cx);
                let field = cx.new(|cx| {
                    let mut field = TextArea::new("Find", "Find in chat", cx);
                    field.single_line = true;
                    field.text_color = t.text;
                    field.placeholder_color = crate::theme::input_placeholder();
                    field.selection_color = t.accent.opacity(0.3);
                    field
                });
                // The newest match first, since a chat is read from its end.
                cx.subscribe(&field, |this, _, _: &Changed, cx| {
                    this.find.current = this.find_matches(cx).len().saturating_sub(1);
                    this.find.reveal = true;
                    cx.notify();
                })
                .detach();
                cx.subscribe(&field, |this, _, _: &Submitted, cx| this.step_find(1, cx))
                    .detach();
                cx.subscribe(&field, |this, _, _: &SubmittedBack, cx| {
                    this.step_find(-1, cx)
                })
                .detach();
                self.find.field = Some(field.clone());
                field
            }
        };
        if !self.find.open {
            self.find.open = true;
            self.find.reveal = true;
        }
        window.focus(&field.read(cx).focus_handle(cx), cx);
        field.update(cx, |f, cx| f.select_everything(window, cx));
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        let editor = self.composer.read(cx).editor.clone();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
        cx.notify();
    }

    fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.find_matches(cx).len();
        if count == 0 {
            return;
        }
        let at = self.find.current.min(count - 1) as isize;
        self.find.current = (at + delta).rem_euclid(count as isize) as usize;
        self.find.reveal = true;
        cx.notify();
    }

    /// What each row marks: the query, and the current match when it is in that row. None while
    /// the field is closed or empty.
    pub fn find_marks(&self, cx: &gpui::App) -> Option<(String, Option<(String, usize)>)> {
        // Drawn again below; a match not drawn this time leaves none.
        self.find.target.borrow_mut().take();
        let needle = self.find_query(cx);
        if !self.find.open || needle.trim().is_empty() {
            return None;
        }
        let found = self.find_matches(cx);
        let current = found
            .get(self.find.current.min(found.len().saturating_sub(1)))
            .map(|m| (m.id.clone(), m.nth));
        Some((needle, current))
    }

    /// A row's marks: what to mark in message `id`, and where its current match was drawn.
    pub fn find_in_row(
        &self,
        marks: &Option<(String, Option<(String, usize)>)>,
        id: &str,
    ) -> Option<FindIn> {
        let (needle, current) = marks.as_ref()?;
        Some(FindIn {
            needle: needle.clone(),
            current: current
                .as_ref()
                .filter(|(m, _)| m == id)
                .map(|(_, nth)| *nth),
            seen: Cell::new(0),
            target: Rc::clone(&self.find.target),
        })
    }

    /// Once the transcript has drawn a new current match, scrolls it to the middle of the view if
    /// it is out of sight. A match the text did not draw, as in a code block, brings its row in.
    pub fn reveal_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find.reveal || !self.find.open {
            return;
        }
        self.find.reveal = false;
        let row = {
            let needle = self.find_query(cx);
            self.find_matches(cx)
                .get(self.find.current)
                .map(|m| self.item_of_match(m.row, m.nth, &needle))
        };
        cx.on_next_frame(window, move |this, _, cx| {
            let list = &this.transcript_list;
            let target = this.find.target.borrow().clone();
            let at = target.and_then(|(layout, i)| layout.position_for_index(i));
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
                        this.find.reveal = true;
                    }
                }
            }
            cx.notify();
        });
    }
}

fn small_button(
    id: &'static str,
    label: &'static str,
    name: &'static str,
    enabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (fg, hover) = (t.text, t.bg_hover);
    let b = button(id, label)
        .flex_none()
        .size(px(24.))
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .child(icon(name, 13.));
    if enabled {
        b.hover(move |s| s.bg(hover).text_color(fg))
    } else {
        b.opacity(0.4).cursor_default()
    }
}

/// The search button over the chat, widened into the field while open.
pub fn find_field(w: &mut Workspace, t: &Theme, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let open = w.find.open;
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
    .child(icon("magnifying-glass", 14.))
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
        .rounded(px(RADIUS))
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
    let Some(field) = w.find.field.clone().filter(|_| open) else {
        return frame;
    };
    let query = w.find_query(cx);
    let count = w.find_matches(cx).len();
    let at = w.find.current.min(count.saturating_sub(1));
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
                .pr(px(4.))
                .child(div().flex_1().min_w_0().text_size(px(FS_SM)).child(field))
                .child(
                    div()
                        .id("find-status")
                        .role(Role::Status)
                        .aria_label(status.clone())
                        .flex_none()
                        .px(px(4.))
                        .text_size(px(FS_XS))
                        .text_color(t.text_subtle)
                        .child(status),
                )
                .child({
                    let b = small_button("find-previous", "Previous match", "caret-up", any, t);
                    if any {
                        b.on_click(cx.listener(|w, _, _, cx| w.step_find(-1, cx)))
                    } else {
                        b
                    }
                })
                .child({
                    let b = small_button("find-next", "Next match", "caret-down", any, t);
                    if any {
                        b.on_click(cx.listener(|w, _, _, cx| w.step_find(1, cx)))
                    } else {
                        b
                    }
                })
                .child(
                    small_button("find-close", "Close find", "x", true, t)
                        .on_click(cx.listener(|w, _, window, cx| w.close_find(window, cx))),
                ),
        )
}
