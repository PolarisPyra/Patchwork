//! The panes drawn: the layout's splits and the lines between their parts, the focused pane's
//! whole chat, every other pane's tabs and live transcript, and the places a dragged tab can drop.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, FocusHandle, KeyDownEvent, SharedString, Window, div, prelude::*, px, relative,
};
use patchwork_ui::theme::{InScheme, Theme, fs_sm, theme};

use super::{Axis, Node, PaneId, TabDrag, Zone};
use crate::chat::ChatView;
use crate::chat::backdrop;
use crate::chat::tabs::{STRIP_HEIGHT, tab_strip};
use crate::chat::text_colors::in_scheme;
use crate::chat::transcript::message_list::{sync_transcript, transcript};
use crate::workspace::Workspace;

/// The shown layout's panes, the focused one drawn with `v`'s state.
pub(crate) fn panes(
    w: &mut Workspace,
    v: &mut ChatView,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    w.load_pane_views(cx);
    w.keep_layouts(cx.has_active_drag());
    let layout = w.shown_layout();
    // A pane shown alone stops being so once another pane takes the focus.
    if layout.maximized.is_some_and(|m| m != layout.focused) {
        layout.maximized = None;
    }
    let focused = layout.focused;
    let root = match layout.maximized {
        Some(alone) => Node::Pane(alone),
        None => layout.root.clone(),
    };
    let t = *theme(cx);
    node(w, v, &root, &[], focused, &t, window, cx)
}

/// The narrowest a pane can be dragged to, in pixels.
const PANE_MIN: f32 = 240.;

/// How far an arrow key moves a line between panes, in shares of its split; Shift moves it four
/// times as far.
const LINE_STEP: f32 = 0.02;

/// A line between two parts of a split, being dragged.
#[derive(Clone)]
pub(crate) struct LineDrag {
    /// The split's place in the layout, as for [`Node::split_at`].
    path: Vec<usize>,
    /// The part the line follows.
    at: usize,
}

/// One part of the layout, at `path` in it: a pane, or a split with a line between each part that
/// can be dragged to share out the room, or pressed twice to share it equally.
#[expect(
    clippy::too_many_arguments,
    reason = "drawn as one pass over the layout"
)]
fn node(
    w: &mut Workspace,
    v: &mut ChatView,
    part: &Node,
    path: &[usize],
    focused: PaneId,
    t: &Theme,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let split = match part {
        Node::Pane(id) => return pane(w, v, *id, *id == focused, t, window, cx),
        Node::Split(split) => split,
    };
    let row = split.axis == Axis::Row;
    let mut drawn = Vec::with_capacity(split.parts.len());
    let mut lines = Vec::with_capacity(split.parts.len());
    let mut start = 0.;
    for (i, (part, size)) in split.parts.iter().zip(&split.sizes).enumerate() {
        if i > 0 {
            let focus = w.line_focus(path, i - 1, cx);
            lines.push(line(path, i - 1, start, row, &focus, t, cx));
        }
        start += size;
        let mut inner_path = path.to_vec();
        inner_path.push(i);
        let inner = node(w, v, part, &inner_path, focused, t, window, cx);
        let mut cell = div()
            .flex()
            .flex_basis(relative(0.))
            .min_w_0()
            .min_h_0()
            .when(i > 0 && row, gpui::Styled::border_l_1)
            .when(i > 0 && !row, gpui::Styled::border_t_1)
            .border_color(t.border)
            .child(inner);
        cell.style().flex_grow = Some(*size);
        cell.style().flex_shrink = Some(1.);
        drawn.push(cell);
    }
    let ws = cx.entity().downgrade();
    let here = path.to_vec();
    div()
        .relative()
        .flex()
        .when(!row, gpui::Styled::flex_col)
        .flex_1()
        .min_w_0()
        .min_h_0()
        .on_drag_move::<LineDrag>(move |e, _, cx| {
            let drag = e.drag(cx).clone();
            if drag.path != here {
                return;
            }
            let (at, origin, length) = if row {
                (e.event.position.x, e.bounds.origin.x, e.bounds.size.width)
            } else {
                (e.event.position.y, e.bounds.origin.y, e.bounds.size.height)
            };
            let length = f32::from(length);
            if length <= 0. {
                return;
            }
            let fraction = f32::from(at - origin) / length;
            // The workspace is gone only as the window closes, with nothing left to resize.
            let _ = ws.update(cx, |w, cx| {
                if let Some(split) = w.shown_layout().root.split_at(&drag.path) {
                    split.drag_line(drag.at, fraction, PANE_MIN / length);
                    cx.notify();
                }
            });
        })
        .children(drawn)
        .children(lines)
        .into_any_element()
}

/// The grip on the line after part `at` of the split at `path`, `start` of the way along it. It
/// lies over both panes beside it, so it is drawn after them. Once pressed it has the keyboard,
/// and the arrow keys along the split move it.
fn line(
    path: &[usize],
    at: usize,
    start: f32,
    row: bool,
    focus: &FocusHandle,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let hover = t.accent.opacity(0.4);
    let focused = t.accent.opacity(0.7);
    let keys_path = path.to_vec();
    let drag = LineDrag {
        path: path.to_vec(),
        at,
    };
    let id = path
        .iter()
        .fold(format!("pane-line-{at}"), |id, p| format!("{id}-{p}"));
    let path = path.to_vec();
    div()
        .id(SharedString::from(id))
        .absolute()
        .when(row, |d| {
            d.top_0()
                .bottom_0()
                .left(relative(start))
                .ml(px(-3.))
                .w(px(6.))
                .cursor_col_resize()
        })
        .when(!row, |d| {
            d.left_0()
                .right_0()
                .top(relative(start))
                .mt(px(-3.))
                .h(px(6.))
                .cursor_row_resize()
        })
        .track_focus(focus)
        .hover(move |s| s.bg(hover))
        .focus(move |s| s.bg(focused))
        .on_key_down(cx.listener(move |w, e: &KeyDownEvent, _, cx| {
            let step = match (e.keystroke.key.as_str(), row) {
                ("left", true) | ("up", false) => -LINE_STEP,
                ("right", true) | ("down", false) => LINE_STEP,
                _ => return,
            };
            let step = if e.keystroke.modifiers.shift {
                step * 4.
            } else {
                step
            };
            if let Some(split) = w.shown_layout().root.split_at(&keys_path) {
                split.nudge_line(at, step);
                cx.stop_propagation();
                cx.notify();
            }
        }))
        .on_drag(drag, |_, _, _, cx| cx.new(|_| gpui::Empty))
        .on_click(cx.listener(move |w, e: &gpui::ClickEvent, _, cx| {
            if e.click_count() == 2
                && let Some(split) = w.shown_layout().root.split_at(&path)
            {
                split.equalize();
                cx.notify();
            }
        }))
        .into_any_element()
}

/// Pane `id`: the whole chat when it has the focus, else its tabs and transcript. A press
/// anywhere in a pane out of focus gives it the focus before whatever was pressed acts.
fn pane(
    w: &mut Workspace,
    v: &mut ChatView,
    id: PaneId,
    focused: bool,
    t: &Theme,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let marked = focused && {
        let layout = w.shown_layout();
        layout.panes.len() > 1 && layout.maximized.is_none()
    };
    let body = if focused {
        crate::chat::layout::focused_chat(w, v, id, window, cx)
    } else {
        other_pane(w, id, t, window, cx)
    };
    div()
        .id(SharedString::from(format!("pane-{id}")))
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .when(!focused, |d| {
            d.capture_any_mouse_down(cx.listener(move |w, _, _, cx| w.focus_pane(id, cx)))
        })
        .child(body)
        .when(marked, |d| {
            // Which of several panes the keys go to.
            d.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(px(2.))
                    .bg(t.accent),
            )
        })
        .child(drop_zones(id, t, cx))
        .into_any_element()
}

/// A pane out of focus: its tabs over its chat's transcript, which keeps up as replies arrive,
/// with the files each reply changed, and the request its agent waits on with its answers.
fn other_pane(
    w: &mut Workspace,
    id: PaneId,
    t: &Theme,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let strip = tab_strip(w, id, t, cx);
    let folder = w.pane_folder(id);
    let body = w.in_pane(id, |w| {
        // The cards under each reply need its files, which only the focused chat loads itself.
        w.load_reply_files_in(folder, cx);
        let scheme = backdrop::chat_scheme(w);
        let _scope = scheme.map(InScheme::enter);
        let t = *theme(cx);
        sync_transcript(w);
        let body = match &w.chat {
            Some(chat) if !chat.messages.is_empty() => {
                transcript(w, None, Some(id), chat, &t, window, cx).into_any_element()
            }
            _ => quiet("No messages yet", &t).into_any_element(),
        };
        let body = match scheme {
            Some(scheme) => in_scheme(scheme, body).into_any_element(),
            None => body,
        };
        // A request the agent waits on is answered here: the press gives the pane the focus
        // first, so the answer goes to its chat.
        let banner = w
            .approval_banner(cx)
            .map(|b| div().px(px(16.)).pb(px(12.)).child(b));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(body)
            .children(banner)
            .into_any_element()
    });
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .text_color(t.text)
        .children(strip)
        .child(body.unwrap_or_else(|| quiet("Drag a tab here", t).into_any_element()))
        .into_any_element()
}

/// A short muted line in the middle of an empty pane.
fn quiet(text: &'static str, t: &Theme) -> Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .child(text)
}

/// Where a tab dragged over pane `id` can drop, under its tabs: each edge splits the pane there,
/// and the middle joins its tabs. Each lights up while a tab is over it.
fn drop_zones(id: PaneId, t: &Theme, cx: &mut Context<Workspace>) -> Div {
    let quarter = relative(0.25);
    div()
        .absolute()
        .top(px(STRIP_HEIGHT))
        .left_0()
        .right_0()
        .bottom_0()
        .child(
            zone(id, Zone::Left, t, cx)
                .top_0()
                .bottom_0()
                .left_0()
                .w(quarter),
        )
        .child(
            zone(id, Zone::Right, t, cx)
                .top_0()
                .bottom_0()
                .right_0()
                .w(quarter),
        )
        .child(
            zone(id, Zone::Top, t, cx)
                .top_0()
                .left(quarter)
                .right(quarter)
                .h(quarter),
        )
        .child(
            zone(id, Zone::Bottom, t, cx)
                .bottom_0()
                .left(quarter)
                .right(quarter)
                .h(quarter),
        )
        .child(
            zone(id, Zone::Middle, t, cx)
                .top(quarter)
                .bottom(quarter)
                .left(quarter)
                .right(quarter),
        )
}

/// One place to drop a tab in pane `id`.
fn zone(id: PaneId, zone: Zone, t: &Theme, cx: &mut Context<Workspace>) -> gpui::Stateful<Div> {
    let (fill, edge) = (t.accent.opacity(0.12), t.accent);
    div()
        .id(SharedString::from(format!("pane-{id}-drop-{zone:?}")))
        .absolute()
        .rounded(px(6.))
        .drag_over::<TabDrag>(move |s, _, _, _| s.bg(fill).border_2().border_color(edge))
        .on_drop(cx.listener(move |w, drag: &TabDrag, _, cx| {
            w.dock_tab(&drag.chat, id, zone, cx);
        }))
}
