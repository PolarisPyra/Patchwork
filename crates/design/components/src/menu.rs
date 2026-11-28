//! A popup list of actions, for right-click menus and dropdowns.
//!
//! A raised box of rows, each with an icon, a label, and the keys that run it when it has some.
//! A row can hold a menu of its own, which opens beside it when the pointer rests on it or Right
//! is pressed, and closes on Left. Moving the pointer straight at an open menu keeps it open over
//! the rows on the way. Up and Down move the highlight, Enter or Space runs the row, and Escape or
//! a press outside closes the menu. A long list can take a filter: typing narrows its rows to
//! those with the text. Its owner places it with [`Menu::view`] and drops it on [`Dismissed`].

// Rust guideline compliant 2026-10-09

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Action, AnyElement, App, Bounds, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    MouseMoveEvent, Pixels, Point, Role, ScrollHandle, SharedString, Subscription, Window,
    anchored, canvas, deferred, div, point, prelude::*, px,
};

use patchwork_design_icons::IconName;
use patchwork_design_icons::icon;
use patchwork_design_theme::{fs_xs, radius, radius_sm, theme};
use crate::popup_shadow;

/// The tallest a menu grows before its rows scroll.
const MAX_HEIGHT: f32 = 420.;
/// The room between a menu and a menu opened from one of its rows.
const SUBMENU_GAP: f32 = 4.;
/// The box's padding and border, which a menu opened from a row moves up by so its first row
/// lines up with that row.
const INSET: f32 = 5.;
/// How long the pointer rests on its way to an open menu before the row under it takes over.
const REST: Duration = Duration::from_millis(250);

/// A row, by its place among the rows shown, and where it was drawn.
type Drawn = (usize, Bounds<Pixels>);

/// What a row does when it is picked.
pub type Run = Rc<dyn Fn(&mut Window, &mut App)>;

/// One row of a menu.
#[derive(Clone)]
pub struct MenuItem {
    pub label: SharedString,
    /// Where it is, quieter after the label.
    pub detail: Option<SharedString>,
    pub icon: Option<IconName>,
    pub danger: bool,
    /// One of a set of choices, marked with a dot: filled when it is the current one.
    pub checked: Option<bool>,
    /// A line above it, setting it apart from the rows before.
    pub separated: bool,
    /// The action whose keys show at the row's end.
    pub keys: Option<Rc<dyn Action>>,
    /// The rows of the menu it opens, in place of running.
    pub submenu: Option<Vec<MenuItem>>,
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
            .field("submenu", &self.submenu)
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
            detail: None,
            icon: None,
            danger: false,
            checked: None,
            separated: false,
            keys: None,
            submenu: None,
            run: Rc::new(run),
        }
    }

    /// A row that opens a menu of `items` beside it.
    pub fn submenu(label: impl Into<SharedString>, items: Vec<MenuItem>) -> Self {
        Self {
            submenu: Some(items),
            ..Self::new(label, |_, _| {})
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
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, name: IconName) -> Self {
        self.icon = Some(name);
        self
    }

    #[must_use]
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Shows the keys bound to `action` at the row's end. A row whose action has no keys shows
    /// none.
    #[must_use]
    pub fn keys(mut self, action: impl Action) -> Self {
        self.keys = Some(Rc::new(action));
        self
    }
}

/// The menu closed: by a row, by Escape, or by a press outside it at the given point, which a
/// trigger checks so the press that closed the menu does not open it again.
#[derive(Debug)]
pub struct Dismissed(pub Option<Point<Pixels>>);

/// A menu opened from a row asks to close and give the keys back to the menu it came from.
#[derive(Debug)]
pub struct Back;

/// The menu open from one of a menu's rows.
struct Open {
    /// The row it was opened from, among the rows shown.
    row: usize,
    menu: Entity<Menu>,
    /// Where it is drawn, worked out when it opened.
    at: Point<Pixels>,
    /// It opens to the row's left, as there is no room to the right.
    left: bool,
    /// The keys pressed for an attached menu go to it.
    keyed: bool,
    _events: [Subscription; 2],
}

pub struct Menu {
    label: SharedString,
    items: Vec<MenuItem>,
    focus: FocusHandle,
    /// The highlighted row, counted among the rows shown.
    highlight: Option<usize>,
    scroll: ScrollHandle,
    /// What was typed to narrow the rows, in a menu that takes a filter.
    filter: Option<String>,
    /// Where the box was last drawn.
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Where each row that opens a menu was last drawn, by its place among the rows shown.
    triggers: Rc<RefCell<Vec<Drawn>>>,
    submenu: Option<Open>,
    /// Where the pointer was when it last moved over a row that took it, the corner of the
    /// triangle it may cross to reach the open menu.
    apex: Option<Point<Pixels>>,
    /// It was opened from another menu's row, and gives the keys back to it on Left or Escape.
    nested: bool,
    /// What had the keys when the menu opened, whose bindings the rows show.
    owner: Option<FocusHandle>,
    /// Counts the waits for the pointer to rest, so a newer one cancels the older.
    resting: usize,
    /// It leaves the focus where it was, and its owner hands it the keys through
    /// [`Menu::press`]; its highlight stays as the pointer leaves.
    attached: bool,
}

impl std::fmt::Debug for Menu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Menu")
            .field("label", &self.label)
            .field("items", &self.items)
            .field("highlight", &self.highlight)
            .field("filter", &self.filter)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<Dismissed> for Menu {}
impl EventEmitter<Back> for Menu {}

impl Focusable for Menu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Menu {
    fn build(
        label: SharedString,
        items: Vec<MenuItem>,
        focused: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let owner = window.focused(cx);
        if focused {
            window.focus(&focus, cx);
        }
        Self {
            label,
            items,
            focus,
            highlight: None,
            scroll: ScrollHandle::new(),
            filter: None,
            bounds: Rc::default(),
            triggers: Rc::default(),
            submenu: None,
            apex: None,
            nested: false,
            owner,
            resting: 0,
            attached: false,
        }
    }

    /// Opens a menu named `label` and moves focus into it.
    pub fn open(
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let label = label.into();
        cx.new(|cx| Self::build(label, items, true, window, cx))
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

    /// Opens a menu named `label` that leaves the focus where it is, as one over a text field
    /// whose typing narrows it. The field hands it the keys through [`Menu::press`].
    pub fn attached(
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let label = label.into();
        cx.new(|cx| {
            let mut menu = Self::build(label, items, false, window, cx);
            menu.attached = true;
            menu.highlight = Some(0);
            menu
        })
    }

    /// A key pressed in the field an attached menu belongs to: Up and Down move the highlight,
    /// Enter or Tab runs the row or goes into its menu, and Escape comes back out of that menu.
    /// Returns whether the menu took the key.
    pub fn press(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(open) = self.submenu.as_ref().filter(|open| open.keyed) {
            if key == "escape" {
                self.submenu = None;
                cx.notify();
                return true;
            }
            let menu = open.menu.clone();
            return menu.update(cx, |menu, cx| menu.press(key, window, cx));
        }
        let count = self.shown().len();
        if count == 0 {
            return false;
        }
        let at = self.highlight.filter(|&i| i < count);
        match key {
            "down" => self.highlight = Some(at.map_or(0, |i| (i + 1) % count)),
            "up" => self.highlight = Some(at.map_or(count - 1, |i| (i + count - 1) % count)),
            "enter" | "tab" => {
                let Some(i) = at else {
                    return false;
                };
                if self.item(i).is_some_and(|item| item.submenu.is_some()) {
                    self.open_submenu(i, false, window, cx);
                    if let Some(open) = &mut self.submenu {
                        open.keyed = true;
                        open.menu.update(cx, |menu, cx| {
                            menu.attached = true;
                            menu.highlight = Some(0);
                            cx.notify();
                        });
                    }
                } else {
                    self.run(i, false, window, cx);
                }
                return true;
            }
            _ => return false,
        }
        self.close_submenu(window, cx);
        if let Some(at) = self.highlight {
            self.scroll.scroll_to_item(at + self.lines_above(at));
        }
        cx.notify();
        true
    }

    /// The lines drawn above the row at `at` among the rows shown, and the filter, which are
    /// children of the list too.
    fn lines_above(&self, at: usize) -> usize {
        let narrowed = self.filter.as_ref().is_some_and(|text| !text.is_empty());
        // The first row has no line above it.
        let lines = self
            .shown()
            .into_iter()
            .take(at + 1)
            .skip(1)
            .filter(|&i| self.items[i].separated && !narrowed)
            .count();
        lines + usize::from(self.filter.is_some())
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

    /// The row at `at` among the rows shown.
    fn item(&self, at: usize) -> Option<&MenuItem> {
        self.shown().get(at).and_then(|&i| self.items.get(i))
    }

    /// Whether `at` is on this menu or on a menu open from it.
    fn covers(&self, at: Point<Pixels>, cx: &App) -> bool {
        self.bounds.get().is_some_and(|b| b.contains(&at))
            || self
                .submenu
                .as_ref()
                .is_some_and(|open| open.menu.read(cx).covers(at, cx))
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
        self.submenu = None;
        self.scroll.scroll_to_item(0);
        true
    }

    /// Runs the row at `at` among the rows shown, or opens its menu.
    fn run(&mut self, at: usize, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item(at) else {
            return;
        };
        if item.submenu.is_some() {
            self.highlight = Some(at);
            self.open_submenu(at, focus, window, cx);
            return;
        }
        let run = Rc::clone(&item.run);
        cx.emit(Dismissed(None));
        run(window, cx);
    }

    /// Opens the menu of the row at `at` beside it, with the keys in it when `focus`.
    fn open_submenu(
        &mut self,
        at: usize,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(open) = self.submenu.as_ref().filter(|open| open.row == at) {
            if focus {
                let menu = open.menu.clone();
                menu.update(cx, |menu, cx| {
                    menu.highlight.get_or_insert(0);
                    window.focus(&menu.focus, cx);
                    cx.notify();
                });
            }
            return;
        }
        let Some(item) = self.item(at) else {
            return;
        };
        let (Some(items), Some(bounds)) = (item.submenu.clone(), self.bounds.get()) else {
            return;
        };
        let label = item.label.clone();
        let owner = self.owner.clone();
        let row = self
            .triggers
            .borrow()
            .iter()
            .find(|(i, _)| *i == at)
            .map(|(_, b)| *b);
        let top = row.map_or(bounds.top(), |b| b.top()) - px(INSET);
        // The menu opens to the right unless it would run off the window; its width is a guess
        // until it is drawn, and the window keeps it in either way.
        let room = window.viewport_size().width - bounds.right();
        let left = room < px(240.) && bounds.left() > room;
        let at_point = if left {
            point(bounds.left() - px(SUBMENU_GAP), top)
        } else {
            point(bounds.right() + px(SUBMENU_GAP), top)
        };
        let menu = cx.new(|cx| {
            let mut menu = Self::build(label, items, focus, window, cx);
            menu.nested = true;
            menu.owner = owner;
            if focus {
                menu.highlight = Some(0);
            }
            menu
        });
        let events = [
            // A row of the menu ran, so this one closes too.
            cx.subscribe(&menu, |_, _, e: &Dismissed, cx| {
                if e.0.is_none() {
                    cx.emit(Dismissed(None));
                }
            }),
            cx.subscribe_in(&menu, window, |this, _, _: &Back, window, cx| {
                this.submenu = None;
                window.focus(&this.focus, cx);
                cx.notify();
            }),
        ];
        self.close_submenu(window, cx);
        self.submenu = Some(Open {
            row: at,
            menu,
            at: at_point,
            left,
            keyed: false,
            _events: events,
        });
        cx.notify();
    }

    /// The pointer moved over the row at `at`: it takes the highlight and opens its menu, unless
    /// the pointer is on its way to the menu already open.
    fn pointed(
        &mut self,
        at: usize,
        e: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pointer = e.position;
        if self.submenu.as_ref().is_some_and(|open| open.row != at) && self.heading_out(pointer, cx)
        {
            // Over this row only on the way across: it takes over once the pointer rests.
            self.resting += 1;
            let resting = self.resting;
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(REST).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.resting == resting && window.mouse_position() == pointer {
                        this.apex = Some(pointer);
                        this.take(at, window, cx);
                    }
                });
            })
            .detach();
            return;
        }
        self.resting += 1;
        self.apex = Some(pointer);
        self.take(at, window, cx);
    }

    /// Moves the highlight to the row at `at`, opening its menu and closing any other.
    fn take(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let open = self.submenu.as_ref().map(|open| open.row);
        let opens = self.item(at).is_some_and(|item| item.submenu.is_some());
        if self.highlight == Some(at) && (open == Some(at) || !opens) {
            return;
        }
        self.highlight = Some(at);
        if open != Some(at) {
            self.close_submenu(window, cx);
            if opens {
                self.open_submenu(at, false, window, cx);
            }
        }
        cx.notify();
    }

    /// Closes the menu open from a row, taking the keys back if it had them.
    fn close_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(open) = self.submenu.take()
            && open.menu.read(cx).focus.contains_focused(window, cx)
        {
            window.focus(&self.focus, cx);
        }
    }

    /// Whether the pointer, now at `at`, is inside the triangle between where it last took a row
    /// and the near edge of the open menu: on its way to that menu.
    fn heading_out(&self, at: Point<Pixels>, cx: &App) -> bool {
        let Some(apex) = self.apex else {
            return false;
        };
        let Some(open) = &self.submenu else {
            return false;
        };
        let Some(b) = open.menu.read(cx).bounds.get() else {
            return false;
        };
        let edge = if open.left { b.right() } else { b.left() };
        inside(at, apex, point(edge, b.top()), point(edge, b.bottom()))
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
        let opens = at.is_some_and(|i| self.item(i).is_some_and(|item| item.submenu.is_some()));
        match e.keystroke.key.as_str() {
            "down" => self.highlight = Some(at.map_or(0, |i| (i + 1) % count)),
            "up" => self.highlight = Some(at.map_or(count - 1, |i| (i + count - 1) % count)),
            "home" => self.highlight = Some(0),
            "end" => self.highlight = Some(count - 1),
            "enter" | "space" => {
                if let Some(i) = at {
                    self.run(i, true, window, cx);
                }
            }
            "right" if opens => {
                if let Some(i) = at {
                    self.open_submenu(i, true, window, cx);
                }
            }
            // A menu opened from a row closes alone; the first one closes them all.
            "left" | "escape" if self.nested => cx.emit(Back),
            "escape" => cx.emit(Dismissed(None)),
            _ => return,
        }
        if matches!(e.keystroke.key.as_str(), "down" | "up" | "home" | "end") {
            self.close_submenu(window, cx);
        }
        if let Some(at) = self.highlight {
            self.scroll.scroll_to_item(at + self.lines_above(at));
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// Whether `p` is inside the triangle `a`, `b`, `c`.
fn inside(p: Point<Pixels>, a: Point<Pixels>, b: Point<Pixels>, c: Point<Pixels>) -> bool {
    let side = |p: Point<Pixels>, q: Point<Pixels>, r: Point<Pixels>| {
        (f32::from(p.x) - f32::from(r.x)) * (f32::from(q.y) - f32::from(r.y))
            - (f32::from(q.x) - f32::from(r.x)) * (f32::from(p.y) - f32::from(r.y))
    };
    let (d1, d2, d3) = (side(p, a, b), side(p, b, c), side(p, c, a));
    let negative = d1 < 0. || d2 < 0. || d3 < 0.;
    let positive = d1 > 0. || d2 > 0. || d3 > 0.;
    !(negative && positive)
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let os = cx.platform_info().os;
        let shown = self.shown();
        let matches = shown.len();
        let row_h = px(patchwork_design_theme::roomy(28.));
        // What is typed, above the rows, read out as it changes.
        let filter = self.filter.as_ref().map(|text| {
            div()
                .id("menu-filter")
                .role(Role::Status)
                .aria_label(format!("Filter: {text}, {matches} of {}", self.items.len()))
                .flex()
                .flex_none()
                .h(row_h)
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .text_color(if text.is_empty() {
                    t.text_muted
                } else {
                    t.text
                })
                .child(icon(IconName::MagnifyingGlass, 14.))
                .child(if text.is_empty() {
                    SharedString::from("Type to filter")
                } else {
                    SharedString::from(text.clone())
                })
                .into_any_element()
        });
        let narrowed = self.filter.as_ref().is_some_and(|text| !text.is_empty());
        self.triggers.borrow_mut().clear();
        let open_row = self.submenu.as_ref().map(|open| open.row);
        let mut rows = Vec::with_capacity(shown.len());
        for (i, at) in shown.into_iter().enumerate() {
            let item = &self.items[at];
            let highlighted = self.highlight == Some(i) || open_row == Some(i);
            // The lines set kinds apart, which a narrowed list no longer shows whole.
            if item.separated && !narrowed && i > 0 {
                rows.push(
                    div()
                        .mx(px(-4.))
                        .my(px(4.))
                        .h(px(1.))
                        .bg(t.border)
                        .into_any_element(),
                );
            }
            let keys = item
                .keys
                .as_deref()
                .and_then(|action| match &self.owner {
                    Some(owner) => window.highest_precedence_binding_for_action_in(action, owner),
                    None => window.highest_precedence_binding_for_action(action),
                })
                .map(|binding| {
                    let keys: Vec<String> = binding
                        .keystrokes()
                        .iter()
                        .map(|k| k.display(os).to_string())
                        .collect();
                    SharedString::from(keys.join(" "))
                });
            let muted = if item.danger { t.danger } else { t.text_muted };
            let trigger = item.submenu.is_some().then(|| {
                let triggers = Rc::clone(&self.triggers);
                canvas(
                    move |bounds, _, _| triggers.borrow_mut().push((i, bounds)),
                    |_, (), _, _| {},
                )
                .absolute()
                .inset_0()
            });
            let row = div()
                .id(i)
                .map(|row| match item.checked {
                    Some(checked) => row.role(Role::MenuItemRadio).aria_toggled(checked.into()),
                    None => row.role(Role::MenuItem),
                })
                .aria_label(item.label.clone())
                .relative()
                .flex()
                .flex_none()
                .h(row_h)
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .rounded(px(radius_sm()))
                .cursor_pointer()
                .whitespace_nowrap()
                .text_color(if item.danger { t.danger } else { t.text })
                .when(highlighted, |row| row.bg(t.bg_hover))
                .children(trigger)
                .children(item.checked.map(|checked| {
                    div()
                        .size(px(6.))
                        .flex_none()
                        .rounded_full()
                        .bg(if checked { t.accent } else { t.border_strong })
                }))
                .children(
                    item.icon
                        .map(|name| div().flex_none().text_color(muted).child(icon(name, 14.))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(item.label.clone()),
                )
                .children(item.detail.clone().map(|detail| {
                    div()
                        .flex_none()
                        .max_w(px(140.))
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(detail)
                }))
                .children(keys.map(|keys| {
                    div()
                        .flex_none()
                        .pl(px(16.))
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(keys)
                }))
                .when(item.submenu.is_some(), |row| {
                    row.child(
                        div()
                            .flex_none()
                            .pl(px(8.))
                            .text_color(t.text_muted)
                            .child(icon(IconName::CaretRight, 12.)),
                    )
                })
                .on_mouse_move(cx.listener(move |this, e: &MouseMoveEvent, window, cx| {
                    this.pointed(i, e, window, cx);
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.run(i, false, window, cx)))
                .into_any_element();
            rows.push(row);
        }
        let bounds = Rc::clone(&self.bounds);
        let submenu = self.submenu.as_ref().map(|open| {
            anchored()
                .position(open.at)
                .anchor(if open.left {
                    gpui::Anchor::TopRight
                } else {
                    gpui::Anchor::TopLeft
                })
                .snap_to_window_with_margin(px(8.))
                .child(open.menu.clone())
        });
        div()
            .relative()
            .child(
                div()
                    .id("menu")
                    .role(Role::Menu)
                    .aria_label(self.label.clone())
                    .track_focus(&self.focus)
                    .occlude()
                    .flex()
                    .flex_col()
                    .min_w(px(180.))
                    .max_w(px(360.))
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
                    .on_mouse_down_out(cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
                        if !this.covers(e.position, cx) {
                            cx.emit(Dismissed(Some(e.position)));
                        }
                    }))
                    .on_key_down(cx.listener(Self::on_key))
                    // The pointer leaving lets go of its row, unless that row's menu is open.
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered && this.submenu.is_none() && !this.attached {
                            this.highlight = None;
                            cx.notify();
                        }
                    }))
                    .children(filter)
                    .children(rows),
            )
            .child(
                canvas(move |b, _, _| bounds.set(Some(b)), |_, (), _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .children(submenu)
    }
}
