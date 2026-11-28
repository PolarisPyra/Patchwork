//! The controls every settings pane is drawn with: rows, headings, switches, segmented choices, steppers, alerts and chips.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;
use gpui::{AnyElement, Div, FontWeight, Hsla, Role, SharedString, Stateful, div, hsla, prelude::*, px};
use patchwork_design_components::menu::MenuItem;
use crate::settings::SettingsDialog;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{font_mono, fs_md, fs_sm, fs_xs, radius, radius_sm, theme, Theme};
use super::found::anchored;
use patchwork_design_components::{BtnKind, btn, button};
use patchwork_design_icons::icon;
use patchwork_model::chats::AccessMode;

/// The access modes a chat can start with, and their names.
pub const ACCESS_MODES: [(AccessMode, &str); 4] = [
    (AccessMode::Supervised, "Supervised"),
    (AccessMode::AutoAcceptEdits, "Auto-accept edits"),
    (AccessMode::Auto, "Auto"),
    (AccessMode::FullAccess, "Full access"),
];

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

/// One setting in a section's card: its name on the left, and the control the caller adds on
/// the right.
pub fn token_row(title: impl Into<SharedString>, t: &Theme) -> Stateful<Div> {
    let title = title.into();
    let row = div().id(SharedString::from(format!("row-{title}")));
    anchored(row, &title)
        .role(Role::Group)
        .aria_label(title.clone())
        .flex()
        .items_center()
        .gap(px(24.))
        .min_h(px(48.))
        .px(px(16.))
        .py(px(10.))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_size(px(fs_md()))
                .text_color(t.text)
                .child(title),
        )
}

/// A heading over a run of rows, as a section's heading, for a pane that lays out its own boxes.
pub fn section_heading(text: impl Into<SharedString>, first: bool, t: &Theme) -> Stateful<Div> {
    let text = text.into();
    let heading = div().id(SharedString::from(format!("heading-{text}")));
    anchored(heading, &text)
        .when(!first, |d| d.mt(px(24.)))
        .mb(px(8.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text)
        .child(text)
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
        .aria_toggled(on.into())
        .tab_index(0)
        .focus_visible(patchwork_design_components::focus_ring)
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

/// A detail page's heading: Back, then the item's `logo` and `title`. Back runs `back`.
pub fn detail_header(
    id: &str,
    logo: impl IntoElement,
    title: impl Into<SharedString>,
    back: impl Fn(&mut SettingsDialog, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            patchwork_design_components::icon_button(
                format!("{id}-back"),
                "Back",
                IconName::CaretLeft,
                16.,
                cx,
            )
            .tooltip(patchwork_design_components::tooltip("Back"))
            .on_click(cx.listener(move |d, _, _, cx| back(d, cx))),
        )
        .child(logo)
        .child(
            div()
                .id(SharedString::from(format!("{id}-title")))
                .role(Role::Heading)
                .text_size(px(patchwork_design_theme::fs_lg()))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
}

/// A search `field` at the right of a toolbar, with the glass at its left.
pub fn search_box(field: Div, t: &Theme) -> Div {
    div()
        .relative()
        .flex()
        .items_center()
        .ml_auto()
        .child(field.w_full().pl(px(26.)))
        .child(
            div()
                .absolute()
                .left(px(8.))
                .text_color(t.text_muted)
                .child(icon(IconName::MagnifyingGlass, 13.)),
        )
}

/// What a list shows when nothing is in it.
pub fn empty(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .py(px(24.))
        .flex()
        .justify_center()
        .text_color(t.text_muted)
        .child(text.into())
}

/// The row of buttons under a form, set off by a rule.
pub fn form_footer(t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .border_t_1()
        .border_color(t.border)
        .pt(px(12.))
}

/// A form's Remove button: once `confirm` is set it reads `sure`, in the danger color.
///
/// A click runs `click` with `confirm`, so the first click can ask and the second remove.
pub fn remove_button(
    id: &'static str,
    sure: &'static str,
    confirm: bool,
    busy: bool,
    t: &Theme,
    click: impl Fn(&mut SettingsDialog, bool, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let label = if confirm { sure } else { "Remove" };
    btn(id, label, BtnKind::Plain, busy, t)
        .when(confirm, |b| b.text_color(t.danger))
        .child(label)
        .when(!busy, |b| {
            b.on_click(cx.listener(move |d, _, _, cx| click(d, confirm, cx)))
        })
}

/// A radio button named `label`, reachable by keyboard, marked when `checked`.
pub fn radio(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    checked: bool,
) -> Stateful<Div> {
    div()
        .id(id.into())
        .role(Role::RadioButton)
        .aria_label(label.into())
        .aria_toggled(checked.into())
        .tab_index(0)
        .focus_visible(patchwork_design_components::focus_ring)
        .cursor_pointer()
}

/// A row titled `title` with a switch named after it; a click runs `flip` with the new state.
pub fn switch_row(
    id: impl Into<SharedString>,
    title: &'static str,
    on: bool,
    flip: impl Fn(&mut SettingsDialog, bool, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let t = *theme(cx);
    token_row(title, &t)
        .child(switch(id, title, on, &t).on_click(cx.listener(move |d, _, _, cx| flip(d, !on, cx))))
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
            radio(
                SharedString::from(format!("{id}-{i}")),
                text.clone(),
                checked,
            )
            .flex()
            .h(px(24.))
            .items_center()
            .justify_center()
            .px(px(16.))
            .whitespace_nowrap()
            .text_size(px(fs_sm()))
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

/// A grid of tiles, such as the providers to add or the chat apps to answer on.
pub fn tile_grid(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .role(Role::Group)
        .aria_label(label)
        .grid()
        .grid_cols(6)
        .gap(px(8.))
        .w_full()
        .py(px(10.))
}

/// One tile of a grid: a mark over a name.
pub fn tile(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, strong, fg) = (t.bg_hover, t.border_strong, t.text);
    patchwork_design_components::button(id.into(), label)
        .relative()
        .h(px(72.))
        .min_w_0()
        .flex_col()
        .justify_center()
        .gap(px(8.))
        .rounded(px(radius()))
        .border_1()
        .px(px(8.))
        .text_size(px(fs_sm()))
        .hover(move |s| s.border_color(strong).bg(hover).text_color(fg))
}

/// A `<select>`: a bordered box with what is picked and a caret, which opens a menu of the choices.
pub fn select_box(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    current: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (strong, fg) = (t.border_strong, t.text);
    button(id.into(), label)
        .aria_expanded(open)
        .flex_none()
        .h(px(32.))
        .min_w(px(144.))
        .max_w(px(240.))
        .justify_between()
        .gap(px(8.))
        .rounded(px(radius()))
        .border_1()
        .border_color(if open { t.border_strong } else { t.border })
        .bg(t.bg_input)
        .px(px(10.))
        .text_size(px(fs_md()))
        .text_color(t.text)
        .hover(move |s| s.border_color(strong).text_color(fg))
        .child(div().min_w_0().truncate().child(current.into()))
        .child(icon(IconName::CaretDown, 12.))
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
    if checked {
        item.icon(IconName::Check)
    } else {
        item
    }
}

/// A click handler that opens the dropdown `key`'s menu of `options`, or closes it when open.
///
/// The option equal to `chosen` is marked, and picking one runs `set` with its value; the menu
/// closes itself on a pick.
pub fn choices<T: Clone + PartialEq + 'static>(
    key: impl Into<SharedString>,
    label: impl Into<SharedString>,
    options: Vec<(T, SharedString)>,
    chosen: T,
    set: impl Fn(&mut SettingsDialog, T, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    let (key, label, set) = (key.into(), label.into(), Rc::new(set));
    cx.listener(move |d, e: &gpui::ClickEvent, window, cx| {
        let weak = cx.entity().downgrade();
        let items = options
            .iter()
            .map(|(value, name)| {
                let (value, set) = (value.clone(), Rc::clone(&set));
                choice(weak.clone(), name.clone(), value == chosen, move |d, cx| {
                    set(d, value.clone(), cx);
                })
            })
            .collect();
        d.toggle_menu(key.clone(), label.clone(), items, e.position(), window, cx);
    })
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
    let step = |id: &'static str, name: &'static str, glyph: IconName, to: Option<usize>| {
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
        .child(step(
            "less-often",
            "Less often",
            IconName::Minus,
            at.checked_sub(1),
        ))
        .child(
            div()
                .w(px(56.))
                .border_x_1()
                .border_color(t.border_strong)
                .text_center()
                .text_size(px(fs_sm()))
                .child(shown),
        )
        .child(step(
            "more-often",
            "More often",
            IconName::Plus,
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
        .text_size(px(fs_sm()))
        .text_color(color)
        .child(text.into())
}

/// A field with its label above it.
pub fn labeled(label: &'static str, field: Div, muted: Hsla) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(fs_sm()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(label),
        )
        .child(field)
}

/// What a pane that saves secrets says when neither the system keychain nor a password locks
/// them: they need a password set, wait for the password, or cannot be unlocked this run.
pub fn vault_notice(id: impl Into<SharedString>, t: &Theme) -> Option<Stateful<Div>> {
    use patchwork_client::local::core::vault::{Method, Status, status};
    match status() {
        Status::Locked(why) => Some(alert(
            id,
            format!("Saved secrets are locked, and new ones cannot be saved: {why}"),
            t.danger,
        )),
        Status::Open(Method::File) => Some(alert(
            id,
            "No system keychain: set a password in Settings, under Vault, to lock saved secrets",
            t.warning,
        )),
        Status::Unset => Some(alert(
            id,
            "No system keychain: set a password in Settings, under Vault, to save secrets",
            t.danger,
        )),
        Status::Waiting => Some(alert(
            id,
            "Saved secrets are locked until the password is entered in Settings, under Vault",
            t.danger,
        )),
        Status::Open(Method::Keychain | Method::Password) | Status::Closed => None,
    }
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
        .aria_toggled(on.into())
        .flex_none()
        .h(px(28.))
        .gap(px(6.))
        .px(px(10.))
        .rounded_full()
        .border_1()
        .whitespace_nowrap()
        .text_size(px(fs_sm()))
        .text_color(t.text)
        .map(|b| {
            if on {
                b.border_color(t.accent).bg(t.bg_accent_soft)
            } else {
                b.border_color(t.border).hover(move |s| s.bg(hover))
            }
        })
}

/// The rows of a list in one card, a line between each.
pub fn list_box(rows: Vec<AnyElement>, t: &Theme) -> Div {
    super::section::card(rows, t)
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

/// How a folder is laid out, under an "Example" heading.
pub fn example(text: &'static str, t: &Theme) -> Div {
    div()
        .child(
            div()
                .mt(px(20.))
                .mb(px(6.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Example"),
        )
        .child(
            div()
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_editor)
                .px(px(12.))
                .py(px(10.))
                .font_family(font_mono())
                .text_size(px(fs_xs()))
                .line_height(gpui::relative(1.6))
                .text_color(t.text_muted)
                .children(
                    text.lines()
                        .map(|line| div().whitespace_nowrap().child(line.to_string())),
                ),
        )
}
