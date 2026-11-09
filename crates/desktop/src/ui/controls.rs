//! The controls every view is built from: icons, buttons, the spinner, scrollbars, popups, menus' rows, checkboxes, tooltips and badges.

// Rust guideline compliant 2026-10-09

use std::f32::consts::TAU;
use std::sync::OnceLock;
use std::time::Instant;
use gpui::{
    Anchor, App, BoxShadow, Div, ElementId, Hsla, IntoElement, PathBuilder, Role, SharedString,
    Stateful, StyleRefinement, Toggled, Window, anchored, canvas, deferred, div, point, prelude::*,
    px,
};
use crate::ui::theme::{FS_MD, RADIUS, RADIUS_SM, Theme, theme};

/// A Phosphor icon by its kebab-case name, as `@phosphor-icons/react` draws it at `size`.
pub fn icon(name: &str, size: f32) -> Icon {
    mono_svg(SharedString::from(format!("icons/{name}.svg")), size)
}

/// A one-colour SVG asset, `size` square.
pub(super) fn mono_svg(path: SharedString, size: f32) -> Icon {
    Icon {
        path,
        style: StyleRefinement::default(),
    }
    .size(px(size))
    .flex_none()
}

/// A one-colour SVG drawn in the text colour around it, as `currentColor` is in the web UI, so it
/// follows its button's hover colour. A colour set on the icon itself wins.
#[derive(IntoElement)]
pub struct Icon {
    pub(super) path: SharedString,
    pub(super) style: StyleRefinement,
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Icon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let own = self.style.text.color;
        let path = self.path;
        // The colour around it is known while laying out; painting no longer has it.
        let mut drawn = canvas(
            move |_, window, _| own.unwrap_or_else(|| window.text_style().color),
            move |bounds, color, window: &mut Window, cx| {
                if let Err(e) = window.paint_svg(
                    bounds,
                    path.clone(),
                    None,
                    gpui::TransformationMatrix::default(),
                    color,
                    cx,
                ) {
                    log::warn!(path:%, error:% = e; "could not draw {{path}}: {{error}}");
                }
            },
        );
        *drawn.style() = self.style;
        drawn
    }
}

/// `:focus-visible`: the 2px outline a control shows while the keyboard, not the mouse, focused it.
/// Drawn just inside its edge, as a shadow outside would fill a control with no background.
pub fn focus_ring(s: StyleRefinement) -> StyleRefinement {
    s.shadow(vec![BoxShadow {
        color: crate::ui::theme::border_focus(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: true,
    }])
}

/// A focusable button: Enter and Space click it, and assistive tech reads `label`.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .flex()
        .items_center()
}

/// `.btn-icon`: a 28px square icon button, muted until hovered.
pub fn icon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    name: &str,
    size: f32,
    cx: &App,
) -> Stateful<Div> {
    let t = theme(cx);
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .size(px(28.))
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, size))
}

/// The steady spinner: a faint ring with a quarter arc running around
/// it once every 0.8s. Every arc's lap counts from the app's start, so all spinners turn in step,
/// however late each one appears. Nothing rotates, so the ring keeps its pixels.
pub fn spinner(size: f32, color: Hsla) -> impl IntoElement {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    let ring = canvas(
        |_, _, _| {},
        move |bounds, (), window: &mut Window, _| {
            window.request_animation_frame();
            let scale = size / 24.;
            let center = bounds.center();
            let radius = 9. * scale;
            let width = px(3. * scale);
            let at = |turn: f32| {
                let angle = turn * TAU;
                point(
                    center.x + px(radius * angle.cos()),
                    center.y + px(radius * angle.sin()),
                )
            };
            let mut ring = PathBuilder::stroke(width);
            ring.move_to(at(0.));
            ring.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(0.5));
            ring.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(0.));
            if let Ok(path) = ring.build() {
                window.paint_path(path, color.opacity(0.25));
            }
            let lap = (start.elapsed().as_secs_f32() / 0.8).fract();
            let (from, to) = (lap, lap + 0.25);
            let mut arc = PathBuilder::stroke(width);
            arc.move_to(at(from));
            arc.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(to));
            if let Ok(path) = arc.build() {
                window.paint_path(path, color);
            }
            // The arc's round caps.
            for end in [from, to] {
                let c = at(end);
                let r = px(1.5 * scale);
                let mut cap = PathBuilder::fill();
                cap.move_to(point(c.x + r, c.y));
                cap.arc_to(point(r, r), px(0.), false, true, point(c.x - r, c.y));
                cap.arc_to(point(r, r), px(0.), false, true, point(c.x + r, c.y));
                if let Ok(path) = cap.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(size))
    .flex_none();
    div()
        .id("spinner")
        .role(Role::ProgressIndicator)
        .aria_label("Loading")
        .flex_none()
        .child(ring)
}

/// The tile a project is drawn with (`ProjectTile`): its first two letters on a colour picked from
/// its name, so each project keeps its colour.
pub fn project_tile(name: &str, size: f32, cx: &App) -> Div {
    let t = theme(cx);
    let palette = [
        t.accent,
        t.syn_string,
        t.syn_property,
        t.syn_keyword,
        t.syn_type,
    ];
    let hash = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let initials = name
        .trim()
        .chars()
        .take(2)
        .collect::<String>()
        .to_uppercase();
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .bg(palette[hash as usize % palette.len()])
        .text_color(t.text_on_accent)
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::BOLD)
        .child(initials)
}

/// Whether the element `handle` tracks scrolls, as last laid out. When layout changes that, the
/// view draws once more, so a scrollbar and the room kept for it follow content that grew or shrank.
pub fn overflows<V: 'static>(
    handle: &gpui::ScrollHandle,
    window: &mut Window,
    cx: &mut gpui::Context<V>,
) -> bool {
    let seen = (handle.bounds().size.height, handle.max_offset().y);
    let handle = handle.clone();
    cx.on_next_frame(window, move |_, _, cx| {
        if (handle.bounds().size.height, handle.max_offset().y) != seen {
            cx.notify();
        }
    });
    seen.1 > px(0.)
}

/// A scrollbar thumb for the element `handle` tracks, `width` wide with `inset` of clear space
/// around it, as the web UI's `::-webkit-scrollbar-thumb` draws one. None when nothing scrolls.
pub fn scrollbar(handle: &gpui::ScrollHandle, width: f32, inset: f32, color: Hsla) -> Option<Div> {
    let view = f32::from(handle.bounds().size.height);
    let max = f32::from(handle.max_offset().y);
    thumb(
        view,
        max,
        -f32::from(handle.offset().y),
        width,
        inset,
        color,
    )
}

/// `scrollbar` for a `list`, sized from the rows it has measured so far.
pub fn list_scrollbar(state: &gpui::ListState, width: f32, inset: f32, color: Hsla) -> Option<Div> {
    let view = f32::from(state.viewport_bounds().size.height);
    let max = f32::from(state.max_offset_for_scrollbar().y);
    let scrolled = -f32::from(state.scroll_px_offset_for_scrollbar().y);
    thumb(view, max, scrolled, width, inset, color)
}

pub(super) fn thumb(
    view: f32,
    max: f32,
    scrolled: f32,
    width: f32,
    inset: f32,
    color: Hsla,
) -> Option<Div> {
    if max <= 0. || view <= 0. {
        return None;
    }
    let scrolled = scrolled.clamp(0., max);
    let thumb = (view * view / (view + max)).max(20.);
    let top = scrolled / max * (view - thumb);
    Some(
        div()
            .absolute()
            .top(px(top))
            .right_0()
            .w(px(width))
            .h(px(thumb))
            .p(px(inset))
            .child(div().size_full().rounded(px(width / 2.)).bg(color)),
    )
}

/// `Dropdown`'s menu: a raised box with a strong border and the popup shadow.
pub fn popup(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(200.))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .p(px(4.))
        .text_size(px(FS_MD))
        .shadow(popup_shadow(t))
}

/// `--shadow-popup`: `0 6px 24px`.
pub fn popup_shadow(t: &Theme) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: t.shadow,
        offset: point(px(0.), px(6.)),
        blur_radius: px(24.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Lays `content` over the window with its bottom-left corner 4px above the top-left of the
/// element this is a child of, which must be `relative`: a menu that opens up from its chip.
pub fn above(content: impl IntoElement) -> Div {
    div().absolute().top_0().left_0().child(
        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .offset(point(px(0.), px(-4.)))
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(1),
    )
}

/// A menu row that picks one value of several (`role="menuitemradio"`): the picked one shaded, and
/// the one under the keyboard highlighted as the pointer would.
pub fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    highlighted: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    div()
        .id(id)
        .role(Role::MenuItemRadio)
        .aria_label(label)
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w(px(160.))
        .rounded(px(RADIUS_SM))
        .px(px(8.))
        .py(px(5.))
        .whitespace_nowrap()
        .text_color(t.text)
        .when(checked, |row| row.bg(t.bg_selected_muted))
        .when(highlighted && !checked, |row| row.bg(hover))
        .hover(move |s| s.bg(hover))
}

/// A checkbox and its label (`<label><input type="checkbox">…</label>`); the row toggles it.
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(label.clone())
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(13.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .map(|b| {
                    if checked {
                        b.border_color(t.accent)
                            .bg(t.accent)
                            .text_color(t.text_on_accent)
                            .child(icon("check", 10.))
                    } else {
                        b.border_color(t.border_strong).bg(t.bg_editor)
                    }
                }),
        )
        .child(label)
}

/// What the web UI's `title` attribute shows: a small box with `text` beside the pointer.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView {
    let text = text.into();
    move |_, cx| {
        let text = text.clone();
        cx.new(|_| Tooltip(text)).into()
    }
}

pub(super) struct Tooltip(pub(super) SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .max_w(px(360.))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .px(px(6.))
            .py(px(2.))
            .text_size(px(crate::ui::theme::FS_SM))
            .line_height(px(16.))
            .text_color(t.text)
            .shadow(popup_shadow(t))
            .children(self.0.split('\n').map(|line| div().child(line.to_string())))
    }
}

/// `color-mix(in srgb, a pct%, b)`.
pub fn mix(a: Hsla, b: Hsla, pct: f32) -> Hsla {
    let (a, b) = (gpui::Rgba::from(a), gpui::Rgba::from(b));
    let p = pct / 100.;
    gpui::Rgba {
        r: a.r * p + b.r * (1. - p),
        g: a.g * p + b.g * (1. - p),
        b: a.b * p + b.b * (1. - p),
        a: a.a * p + b.a * (1. - p),
    }
    .into()
}

/// `.btn`'s variants: the plain button and the coloured ones.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Plain,
    Primary,
    Success,
}

/// `.btn`: a 28px button with a border, its label at `--fs-md`. A disabled one is faded and does
/// not react to the pointer; its owner leaves out the click handler.
pub fn btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: BtnKind,
    disabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (bg, border, fg, hover) = match kind {
        BtnKind::Plain => (t.bg_editor, t.border_strong, t.text, t.bg_hover),
        BtnKind::Primary => (t.accent, t.accent, t.text_on_accent, t.accent_hover),
        BtnKind::Success => (t.success, t.success, t.text_on_accent, darken(t.success)),
    };
    let base = button(id, label)
        .flex_none()
        .h(px(28.))
        .justify_center()
        .gap(px(6.))
        .px(px(14.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(FS_MD))
        .whitespace_nowrap();
    if disabled {
        base.opacity(0.5).cursor_default()
    } else {
        base.hover(move |s| s.bg(hover))
    }
}

/// `filter: brightness(0.92)`.
pub(super) fn darken(c: Hsla) -> Hsla {
    Hsla { l: c.l * 0.92, ..c }
}

/// `.badge`: a small count or tag.
pub fn badge(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(20.))
        .flex()
        .items_center()
        .px(px(6.))
        .rounded(px(RADIUS_SM))
        .bg(t.bg_selected_muted)
        .text_size(px(crate::ui::theme::FS_XS))
        .text_color(t.text_muted)
        .child(text.into())
}
