//! Settings → Interface: how much room rows take, whether anything moves, the fonts of the app,
//! of code and of chats, and how large a chat's text is. A font is picked from a list of the
//! ones the computer has, narrowed by what is typed.

// Rust guideline compliant 2026-10-09

use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Focusable, KeyDownEvent, Role, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use patchwork_server::settings::Settings;

use super::look::density_cards;
use super::reading::{motion_row, text_size_cards};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, section_heading, select_box, token_row};
use crate::ui::text_area::Arrow;
use crate::ui::theme::{FONT_MONO, FONT_UI, FS_MD, font_ui, radius_sm, theme};
use crate::ui::{icon, scrollbar};

/// The family name that stands for the system's own font.
const SYSTEM_FONT: &str = ".SystemUIFont";

/// The field the list of fonts is narrowed from.
const FILTER: &str = "font-filter";

const ROW_H: f32 = 28.;
/// Eight fonts show at once.
const LIST_H: f32 = ROW_H * 8. + 8.;

/// The text a font is picked for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Text {
    App,
    Code,
    Chat,
}

impl Text {
    fn title(self) -> &'static str {
        match self {
            Self::App => "App font",
            Self::Code => "Code font",
            Self::Chat => "Chat font",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::App => "font-app",
            Self::Code => "font-code",
            Self::Chat => "font-chat",
        }
    }

    /// The family the settings keep for it; None is the app's own.
    fn of(self, settings: &Settings) -> Option<&String> {
        match self {
            Self::App => settings.ui_font.as_ref(),
            Self::Code => settings.code_font.as_ref(),
            Self::Chat => settings.chat_font.as_ref(),
        }
    }

    fn set(self, settings: &mut Settings, font: Option<String>) {
        match self {
            Self::App => settings.ui_font = font,
            Self::Code => settings.code_font = font,
            Self::Chat => settings.chat_font = font,
        }
    }

    /// The family it is drawn in while none is picked. Chats follow the app's font.
    fn own(self) -> SharedString {
        match self {
            Self::App => SharedString::new_static(FONT_UI),
            Self::Code => SharedString::new_static(FONT_MONO),
            Self::Chat => font_ui(),
        }
    }
}

/// The font picker of the pane: the fonts the computer has, and the list that is open.
#[derive(Default)]
pub(super) struct Fonts {
    all: Vec<SharedString>,
    open: Option<Text>,
    filter: String,
    /// The row of the open list the keyboard is on.
    active: usize,
    scroll: UniformListScrollHandle,
    /// The list just opened: its filter takes the keyboard.
    opened: bool,
    /// The list just closed: the dialog takes the keyboard.
    closed: bool,
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

    /// What the list for `text` offers: the family the settings keep, its name, and the family
    /// the name is drawn in.
    fn choices(&self, text: Text) -> Vec<Choice> {
        let wanted = self.filter.trim().to_lowercase();
        let own = (None, SharedString::new_static("Default"), text.own());
        let system = (
            Some(SYSTEM_FONT.to_string()),
            SharedString::new_static("System"),
            SharedString::new_static(SYSTEM_FONT),
        );
        let named = self
            .all
            .iter()
            .map(|font| (Some(font.to_string()), font.clone(), font.clone()));
        // The system's font has letters of many widths, which code is not set in.
        std::iter::once(own)
            .chain((text != Text::Code).then_some(system))
            .chain(named)
            .filter(|(_, name, _)| name.to_lowercase().contains(&wanted))
            .collect()
    }
}

/// A row of the list: the family the settings keep, its name, and the family it is drawn in.
type Choice = (Option<String>, SharedString, SharedString);

impl SettingsDialog {
    /// Opens the list of fonts for `text` on the font it has, or closes it when it is open.
    fn toggle_fonts(&mut self, text: Text, cx: &mut Context<Self>) {
        if self.themes.fonts.open == Some(text) {
            return self.close_fonts(cx);
        }
        if let Some(area) = self.field_area(FILTER) {
            area.update(cx, |area, cx| area.set_text("", cx));
        }
        let current = text.of(&self.settings).cloned();
        let fonts = &mut self.themes.fonts;
        fonts.filter.clear();
        let choices = fonts.choices(text);
        fonts.active = choices
            .iter()
            .position(|(family, _, _)| *family == current)
            .unwrap_or(0);
        fonts
            .scroll
            .scroll_to_item(fonts.active, ScrollStrategy::Center);
        fonts.open = Some(text);
        fonts.opened = true;
        cx.notify();
    }

    fn close_fonts(&mut self, cx: &mut Context<Self>) {
        self.themes.fonts.open = None;
        self.themes.fonts.closed = true;
        cx.notify();
    }

    /// Moves the keyboard's row of the open list `by` rows, keeping it in view.
    fn step_fonts(&mut self, by: isize, cx: &mut Context<Self>) {
        let fonts = &mut self.themes.fonts;
        let Some(text) = fonts.open else {
            return;
        };
        let last = fonts.choices(text).len().saturating_sub(1);
        fonts.active = fonts.active.saturating_add_signed(by).min(last);
        fonts
            .scroll
            .scroll_to_item(fonts.active, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Sets the font of the open list's row `index`, and closes the list.
    fn pick_font(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(text) = self.themes.fonts.open else {
            return;
        };
        let Some((family, _, _)) = self.themes.fonts.choices(text).into_iter().nth(index) else {
            return;
        };
        self.update(|s| text.set(s, family), cx);
        self.close_fonts(cx);
    }
}

/// The row that shows the font of `text`, and opens the list to change it.
fn font_row(d: &SettingsDialog, text: Text, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let current = text.of(&d.settings);
    let name = match current.map(String::as_str) {
        None => SharedString::new_static("Default"),
        Some(SYSTEM_FONT) => SharedString::new_static("System"),
        Some(font) => SharedString::from(font.to_string()),
    };
    let family = current.map_or_else(|| text.own(), SharedString::from);
    let open = d.themes.fonts.open == Some(text);
    let select = select_box(text.key(), text.title(), name, open, &t)
        .w(px(220.))
        .font_family(family)
        .on_click(cx.listener(move |d, _, _, cx| d.toggle_fonts(text, cx)));
    token_row(text.title(), None, &t)
        .child(select)
        .into_any_element()
}

/// The open list of fonts for `text`: the filter, then each font that matches in its own face.
/// Up and Down move through them from the filter, Enter picks one, and Escape closes the list.
fn font_list(
    d: &mut SettingsDialog,
    text: Text,
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
        if std::mem::take(&mut d.themes.fonts.opened) {
            window.focus(&area.read(cx).focus_handle(cx), cx);
        }
    }
    let current = text.of(&d.settings).cloned();
    let choices = Rc::new(d.themes.fonts.choices(text));
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
                        .h(px(ROW_H))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(radius_sm()))
                        .cursor_pointer()
                        .font_family(face.clone())
                        .text_size(px(FS_MD))
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
        .aria_label(text.title())
        .size_full()
        .p(px(4.))
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
    div()
        .id("font-picker")
        .border_b_1()
        .border_color(t.border)
        .pb(px(10.))
        .flex()
        .flex_col()
        .gap(px(6.))
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
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_editor)
                .child(list)
                .children(scrollbar(&bar, 10., 2., t.border_strong)),
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
    if std::mem::take(&mut d.themes.fonts.closed) {
        window.focus(&d.focus, cx);
    }
    let mut fonts = Vec::new();
    for text in [Text::App, Text::Code, Text::Chat] {
        fonts.push(font_row(d, text, cx));
        if d.themes.fonts.open == Some(text) {
            fonts.push(font_list(d, text, window, cx));
        }
    }
    div()
        .id("interface-pane")
        .flex()
        .flex_col()
        .child(section_heading("Layout", true, &t))
        .child(density_cards(d, cx))
        .child(motion_row(d, cx))
        .child(section_heading("Text", false, &t))
        .children(fonts)
        .child(text_size_cards(d, cx))
        .into_any_element()
}
