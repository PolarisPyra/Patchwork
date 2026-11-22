//! The card a call of the quilt tool shows in the transcript: a shimmer with the quilt's
//! title and the lines written so far while the agent writes it, then its picture and name,
//! which open it. A reply that changes one quilt several times shows one card, its last.

// Rust guideline compliant 2026-10-09

use std::sync::OnceLock;
use std::time::Instant;

use gpui::{
    App, Bounds, Corners, Div, Hsla, ObjectFit, Pixels, Role, SharedString, Window, canvas, div,
    fill, img, linear_color_stop, linear_gradient, point, prelude::*, px,
};
use patchwork_core::quilts::{QuiltKind, is_quilt_call, saved_by};
use patchwork_core::chats::{Message, MessageStatus, StepStatus, ToolCall};

use patchwork_pages::quilts::kind_icon;
use crate::chat::transcript::turn_contents::is_running;
use patchwork_ui::theme::{Theme, fs_md, fs_xs, radius_lg, radius_sm};

use patchwork_ui::{button, icon, next_step};
use crate::workspace::Workspace;

/// The card's height, fixed so the transcript never measures it again.
const HEIGHT: f32 = 60.;

/// How bright the shimmer's band is at its middle.
const GLOW: f32 = 0.07;

/// How long the shimmer takes to cross the card and rest, in seconds.
const SWEEP: f32 = 1.8;

/// The card for the quilt-tool call `tool` in reply `m`.
pub(super) fn quilt_card(
    w: &Workspace,
    m: &Message,
    tool: &ToolCall,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let chat_id = w.chat.as_ref().map(|t| t.id.as_str()).unwrap_or_default();
    let running = m.status == MessageStatus::Streaming && is_running(tool);
    let failed = tool.status == StepStatus::Error;
    let saved = (!running && !failed)
        .then(|| saved_by(&w.quilt_list, chat_id, tool))
        .flatten();
    // The reply's later calls that saved this quilt again show it instead.
    let later: Vec<&ToolCall> = m
        .tools
        .iter()
        .skip_while(|other| other.id != tool.id)
        .skip(1)
        .filter(|other| is_quilt_call(other))
        .collect();
    if let Some((quilt, _)) = saved
        && later.iter().any(|other| {
            saved_by(&w.quilt_list, chat_id, other).is_some_and(|(a, _)| a.id == quilt.id)
        })
    {
        return div();
    }
    let changing = matches!(tool.summary.as_str(), "update" | "rewrite");
    let written = tool
        .lines
        .map(|l| format!(" \u{b7} {} lines", l.added))
        .unwrap_or_default();
    let (glyph, title, detail) = match saved {
        Some((quilt, n)) => (
            kind_icon(quilt.kind),
            quilt.title.clone(),
            Some(format!("{} \u{b7} Version {n}", quilt.kind.label())),
        ),
        None if running => {
            // Until its input names them, the kind is not known, and most calls make one.
            let kind = tool.kind.as_deref().and_then(QuiltKind::parse);
            let noun = kind.map_or("a quilt", QuiltKind::noun);
            let verb = if changing { "Changing" } else { "Making" };
            (
                kind.map_or("sparkle", kind_icon),
                tool.title
                    .clone()
                    .unwrap_or_else(|| format!("{verb} {noun}")),
                Some(match kind {
                    Some(_) if tool.title.is_some() => format!("{verb} {noun}{written}"),
                    _ => format!("{}{written}", if changing { "Changing" } else { "Writing" }),
                }),
            )
        }
        None if failed => ("x-circle", "The quilt was not saved".to_owned(), None),
        None => ("sparkle", "Quilt".to_owned(), None),
    };
    let picture = saved.and_then(|(quilt, n)| {
        if !quilt.kind.drawn() {
            return None;
        }
        let (id, thumbs) = (quilt.id.clone(), w.thumbs.clone());
        let taken = thumbs
            .read(cx)
            .get(&id, n, t.is_dark())
            .map(Option::<&std::path::PathBuf>::cloned);
        if taken.is_none() {
            thumbs.update(cx, |thumbs, cx| thumbs.want(&id, n, t.is_dark(), cx));
        }
        taken.flatten()
    });
    let fg = if failed { t.danger } else { t.text };
    let body = div()
        .relative()
        .h(px(HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(12.))
        .overflow_hidden()
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_elevated)
        .child(
            div()
                .h(px(40.))
                .w(px(if picture.is_some() { 64. } else { 40. }))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .rounded(px(radius_sm()))
                .bg(t.bg_hover)
                .text_color(if failed { t.danger } else { t.text_muted })
                .child(match picture {
                    // The picture keeps the frame's corners, which clipping alone does not.
                    Some(path) => img(path)
                        .size_full()
                        .rounded(px(radius_sm()))
                        .object_fit(ObjectFit::Cover)
                        .into_any_element(),
                    None => icon(glyph, 18.).into_any_element(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(fs_md()))
                        .text_color(fg)
                        .truncate()
                        .child(title.clone()),
                )
                .children(detail.clone().map(|d| {
                    div()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .truncate()
                        .child(d)
                })),
        );
    let card = match saved {
        _ if running => body
            .id(SharedString::from(format!("quilt-{}", tool.id)))
            .role(Role::Status)
            .aria_label(match &detail {
                Some(d) => format!("{title}, {d}"),
                None => title,
            })
            .child(shimmer(t.text)),
        Some((quilt, n)) => {
            let id = quilt.id.clone();
            let hover = t.bg_hover;
            button(
                SharedString::from(format!("quilt-{}", tool.id)),
                format!("Open {title}"),
            )
            .rounded(px(radius_lg()))
            .child(
                body.w_full().hover(move |s| s.bg(hover)).child(
                    div()
                        .flex_none()
                        .text_color(t.text_muted)
                        .child(icon("caret-right", 14.)),
                ),
            )
            .on_click(cx.listener(move |this: &mut Workspace, _, window, cx| {
                this.open_quilt_beside(id.clone(), n, window, cx);
            }))
        }
        None => body
            .id(SharedString::from(format!("quilt-{}", tool.id)))
            .aria_label(title),
    };
    div().pb(px(8.)).child(card)
}

/// A soft band of light sweeping across the card, in step with every other card's.
fn shimmer(color: Hsla) -> impl IntoElement {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    // The curve of the card's corners inside its border.
    let inner = px(radius_lg() - 1.);
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<gpui::Pixels>, (), window: &mut Window, cx: &mut App| {
            next_step(window, cx);
            let lap = (start.elapsed().as_secs_f32() / SWEEP).fract();
            let band = bounds.size.width * 0.35;
            // The band crosses in the first 70% of the lap and rests off the card after.
            let across = (lap / 0.7).min(1.);
            let left = bounds.origin.x - band * 2. + (bounds.size.width + band * 2.) * across;
            // Each half brightens from `from` to `to` across its width.
            for (from, to, x) in [(0., GLOW, left), (GLOW, 0., left + band)] {
                // Only the part on the card is drawn, with its corners rounded to fit inside
                // the card's, since clipping keeps to a rectangle.
                let (start, end) = (x.max(bounds.left()), (x + band).min(bounds.right()));
                let curve = |gap: Pixels| (inner - gap).max(px(0.));
                let (left_curve, right_curve) =
                    (curve(start - bounds.left()), curve(bounds.right() - end));
                // A sliver too thin for its curve would show past the card's corner.
                if end - start <= left_curve + right_curve {
                    continue;
                }
                let at = |edge: Pixels| color.opacity(from + (to - from) * ((edge - x) / band));
                let half =
                    Bounds::from_corners(point(start, bounds.top()), point(end, bounds.bottom()));
                window.paint_quad(
                    fill(
                        half,
                        linear_gradient(
                            90.,
                            linear_color_stop(at(start), 0.),
                            linear_color_stop(at(end), 1.),
                        ),
                    )
                    .corner_radii(Corners {
                        top_left: left_curve,
                        bottom_left: left_curve,
                        top_right: right_curve,
                        bottom_right: right_curve,
                    }),
                );
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
