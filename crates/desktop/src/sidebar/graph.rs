//! The Graph pane: the commit history of the checked-out branch and its upstream, drawn as lanes. A
//! commit not pushed yet is a hollow dot, and the lines down from it are dashed.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::collections::HashMap;

use gpui::{
    AnyElement, Bounds, Div, FontWeight, Hsla, MouseButton, MouseDownEvent, PathBuilder, Pixels,
    Point, Role, SharedString, Window, canvas, div, point, prelude::*, px,
};
use patchwork_server::threads::Thread;
use patchwork_server::workspace::GitCommit;

use super::changes::ChangesView;
use crate::ui::menu::MenuItem;
use crate::ui::theme::{FS_MD, FS_SM, radius_sm, Theme};
use crate::ui::{agent_logo, button, mix, tooltip};

/// Row height, lane width, and the radius of the bends where a lane shifts over.
const ROW: f32 = 22.;
const LANE: f32 = 11.;
const BEND: f32 = 5.;

fn x(lane: usize) -> f32 {
    LANE * (lane as f32 + 1.)
}

/// A lane waiting for the commit `id` further down, drawn in `color`; `pending` while it leads
/// down from a commit not pushed yet.
#[derive(Clone)]
struct Lane {
    id: String,
    color: Hsla,
    pending: bool,
}

struct GraphRow<'a> {
    commit: &'a GitCommit,
    input: Vec<Lane>,
    output: Vec<Lane>,
    lane: usize,
    color: Hsla,
}

fn is_head(commit: &GitCommit) -> bool {
    commit.refs.iter().any(|r| r.kind == "head")
}

/// The checked-out branch's lane is the accent; every other lane takes a muted shade of it, in
/// the order lanes open.
fn lane_colors(t: &Theme) -> [Hsla; 4] {
    [
        mix(t.accent, t.text_subtle, 45.),
        t.text_muted,
        mix(t.accent, t.text_subtle, 20.),
        t.text_subtle,
    ]
}

/// Which lanes run through each commit's row, read top to bottom from commits in graph order.
fn layout<'a>(commits: &'a [GitCommit], t: &Theme) -> Vec<GraphRow<'a>> {
    let colors = lane_colors(t);
    let head = t.accent;
    let mut rows: Vec<GraphRow> = Vec::new();
    let mut color_index = colors.len() - 1;
    let mut next_color = || {
        color_index = (color_index + 1) % colors.len();
        colors[color_index]
    };
    for commit in commits {
        let input = rows.last().map(|r| r.output.clone()).unwrap_or_default();
        let mut output = Vec::new();
        let pending = !commit.pushed;
        let mut first_parent_added = false;
        if !commit.parents.is_empty() {
            for lane in &input {
                if lane.id != commit.hash {
                    output.push(lane.clone());
                } else if !first_parent_added {
                    // The first lane reaching this commit carries on to its first parent.
                    let color = if is_head(commit) { head } else { lane.color };
                    output.push(Lane {
                        id: commit.parents[0].clone(),
                        color,
                        pending,
                    });
                    first_parent_added = true;
                }
            }
        }
        // A branch tip no lane reached yet, and each further parent of a merge, opens a lane.
        for (i, parent) in commit
            .parents
            .iter()
            .enumerate()
            .skip(usize::from(first_parent_added))
        {
            let color = if i == 0 && is_head(commit) {
                head
            } else {
                next_color()
            };
            output.push(Lane {
                id: parent.clone(),
                color,
                pending,
            });
        }
        let lane = input
            .iter()
            .position(|l| l.id == commit.hash)
            .unwrap_or(input.len());
        let color = output
            .get(lane)
            .or(input.get(lane))
            .map_or(if is_head(commit) { head } else { colors[0] }, |l| l.color);
        rows.push(GraphRow {
            commit,
            input,
            output,
            lane,
            color,
        });
    }
    rows
}

/// One stroke of the row's drawing, in the row's own coordinates.
enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    /// An arc of radius `r` to the point, clockwise when `sweep`.
    Arc(f32, bool, f32, f32),
}

struct Stroke {
    segs: Vec<Seg>,
    color: Hsla,
    dashed: bool,
}

/// The row's lanes, as `RowGraph` draws them.
fn strokes(row: &GraphRow) -> Vec<Stroke> {
    use Seg::{Move, Arc, Line};
    let (commit, circle) = (row.commit, row.lane);
    let mid = ROW / 2.;
    let mut out_strokes = Vec::new();
    let mut out = 0;
    for (i, lane) in row.input.iter().enumerate() {
        if lane.id == commit.hash {
            // Another lane waiting for this commit joins its dot here.
            if i == circle {
                out += 1;
            } else {
                out_strokes.push(Stroke {
                    segs: vec![
                        Move(x(i), 0.),
                        Arc(LANE, true, x(i) - LANE, mid),
                        Line(x(circle), mid),
                    ],
                    color: lane.color,
                    dashed: lane.pending,
                });
            }
        } else if out < row.output.len() && lane.id == row.output[out].id {
            let segs = if i == out {
                vec![Move(x(i), 0.), Line(x(i), ROW)]
            } else {
                vec![
                    Move(x(i), 0.),
                    Line(x(i), 6.),
                    Arc(BEND, true, x(i) - BEND, mid),
                    Line(x(out) + BEND, mid),
                    Arc(BEND, false, x(out), mid + BEND),
                    Line(x(out), ROW),
                ]
            };
            out_strokes.push(Stroke {
                segs,
                color: lane.color,
                dashed: lane.pending,
            });
            out += 1;
        }
    }
    // A merge's further parents branch off to their own lanes.
    for parent in commit.parents.iter().skip(1) {
        let Some(p) = row.output.iter().rposition(|l| &l.id == parent) else {
            continue;
        };
        out_strokes.push(Stroke {
            segs: vec![
                Move(x(p) - LANE, mid),
                Arc(LANE, true, x(p), ROW),
                Move(x(p) - LANE, mid),
                Line(x(circle), mid),
            ],
            color: row.output[p].color,
            dashed: row.output[p].pending,
        });
    }
    if let Some(lane) = row.input.get(circle) {
        out_strokes.push(Stroke {
            segs: vec![Move(x(circle), 0.), Line(x(circle), mid)],
            color: lane.color,
            dashed: lane.pending,
        });
    }
    if !commit.parents.is_empty() {
        out_strokes.push(Stroke {
            segs: vec![Move(x(circle), mid), Line(x(circle), ROW)],
            color: row.color,
            dashed: !commit.pushed,
        });
    }
    out_strokes
}

fn circle_path(c: Point<Pixels>, r: f32, stroke: Option<f32>) -> Option<gpui::Path<Pixels>> {
    let r = px(r);
    let mut path = match stroke {
        Some(width) => PathBuilder::stroke(px(width)),
        None => PathBuilder::fill(),
    };
    path.move_to(point(c.x + r, c.y));
    path.arc_to(point(r, r), px(0.), false, true, point(c.x - r, c.y));
    path.arc_to(point(r, r), px(0.), false, true, point(c.x + r, c.y));
    path.build().ok()
}

/// The row's lanes and its commit's dot.
fn row_graph(row: &GraphRow, panel: Hsla) -> impl IntoElement {
    let lanes = strokes(row);
    let width = LANE * (row.input.len().max(row.output.len()).max(1) as f32 + 1.);
    let head = row.commit.refs.iter().any(|r| r.kind == "head");
    let radius = if head { 4.5 } else { 3.5 };
    let (dot, color, pushed) = (x(row.lane), row.color, row.commit.pushed);
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, (), window: &mut Window, _| {
            let at = |x: f32, y: f32| bounds.origin + point(px(x), px(y));
            for stroke in &lanes {
                let mut path = PathBuilder::stroke(px(1.5));
                if stroke.dashed {
                    path = path.dash_array(&[px(2.), px(2.5)]);
                }
                for seg in &stroke.segs {
                    match *seg {
                        Seg::Move(x, y) => path.move_to(at(x, y)),
                        Seg::Line(x, y) => path.line_to(at(x, y)),
                        Seg::Arc(r, sweep, x, y) => {
                            path.arc_to(point(px(r), px(r)), px(0.), false, sweep, at(x, y));
                        }
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, stroke.color);
                }
            }
            let center = at(dot, ROW / 2.);
            if pushed {
                if let Some(path) = circle_path(center, radius, None) {
                    window.paint_path(path, color);
                }
            } else {
                if let Some(path) = circle_path(center, radius, None) {
                    window.paint_path(path, panel);
                }
                if let Some(path) = circle_path(center, radius, Some(1.5)) {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .w(px(width))
    .h(px(ROW))
    .flex_none()
}

/// A reply's request, cut to fit a menu item.
fn shorten(text: &str) -> String {
    if text.chars().count() > 48 {
        format!("{}…", text.chars().take(47).collect::<String>())
    } else {
        text.to_string()
    }
}

/// "Nov 8, 2026, 10:21 PM", in local time.
fn format_date(secs: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(secs, 0).single() {
        Some(d) => d.format("%b %-d, %Y, %-I:%M %p").to_string(),
        None => String::new(),
    }
}

/// The undo menu's rows: one per reply the commit took in.
fn undo_items(view: &Context<ChangesView>, commit: &GitCommit, thread: &str) -> Vec<MenuItem> {
    let entity = view.entity().downgrade();
    commit
        .replies
        .iter()
        .map(|reply| {
            let label = if reply.prompt.is_empty() {
                "Undo reply".to_string()
            } else {
                format!("Undo “{}”", shorten(&reply.prompt))
            };
            let (entity, thread) = (entity.clone(), thread.to_string());
            let (message, prompt) = (reply.message_id.clone(), reply.prompt.clone());
            MenuItem::new(label, move |window, cx| {
                let (thread, message, prompt) = (thread.clone(), message.clone(), prompt.clone());
                let _ = entity.update(cx, |v, cx| {
                    v.undo_reply(thread, message, prompt, window, cx);
                });
            })
            .icon("arrow-u-up-left")
            .danger()
        })
        .collect()
}

pub fn section(
    view: &ChangesView,
    live: &[Arc<Thread>],
    archived: &[Arc<Thread>],
    t: &Theme,
    cx: &mut Context<ChangesView>,
) -> AnyElement {
    if let Some(e) = &view.log_error {
        return div()
            .id("graph-error")
            .role(Role::Alert)
            .px(px(12.))
            .text_size(px(FS_MD))
            .text_color(t.danger)
            .child(e.clone())
            .into_any_element();
    }
    let rows = layout(&view.commits, t);
    if rows.is_empty() {
        return div()
            .px(px(12.))
            .text_size(px(FS_MD))
            .text_color(t.text_muted)
            .child("No commits")
            .into_any_element();
    }
    let live_by_id: HashMap<&str, &Thread> = live
        .iter()
        .map(|th| (th.id.as_str(), th.as_ref()))
        .collect();
    let archived_by_id: HashMap<&str, &Thread> = archived
        .iter()
        .map(|th| (th.id.as_str(), th.as_ref()))
        .collect();
    let hover = t.bg_hover;
    let mut list = div()
        .id("commit-history")
        .role(Role::List)
        .aria_label("Commit history")
        .flex()
        .flex_col();
    for row in &rows {
        let commit = row.commit;
        let head = commit.refs.iter().find(|r| r.kind == "head");
        // The branch is named once, at its commit; a remote only where it is somewhere else.
        let remotes: Vec<&str> = if head.is_some() {
            Vec::new()
        } else {
            commit
                .refs
                .iter()
                .filter(|r| r.kind == "remote" && !r.name.ends_with("/HEAD"))
                .map(|r| r.name.as_str())
                .collect()
        };
        let thread = commit
            .thread_id
            .as_deref()
            .and_then(|id| live_by_id.get(id).or(archived_by_id.get(id)).copied());
        let is_live = thread.is_some_and(|th| live_by_id.contains_key(th.id.as_str()));
        // Only a live thread's replies can be undone; an archived one is restored first.
        let undoable = is_live && !commit.replies.is_empty();
        let menu_open = view.menu_for(&commit.hash);
        let group = SharedString::from(format!("commit:{}", commit.hash));
        let title = format!(
            "{}\n{}, {}\n{}{}",
            commit.subject,
            commit.author,
            format_date(commit.time),
            commit.hash.chars().take(8).collect::<String>(),
            if commit.pushed { "" } else { "\nNot pushed" }
        );
        let mark = thread.map(|th| {
            let logo = agent_logo(th.agent, th.model.as_deref(), 12.);
            if is_live {
                let label = format!("Committed from {}", th.title);
                let id = th.id.clone();
                let workspace = view.workspace();
                button(
                    SharedString::from(format!("from:{}", commit.hash)),
                    label.clone(),
                )
                .size(px(16.))
                .flex_none()
                .justify_center()
                .rounded(px(radius_sm()))
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip(label))
                .child(logo)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let id = id.clone();
                    if let Some(w) = workspace.upgrade() {
                        w.update(cx, |w, cx| w.open_thread(&id, cx));
                    }
                })
                .into_any_element()
            } else {
                let label = format!("Committed from {} (archived)", th.title);
                div()
                    .id(SharedString::from(format!("from:{}", commit.hash)))
                    .role(Role::Image)
                    .aria_label(label.clone())
                    .tooltip(tooltip(label))
                    .flex()
                    .flex_none()
                    .child(logo)
                    .into_any_element()
            }
        });
        let thread_id = thread.map(|th| th.id.clone());
        let undo = undoable.then(|| {
            let (hash, thread) = (commit.hash.clone(), thread_id.clone().unwrap_or_default());
            let replies = commit.clone();
            button(SharedString::from(format!("undo:{hash}")), "Undo a reply")
                .size(px(16.))
                .flex_none()
                .justify_center()
                .rounded(px(radius_sm()))
                .text_color(t.text_muted)
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip("Undo a reply"))
                .aria_expanded(menu_open)
                .when(!menu_open, |b| {
                    b.opacity(0.)
                        .group_hover(group.clone(), |s| s.opacity(1.))
                        .focus(|s| s.opacity(1.))
                })
                .child(crate::ui::icon("arrow-u-up-left", 12.))
                .on_click(cx.listener(move |v, e: &gpui::ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if v.just_dismissed(e) {
                        return;
                    }
                    let items = undo_items(cx, &replies, &thread);
                    let at = e.position() + point(px(-8.), px(12.));
                    v.open_menu("Undo a reply", items, at, Some(hash.clone()), window, cx);
                }))
        });
        let context = undoable.then(|| (commit.clone(), thread_id.clone().unwrap_or_default()));
        let refs = (head.is_some() || !remotes.is_empty()).then(|| {
            div()
                .max_w(px(120.))
                .flex_none()
                .truncate()
                .text_size(px(FS_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if head.is_some() { t.accent } else { row.color })
                .child(match head {
                    Some(h) => h.name.clone(),
                    None => remotes.join(", "),
                })
        });
        list = list.child(
            div()
                .id(group.clone())
                .group(group.clone())
                .role(Role::ListItem)
                .aria_label(commit.subject.clone())
                .h(px(ROW))
                .flex()
                .items_center()
                .gap(px(6.))
                .pr(px(8.))
                .pl(px(4.))
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip(title))
                .child(row_graph(row, t.bg_panel))
                .children(mark)
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(FS_MD))
                        .when(head.is_some(), |d| d.font_weight(FontWeight::SEMIBOLD))
                        .child(commit.subject.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(FS_SM))
                        .text_color(t.text_subtle)
                        .child(commit.author.clone()),
                )
                .children(undo)
                .children(refs)
                .when_some(context, |d, (commit, thread)| {
                    d.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |v, e: &MouseDownEvent, window, cx| {
                            let items = undo_items(cx, &commit, &thread);
                            v.open_menu(
                                "Undo a reply",
                                items,
                                e.position,
                                Some(commit.hash.clone()),
                                window,
                                cx,
                            );
                        }),
                    )
                }),
        );
    }
    if view.more {
        list = list.child(
            div().h(px(ROW)).flex().items_center().px(px(12.)).child(
                button("load-more", "Load more")
                    .text_size(px(FS_MD))
                    .text_color(t.accent_text)
                    .hover(gpui::Styled::underline)
                    .child("Load more")
                    .on_click(cx.listener(|v, _, _, cx| v.load_more(cx))),
            ),
        );
    }
    let error: Option<Div> = view.undo_error.clone().map(|e| {
        div()
            .px(px(12.))
            .pb(px(4.))
            .text_size(px(FS_SM))
            .text_color(t.danger)
            .child(e)
    });
    div()
        .children(error.map(|e| e.id("undo-error").role(Role::Alert)))
        .child(list)
        .into_any_element()
}
