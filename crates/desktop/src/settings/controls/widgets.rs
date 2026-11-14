//! The controls every settings pane is drawn with: rows, headings, switches, segmented choices, steppers, alerts and chips.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;
use gpui::{
    AnyElement, Div, FontWeight, Hsla, Role, SharedString, Stateful, Toggled, div, hsla,
    prelude::*, px,
};
use crate::ui::menu::MenuItem;
use crate::settings::SettingsDialog;
use crate::ui::theme::{FS_SM, radius_sm, Theme};
use crate::ui::{button, icon};

/// A letter on a colour picked from the name, so each row is easy to spot.
pub fn letter_badge(name: &str, size: f32) -> Div {
    let mut hash: i32 = 0;
    for c in name.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(c));
    }
    let hue = (hash.unsigned_abs() % 360) as f32 / 360.;
    let letter = name
        .chars()
        .find(char::is_ascii_alphanumeric)
        .map_or_else(|| "?".to_string(), |c| c.to_uppercase().to_string());
    div()
        .flex_none()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.))
        .bg(hsla(hue, 0.7, 0.55, 0.18))
        .text_color(hsla(hue, 0.75, 0.65, 1.))
        .text_size(px(size * 0.45))
        .font_weight(FontWeight::SEMIBOLD)
        .child(letter)
}

/// One setting: its name and what it does, with the control on the right.
pub fn token_row(title: impl Into<SharedString>, desc: Option<&str>, t: &Theme) -> Stateful<Div> {
    let title = title.into();
    div()
        .id(SharedString::from(format!("row-{title}")))
        .role(Role::Group)
        .aria_label(title.clone())
        .flex()
        .items_center()
        .gap(px(12.))
        .border_b_1()
        .border_color(t.border)
        .py(px(10.))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .children(desc.map(|desc| {
                    div()
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(desc.to_string())
                })),
        )
}

/// A heading over a run of rows, for a pane that holds more than one kind of setting.
pub fn section_heading(text: impl Into<SharedString>, first: bool, t: &Theme) -> Div {
    div()
        .when(!first, |d| d.mt(px(16.)))
        .mb(px(2.))
        .text_size(px(FS_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_subtle)
        .child(text.into())
}

/// `.toggle`: a switch, its knob at the right when on.
pub fn switch(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    t: &Theme,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Switch)
        .aria_label(label.into())
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .tab_index(0)
        .focus_visible(crate::ui::focus_ring)
        .cursor_pointer()
        .relative()
        .flex_none()
        .w(px(34.))
        .h(px(20.))
        .rounded(px(10.))
        .border_1()
        .border_color(if on { t.accent } else { t.border_strong })
        .bg(if on { t.accent } else { t.bg_selected_muted })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 16. } else { 2. }))
                .size(px(14.))
                .rounded_full()
                .bg(t.text_on_accent)
                .shadow(vec![gpui::BoxShadow {
                    color: hsla(0., 0., 0., 0.3),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                }]),
        )
}

/// Two to five choices side by side, as the theme picker. Each choice is `(label, checked)`;
/// the caller adds the click to each with `pick`.
pub fn segmented<T: Copy + PartialEq + 'static>(
    id: &str,
    label: impl Into<SharedString>,
    value: T,
    options: &[(T, impl Into<SharedString> + Clone)],
    t: &Theme,
    pick: impl Fn(&mut SettingsDialog, T, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let pick = Rc::new(pick);
    let (hover, fg, muted) = (t.bg_hover, t.text, t.text_muted);
    div()
        .id(SharedString::from(format!("{id}-group")))
        .role(Role::RadioGroup)
        .aria_label(label.into())
        // Every choice as wide as the widest (`auto-cols-fr`).
        .grid()
        .grid_cols_min_content(options.len() as u16)
        .flex_none()
        .overflow_hidden()
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .children(options.iter().enumerate().map(|(i, (option, text))| {
            let (option, text): (T, SharedString) = (*option, text.clone().into());
            let checked = option == value;
            let pick = Rc::clone(&pick);
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .role(Role::RadioButton)
                .aria_label(text.clone())
                .aria_toggled(if checked {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .tab_index(0)
                .focus_visible(crate::ui::focus_ring)
                .cursor_pointer()
                .flex()
                .h(px(24.))
                .items_center()
                .justify_center()
                .px(px(16.))
                .whitespace_nowrap()
                .text_size(px(FS_SM))
                .map(|d| {
                    if checked {
                        d.bg(t.bg_selected).text_color(fg)
                    } else {
                        d.text_color(muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                    }
                })
                .child(text)
                .on_click(cx.listener(move |d, _, _, cx| pick(d, option, cx)))
        }))
}

/// A `<select>`: a bordered box with what is picked and a caret, which opens a menu of the choices.
pub fn select_box(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    current: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id.into(), label)
        .aria_expanded(open)
        .flex_none()
        .h(px(28.))
        .justify_between()
        .gap(px(6.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_editor)
        .pl(px(8.))
        .pr(px(6.))
        .text_size(px(FS_SM))
        .text_color(t.text)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(div().min_w_0().truncate().child(current.into()))
        .child(icon("caret-down", 12.))
}

/// A menu row that runs `pick` on the dialog, marked when it is the current choice.
pub fn choice(
    dialog: gpui::WeakEntity<SettingsDialog>,
    label: impl Into<SharedString>,
    checked: bool,
    pick: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
) -> MenuItem {
    let item = MenuItem::new(label, move |_, cx| {
        let _ = dialog.update(cx, |d, cx| pick(d, cx));
    });
    if checked { item.icon("check") } else { item }
}

/// The minutes between update checks that the stepper offers.
pub(super) const CHECK_MINUTES: [u32; 6] = [10, 20, 30, 40, 50, 60];

/// Steps through `CHECK_MINUTES` (`MinutesStepper`).
pub fn minutes_stepper(
    label: &str,
    value: u32,
    t: &Theme,
    change: impl Fn(&mut SettingsDialog, u32, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let at = CHECK_MINUTES.iter().position(|&m| m == value).unwrap_or(0);
    let change = Rc::new(change);
    let (hover, fg, muted) = (t.bg_hover, t.text, t.text_muted);
    let step = |id: &'static str, name: &'static str, glyph: &str, to: Option<usize>| {
        let change = Rc::clone(&change);
        button(id, name)
            .size(px(24.))
            .justify_center()
            .text_color(muted)
            .child(icon(glyph, 12.))
            .map(|b| match to {
                Some(to) => b
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .on_click(cx.listener(move |d, _, _, cx| change(d, CHECK_MINUTES[to], cx))),
                None => b.opacity(0.4).cursor_default(),
            })
    };
    let shown = if CHECK_MINUTES[at] == 60 {
        "1 hr".to_string()
    } else {
        format!("{} min", CHECK_MINUTES[at])
    };
    div()
        .id("minutes-stepper")
        .role(Role::Group)
        .aria_label(label.to_string())
        .flex()
        .flex_none()
        .items_center()
        .overflow_hidden()
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .child(step("less-often", "Less often", "minus", at.checked_sub(1)))
        .child(
            div()
                .w(px(56.))
                .border_x_1()
                .border_color(t.border_strong)
                .text_center()
                .text_size(px(FS_SM))
                .child(shown),
        )
        .child(step(
            "more-often",
            "More often",
            "plus",
            (at + 1 < CHECK_MINUTES.len()).then_some(at + 1),
        ))
}

/// A line in red under what failed (`role="alert"`).
pub fn alert(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    color: Hsla,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::Alert)
        .pt(px(8.))
        .text_size(px(FS_SM))
        .text_color(color)
        .child(text.into())
}

/// A filter chip, filled when it is the one showing (`chip`). The caller adds what it shows.
pub fn chip(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    on: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    button(id.into(), label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .flex_none()
        .h(px(28.))
        .gap(px(6.))
        .px(px(10.))
        .rounded_full()
        .border_1()
        .whitespace_nowrap()
        .text_size(px(FS_SM))
        .text_color(t.text)
        .map(|b| {
            if on {
                b.border_color(t.accent).bg(t.bg_accent_soft)
            } else {
                b.border_color(t.border).hover(move |s| s.bg(hover))
            }
        })
}

/// The rows of a list in one bordered box, a line between each.
pub fn list_box(rows: Vec<AnyElement>, t: &Theme) -> Div {
    let border = t.border;
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(crate::ui::theme::radius()))
        .border_1()
        .border_color(border)
        .children(rows.into_iter().enumerate().map(move |(i, row)| {
            div()
                .when(i > 0, |d| d.border_t_1().border_color(border))
                .child(row)
        }))
}

/// `~/…` for a path under the home folder.
pub fn tildify(path: &str) -> String {
    match std::env::var("HOME") {
        // Only at a folder's edge: `/home/ann` is not the start of `/home/anna`.
        Ok(home)
            if !home.is_empty()
                && path
                    .strip_prefix(&home)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')) =>
        {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
    }
}
