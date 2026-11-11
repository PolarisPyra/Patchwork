//! A popup list of actions, for right-click menus and dropdowns: a raised box
//! of rows, each with an icon and a label. Up and Down move the highlight, Enter or Space runs the
//! row, and Escape or a press outside closes it. Its owner places it with [`Menu::view`] and
//! drops it on [`Dismissed`].

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    AnyElement, App, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Pixels, Point,
    Role, SharedString, Toggled, Window, anchored, deferred, div, prelude::*, px,
};

use crate::ui::theme::{RADIUS, RADIUS_SM, theme};
use crate::ui::{icon, popup_shadow};

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

    pub fn radio(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn separated(mut self) -> Self {
        self.separated = true;
        self
    }

    pub fn icon(mut self, name: &'static str) -> Self {
        self.icon = Some(name);
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

/// The menu closed: by a row, by Escape, or by a press outside it at the given point, which a
/// trigger checks so the press that closed the menu does not open it again.
pub struct Dismissed(pub Option<Point<Pixels>>);

pub struct Menu {
    label: SharedString,
    items: Vec<MenuItem>,
    focus: FocusHandle,
    highlight: Option<usize>,
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

    fn run(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.items.get(i).map(|item| Rc::clone(&item.run)) else {
            return;
        };
        cx.emit(Dismissed(None));
        run(window, cx);
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.items.len();
        if count == 0 {
            return;
        }
        let at = self.highlight;
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
        cx.stop_propagation();
        cx.notify();
    }
}

impl Render for Menu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let rows = self.items.iter().enumerate().flat_map(|(i, item)| {
            let highlighted = self.highlight == Some(i);
            let line = item
                .separated
                .then(|| div().my(px(4.)).h(px(1.)).bg(t.border).into_any_element());
            let row = div()
                .id(i)
                .map(|row| match item.checked {
                    Some(checked) => row.role(Role::MenuItemRadio).aria_toggled(if checked {
                        Toggled::True
                    } else {
                        Toggled::False
                    }),
                    None => row.role(Role::MenuItem),
                })
                .aria_label(item.label.clone())
                .flex()
                .h(px(26.))
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(RADIUS_SM))
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
            .p(px(4.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .shadow(popup_shadow(&t))
            .text_color(t.text)
            .on_mouse_down_out(cx.listener(|_, e: &gpui::MouseDownEvent, _, cx| {
                cx.emit(Dismissed(Some(e.position)));
            }))
            .on_key_down(cx.listener(Self::on_key))
            .children(rows)
    }
}
