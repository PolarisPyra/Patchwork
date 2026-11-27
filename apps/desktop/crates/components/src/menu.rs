//! A popup list of actions, for right-click menus and dropdowns.
//!
//! A raised box of rows, each with an icon and a label. Up and Down move the highlight, Enter or
//! Space runs the row, and Escape or a press outside closes it. A long list can take a filter:
//! typing narrows its rows to those with the text. Its owner places it with [`Menu::view`] and
//! drops it on [`Dismissed`].

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Pixels, Point,
    Role, ScrollHandle, SharedString, Window, anchored, deferred, div, prelude::*, px,
};

use patchwork_ui_theme::{radius, radius_sm, theme};
use patchwork_ui_theme::icon;
use crate::popup_shadow;

/// The tallest a menu grows before its rows scroll.
const MAX_HEIGHT: f32 = 420.;

/// What a row does when it is picked.
pub type Run = Rc<dyn Fn(&mut Window, &mut App)>;

/// One row of a menu.
pub struct MenuItem {
    pub label: SharedString,
    pub icon: Option<&'static str>,
    pub danger: bool,
    /// One of a set of choices, marked with a dot: filled when it is the current one.
    pub checked: Option<bool>,
    /// A line above it, setting it apart from the rows before.
    pub separated: bool,
    pub run: Run,
}

impl std::fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuItem")
            .field("label", &self.label)
            .field("icon", &self.icon)
            .field("danger", &self.danger)
            .field("checked", &self.checked)
            .field("separated", &self.separated)
            .finish_non_exhaustive()
    }
}

impl MenuItem {
    pub fn new(
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon: None,
            danger: false,
            checked: None,
            separated: false,
            run: Rc::new(run),
        }
    }

    #[must_use]
    pub fn radio(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    #[must_use]
    pub fn separated(mut self) -> Self {
        self.separated = true;
        self
    }

    #[must_use]
    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// The menu closed: by a row, by Escape, or by a press outside it at the given point, which a
/// trigger checks so the press that closed the menu does not open it again.
#[derive(Debug)]
pub struct Dismissed(pub Option<Point<Pixels>>);

#[derive(Debug)]
pub struct Menu {
    label: SharedString,
    items: Vec<MenuItem>,
    focus: FocusHandle,
    /// The highlighted row, counted among the rows shown.
    highlight: Option<usize>,
    scroll: ScrollHandle,
    /// What was typed to narrow the rows, in a menu that takes a filter.
    filter: Option<String>,
}

impl EventEmitter<Dismissed> for Menu {}

impl Focusable for Menu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Menu {
    /// Opens a menu named `label` and moves focus into it.
    pub fn open(
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let label = label.into();
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Self {
                label,
                items,
                focus,
                highlight: None,
                scroll: ScrollHandle::new(),
                filter: None,
            }
        })
    }

    /// Opens a menu named `label` for the view that owns it, and tells that view through
    /// `closed` when the menu closes: by a row, by Escape, or by a press outside it. The owner
    /// then drops the menu and takes the focus back.
    pub fn open_for<V: 'static>(
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut gpui::Context<V>,
        closed: impl Fn(&mut V, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> Entity<Self> {
        let menu = Self::open(label, items, window, cx);
        cx.subscribe_in(&menu, window, move |owner, _, _: &Dismissed, window, cx| {
            closed(owner, window, cx);
        })
        .detach();
        menu
    }

    /// The menu laid over the window at `at`, kept inside it.
    #[must_use]
    pub fn view(menu: &Entity<Self>, at: Point<Pixels>) -> AnyElement {
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(menu.clone()),
        )
        .with_priority(3)
        .into_any_element()
    }

    /// Lets typing narrow the rows to those whose label has the text.
    pub fn take_filter(&mut self) {
        self.filter = Some(String::new());
    }

    /// The rows shown, by their place in the menu: all of them, or those the filter leaves.
    fn shown(&self) -> Vec<usize> {
        let wanted = self.filter.as_deref().unwrap_or_default().to_lowercase();
        (0..self.items.len())
            .filter(|&i| wanted.is_empty() || self.items[i].label.to_lowercase().contains(&wanted))
            .collect()
    }

    /// Takes a key as an edit of the filter, and returns whether it was one.
    fn edit_filter(&mut self, e: &KeyDownEvent) -> bool {
        let Some(filter) = &mut self.filter else {
            return false;
        };
        let keys = &e.keystroke;
        let held = keys.modifiers.control || keys.modifiers.platform;
        let typed = keys.key_char.as_deref().filter(|text| {
            // A space picks the highlighted row until something is typed.
            let picks = filter.is_empty() && text.trim().is_empty();
            !held && !picks && !text.chars().any(char::is_control)
        });
        match (keys.key.as_str(), typed) {
            ("backspace", _) if !filter.is_empty() => {
                filter.pop();
            }
            ("enter", _) => return false,
            (_, Some(text)) => filter.push_str(text),
            _ => return false,
        }
        self.highlight = Some(0);
        self.scroll.scroll_to_item(0);
        true
    }

    /// Runs the row at `at` among the rows shown.
    fn run(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self
            .shown()
            .get(at)
            .and_then(|&i| self.items.get(i))
            .map(|item| Rc::clone(&item.run))
        else {
            return;
        };
        cx.emit(Dismissed(None));
        run(window, cx);
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit_filter(e) {
            cx.stop_propagation();
            return cx.notify();
        }
        let shown = self.shown();
        let count = shown.len();
        if count == 0 {
            if e.keystroke.key == "escape" {
                cx.emit(Dismissed(None));
            }
            return;
        }
        let at = self.highlight.filter(|&i| i < count);
        match e.keystroke.key.as_str() {
            "down" => self.highlight = Some(at.map_or(0, |i| (i + 1) % count)),
            "up" => self.highlight = Some(at.map_or(count - 1, |i| (i + count - 1) % count)),
            "home" => self.highlight = Some(0),
            "end" => self.highlight = Some(count - 1),
            "enter" | "space" => {
                if let Some(i) = at {
                    self.run(i, window, cx);
                }
            }
            "escape" => cx.emit(Dismissed(None)),
            _ => return,
        }
        if let Some(at) = self.highlight {
            // Each line above a row is a child of the box too, and so is the filter.
            let narrowed = self.filter.as_ref().is_some_and(|text| !text.is_empty());
            let lines = shown
                .iter()
                .take(at + 1)
                .filter(|&&i| self.items[i].separated && !narrowed);
            let above = lines.count() + usize::from(self.filter.is_some());
            self.scroll.scroll_to_item(at + above);
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let shown = self.shown();
        let matches = shown.len();
        // What is typed, above the rows, read out as it changes.
        let filter = self.filter.as_ref().map(|text| {
            div()
                .id("menu-filter")
                .role(Role::Status)
                .aria_label(format!("Filter: {text}, {matches} of {}", self.items.len()))
                .flex()
                .flex_none()
                .h(px(patchwork_ui_theme::roomy(26.)))
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .text_color(if text.is_empty() {
                    t.text_muted
                } else {
                    t.text
                })
                .child(icon("magnifying-glass", 14.))
                .child(if text.is_empty() {
                    SharedString::from("Type to filter")
                } else {
                    SharedString::from(text.clone())
                })
                .into_any_element()
        });
        let narrowed = self.filter.as_ref().is_some_and(|text| !text.is_empty());
        let rows = shown.into_iter().enumerate().flat_map(|(i, at)| {
            let item = &self.items[at];
            let highlighted = self.highlight == Some(i);
            // The lines set kinds apart, which a narrowed list no longer shows whole.
            let line = (item.separated && !narrowed)
                .then(|| div().my(px(4.)).h(px(1.)).bg(t.border).into_any_element());
            let row = div()
                .id(i)
                .map(|row| match item.checked {
                    Some(checked) => row.role(Role::MenuItemRadio).aria_toggled(checked.into()),
                    None => row.role(Role::MenuItem),
                })
                .aria_label(item.label.clone())
                .flex()
                .h(px(patchwork_ui_theme::roomy(26.)))
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(radius_sm()))
                .cursor_pointer()
                .whitespace_nowrap()
                .text_color(if item.danger { t.danger } else { t.text })
                .when(highlighted, |row| row.bg(t.bg_hover))
                .hover(move |s| s.bg(t.bg_hover))
                .children(item.checked.map(|checked| {
                    div()
                        .size(px(6.))
                        .flex_none()
                        .rounded_full()
                        .bg(if checked { t.accent } else { t.border_strong })
                }))
                .children(item.icon.map(|name| icon(name, 14.)))
                .child(item.label.clone())
                .on_click(cx.listener(move |this, _, window, cx| this.run(i, window, cx)))
                .into_any_element();
            line.into_iter().chain([row])
        });
        div()
            .id("menu")
            .role(Role::Menu)
            .aria_label(self.label.clone())
            .track_focus(&self.focus)
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(160.))
            .max_h(px(MAX_HEIGHT))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .p(px(4.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .shadow(popup_shadow(&t))
            .text_color(t.text)
            .on_mouse_down_out(cx.listener(|_, e: &gpui::MouseDownEvent, _, cx| {
                cx.emit(Dismissed(Some(e.position)));
            }))
            .on_key_down(cx.listener(Self::on_key))
            .children(filter)
            .children(rows)
    }
}
