//! Settings → Interface: how much room rows take, whether anything moves, and the font and size
//! of the app's text. The font is picked from a list of the ones the computer has, narrowed by
//! what is typed, that opens under the font's select.

// Rust guideline compliant 2026-10-09

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, Focusable, KeyDownEvent, MouseDownEvent, Role, ScrollStrategy,
    SharedString, UniformListScrollHandle, Window, anchored, deferred, div, point, prelude::*, px,
    uniform_list,
};

use super::look::density_cards;
use super::reading::{motion_row, text_size_cards};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, section_heading, select_box, token_row};
use crate::ui::text_area::Arrow;
use crate::ui::theme::{FONT_UI, fs_md, radius_sm, theme};
use crate::ui::{icon, popup, scrollbar};

/// The family name that stands for the system's own font.
const SYSTEM_FONT: &str = ".SystemUIFont";

/// The field the list of fonts is narrowed from.
const FILTER: &str = "font-filter";

const ROW_H: f32 = 28.;
/// Eight fonts show at once.
const LIST_H: f32 = ROW_H * 8.;
const MENU_W: f32 = 280.;

const TITLE: &str = "Font";

/// What takes the keyboard: the list's filter once it opens, and the dialog once it closes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Filter,
    Dialog,
}

/// The font picker of the pane: the fonts the computer has, and whether its list is open.
#[derive(Default)]
pub(super) struct Fonts {
    all: Vec<SharedString>,
    open: bool,
    filter: String,
    /// The row of the open list the keyboard is on.
    active: usize,
    scroll: UniformListScrollHandle,
    /// What takes the keyboard when the pane is next drawn, after the list opened or closed.
    focus: Option<Focus>,
    /// The pointer is over the select, whose click opens and closes the list.
    over: bool,
}

impl Fonts {
    /// Lists the fonts the computer has, once.
    pub(super) fn load(&mut self, cx: &App) {
        if !self.all.is_empty() {
            return;
        }
        let mut names = cx.text_system().all_font_names();
        // A name with a leading dot is one of the system's private fonts.
        names.retain(|name| !name.starts_with('.'));
        names.sort_by_key(|name| name.to_lowercase());
        self.all = names.into_iter().map(SharedString::from).collect();
    }

    /// What the list offers: the family the settings keep, its name, and the family the name
    /// is drawn in.
    fn choices(&self) -> Vec<Choice> {
        let wanted = self.filter.trim().to_lowercase();
        let own = (
            None,
            SharedString::new_static("Default"),
            SharedString::new_static(FONT_UI),
        );
        let system = (
            Some(SYSTEM_FONT.to_string()),
            SharedString::new_static("System"),
            SharedString::new_static(SYSTEM_FONT),
        );
        let named = self
            .all
            .iter()
            .map(|font| (Some(font.to_string()), font.clone(), font.clone()));
        [own, system]
            .into_iter()
            .chain(named)
            .filter(|(_, name, _)| name.to_lowercase().contains(&wanted))
            .collect()
    }
}

/// A row of the list: the family the settings keep, its name, and the family it is drawn in.
type Choice = (Option<String>, SharedString, SharedString);

impl SettingsDialog {
    /// Opens the list of fonts on the font the app has, or closes it when it is open.
    fn toggle_fonts(&mut self, cx: &mut Context<Self>) {
        if self.themes.fonts.open {
            return self.close_fonts(cx);
        }
        if let Some(area) = self.field_area(FILTER) {
            area.update(cx, |area, cx| area.set_text("", cx));
        }
        let current = self.settings.ui_font.clone();
        let fonts = &mut self.themes.fonts;
        fonts.filter.clear();
        let choices = fonts.choices();
        fonts.active = choices
            .iter()
            .position(|(family, _, _)| *family == current)
            .unwrap_or(0);
        fonts
            .scroll
            .scroll_to_item(fonts.active, ScrollStrategy::Center);
        fonts.open = true;
        fonts.focus = Some(Focus::Filter);
        cx.notify();
    }

    fn close_fonts(&mut self, cx: &mut Context<Self>) {
        self.themes.fonts.open = false;
        self.themes.fonts.focus = Some(Focus::Dialog);
        cx.notify();
    }

    /// Moves the keyboard's row of the open list `by` rows, keeping it in view.
    fn step_fonts(&mut self, by: isize, cx: &mut Context<Self>) {
        let fonts = &mut self.themes.fonts;
        if !fonts.open {
            return;
        }
        let last = fonts.choices().len().saturating_sub(1);
        fonts.active = fonts.active.saturating_add_signed(by).min(last);
        fonts
            .scroll
            .scroll_to_item(fonts.active, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Sets the font of the open list's row `index`, and closes the list.
    fn pick_font(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.themes.fonts.open {
            return;
        }
        let Some((family, _, _)) = self.themes.fonts.choices().into_iter().nth(index) else {
            return;
        };
        self.update(|s| s.ui_font = family, cx);
        self.close_fonts(cx);
    }
}

/// The row that shows the app's font, and opens the list to change it.
fn font_row(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let current = d.settings.ui_font.as_ref();
    let name = match current.map(String::as_str) {
        None => SharedString::new_static("Default"),
        Some(SYSTEM_FONT) => SharedString::new_static("System"),
        Some(font) => SharedString::from(font.to_string()),
    };
    let family = current.map_or(SharedString::new_static(FONT_UI), SharedString::from);
    let open = d.themes.fonts.open;
    let select = select_box("font", TITLE, name, open, &t)
        .w(px(220.))
        .font_family(family)
        .on_hover(cx.listener(|d, over: &bool, _, _| d.themes.fonts.over = *over))
        .on_click(cx.listener(|d, _, _, cx| d.toggle_fonts(cx)));
    let list = open.then(|| font_list(d, window, cx));
    token_row(TITLE, None, &t)
        .child(div().relative().child(select).children(list))
        .into_any_element()
}

/// The open list of fonts, under the select: the filter, then each font that matches in its
/// own face. Up and Down move through them from the filter, Enter picks one, and Escape closes the list.
fn font_list(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let fresh = d.field_area(FILTER).is_none();
    let filter = d.themes.fonts.filter.clone();
    let kind = FieldKind {
        live: true,
        ..FieldKind::default()
    };
    let narrow = |d: &mut SettingsDialog, typed: String, _: &mut Context<SettingsDialog>| {
        let fonts = &mut d.themes.fonts;
        fonts.filter = typed;
        fonts.active = 0;
        fonts.scroll.scroll_to_item(0, ScrollStrategy::Top);
    };
    let field = d.field(
        FILTER,
        "Search fonts",
        &filter,
        "Search",
        kind,
        narrow,
        window,
        cx,
    );
    d.on_submit(FILTER, |d, _, cx| d.pick_font(d.themes.fonts.active, cx));
    if let Some(area) = d.field_area(FILTER) {
        if fresh {
            cx.subscribe(&area, |d, _, step: &Arrow, cx| d.step_fonts(step.0, cx))
                .detach();
        }
        if d.themes.fonts.focus == Some(Focus::Filter) {
            d.themes.fonts.focus = None;
            window.focus(&area.read(cx).focus_handle(cx), cx);
        }
    }
    let current = d.settings.ui_font.clone();
    let choices = Rc::new(d.themes.fonts.choices());
    let active = d.themes.fonts.active.min(choices.len().saturating_sub(1));
    let (count, rows) = (choices.len(), Rc::clone(&choices));
    let options = uniform_list(
        "font-options",
        count,
        cx.processor(move |_, range: Range<usize>, _, cx| {
            range
                .filter_map(|i| rows.get(i).map(|row| (i, row)))
                .map(|(i, (family, name, face))| {
                    let on = i == active;
                    div()
                        .id(i)
                        .role(Role::ListBoxOption)
                        .aria_label(name.clone())
                        .aria_selected(on)
                        .w_full()
                        .h(px(ROW_H))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(radius_sm()))
                        .cursor_pointer()
                        .font_family(face.clone())
                        .text_size(px(fs_md()))
                        .when(on, |row| row.bg(t.bg_selected))
                        .when(!on, |row| row.hover(move |s| s.bg(t.bg_hover)))
                        .on_click(cx.listener(move |d, _, _, cx| d.pick_font(i, cx)))
                        .child(div().flex_1().min_w_0().truncate().child(name.clone()))
                        .children((*family == current).then(|| icon("check", 14.)))
                        .into_any_element()
                })
                .collect()
        }),
    )
    .track_scroll(&d.themes.fonts.scroll)
    .size_full();
    let bar = d.themes.fonts.scroll.0.borrow().base_handle.clone();
    let list = div()
        .id("fonts")
        .role(Role::ListBox)
        .aria_label(TITLE)
        .size_full()
        .map(|list| {
            if count == 0 {
                list.child(
                    div()
                        .p(px(8.))
                        .text_color(t.text_subtle)
                        .child("No fonts match"),
                )
            } else {
                list.child(options)
            }
        });
    let menu = popup(&t)
        .id("font-picker")
        .occlude()
        .w(px(MENU_W))
        .gap(px(4.))
        .text_color(t.text)
        // A press on the select is left to its click, which closes this list.
        .on_mouse_down_out(cx.listener(|d, _: &MouseDownEvent, _, cx| {
            if !d.themes.fonts.over {
                d.close_fonts(cx);
            }
        }))
        .on_key_down(cx.listener(|d, e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" {
                cx.stop_propagation();
                d.close_fonts(cx);
            }
        }))
        .child(field)
        .child(
            div()
                .relative()
                .h(px(LIST_H))
                .child(list)
                .children(scrollbar(&bar, 10., 2., t.border_strong)),
        );
    // Over the dialog, which is itself drawn over the window: its top-right corner 4px under
    // the bottom-right one of the select.
    div()
        .absolute()
        .top_full()
        .right_0()
        .child(
            deferred(
                anchored()
                    .anchor(Anchor::TopRight)
                    .offset(point(px(0.), px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(3),
        )
        .into_any_element()
}

pub(in crate::settings) fn interface_pane(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    d.themes.fonts.load(cx);
    if d.themes.fonts.focus == Some(Focus::Dialog) {
        d.themes.fonts.focus = None;
        window.focus(&d.focus, cx);
    }
    let font = font_row(d, window, cx);
    div()
        .id("interface-pane")
        .flex()
        .flex_col()
        .child(section_heading("Layout", true, &t))
        .child(density_cards(d, cx))
        .child(motion_row(d, cx))
        .child(section_heading("Text", false, &t))
        .child(font)
        .child(text_size_cards(d, cx))
        .into_any_element()
}
