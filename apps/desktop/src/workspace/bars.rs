//! The bars around the window's content: the toolbar with the machine dropdown, the activity bar
//! that picks the sidebar's view, and the status bar.

// Rust guideline compliant 2026-10-09

use gpui::{App, ClickEvent, FontWeight, Pixels, Point, Role, Window, div, point, prelude::*, px};
use patchwork_platform::machine::device::Kind;

use crate::workspace::{Page, View, Workspace};
use patchwork_ui_components::menu::{Dismissed, Menu, MenuItem};
use patchwork_ui_theme::{
    IconName, ACTIVITY_BAR_W, fs_xs, radius, STATUSBAR_H, TOOLBAR_H, Theme, theme,
};
use patchwork_ui_components::{button, spinner};
use patchwork_ui_theme::icon;

use crate::sidebar::projects::kind_icon;

/// The machine button's size, the same as the activity bar's buttons.
const MACHINE_BUTTON: f32 = 28.;

/// The toolbar's left padding, which puts the machine button's icon in the column of the activity
/// bar's icons: that bar's content, inside its 1px right border, centers 28px buttons.
const TOOLBAR_PL: f32 = (ACTIVITY_BAR_W - 1. - MACHINE_BUTTON) / 2.;

/// Where the machine menu opens: 4px under the machine button.
pub(crate) fn machine_menu_at() -> Point<Pixels> {
    point(
        px(TOOLBAR_PL),
        px(f32::midpoint(TOOLBAR_H, MACHINE_BUTTON) + 4.),
    )
}

impl Workspace {
    /// The machine new projects open on: the SSH host picked, while the SSH config still lists
    /// it, or this computer.
    fn picked_machine(&self, cx: &App) -> Option<(String, Option<Kind>)> {
        let machines = self.devices_view.as_ref()?.read(cx).machines()?;
        let picked = self.settings.window.default_machine.as_deref();
        machines
            .iter()
            .find(|(m, _)| !m.local && Some(m.name.as_str()) == picked)
            .or_else(|| machines.iter().find(|(m, _)| m.local))
            .map(|(m, kind)| (m.name.clone(), *kind))
    }

    /// The SSH host picked in the toolbar, by its alias; none is this computer. Until the machines
    /// are read, the host saved last.
    pub(crate) fn machine_host(&self, cx: &App) -> Option<String> {
        let listed = self
            .devices_view
            .as_ref()
            .and_then(|v| v.read(cx).machines().map(<[_]>::to_vec));
        let picked = self.settings.window.default_machine.clone();
        match listed {
            Some(machines) => {
                picked.filter(|h| machines.iter().any(|(m, _)| !m.local && m.name == *h))
            }
            None => picked,
        }
    }

    /// Makes `host` the machine new projects open on and the sidebar lists; none is this
    /// computer. When the open chat is on another machine, the first project on this one opens.
    fn pick_machine(&mut self, host: Option<String>, cx: &mut Context<Self>) {
        self.settings.window.default_machine.clone_from(&host);
        let elsewhere = self
            .project()
            .map_or(host.is_some(), |p| p.host() != host.as_deref());
        let first = self
            .settings
            .projects
            .iter()
            .find(|p| p.host() == host.as_deref())
            .map(|p| p.id.clone());
        if let Err(e) = self
            .core
            .update_settings(|s| s.window.default_machine = host)
        {
            log::warn!(error:% = e; "could not record the machine: {{error}}");
        }
        if elsewhere && let Some(id) = first {
            self.select_project(&id, cx);
        }
        self.sync_panel(cx);
        cx.notify();
    }

    /// Opens the machine menu: this computer and every SSH host, the picked one marked. A press
    /// on the button while it is open closes it.
    fn toggle_machine_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let dismissed_at = self.dialogs_mut(cx, |d| d.machine_menu_dismissed.take());
        let closing_press = matches!(e, ClickEvent::Mouse(click)
            if dismissed_at == Some(click.down.position));
        if closing_press || self.dialogs_mut(cx, |d| d.machine_menu.take()).is_some() {
            cx.notify();
            return;
        }
        let this = cx.entity().downgrade();
        let picked = self.picked_machine(cx).map(|(name, _)| name);
        let machines = self
            .devices_view
            .as_ref()
            .and_then(|v| v.read(cx).machines().map(<[_]>::to_vec))
            .unwrap_or_default();
        let items: Vec<MenuItem> = machines
            .into_iter()
            .map(|(m, kind)| {
                let host = (!m.local).then(|| m.name.clone());
                let this = this.clone();
                let label = if m.local {
                    format!("{} (this computer)", m.name)
                } else {
                    m.name.clone()
                };
                MenuItem::new(label, move |_, cx| {
                    let _ = this.update(cx, |w, cx| w.pick_machine(host.clone(), cx));
                })
                .icon(kind.map_or(IconName::HardDrives, kind_icon))
                .radio(picked.as_ref() == Some(&m.name))
            })
            .collect();
        let menu = Menu::open("Machines", items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, window, cx| {
            this.dialogs_mut(cx, |d| {
                d.machine_menu_dismissed = e.0;
                d.machine_menu = None;
            });
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        self.dialogs_mut(cx, |d| d.machine_menu = Some(menu));
        cx.notify();
    }

    pub(crate) fn toolbar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // The Devices view reads the machines and what each is; the toolbar shows the same list.
        self.devices_view(window, cx);
        let t = *theme(cx);
        let picked = self.picked_machine(cx);
        let name = picked
            .as_ref()
            .map_or_else(|| "This computer".to_string(), |(name, _)| name.clone());
        let mark = match picked.as_ref().map(|(_, kind)| *kind) {
            Some(Some(kind)) => icon(kind_icon(kind), 20.).into_any_element(),
            _ => spinner(16., t.text_muted).into_any_element(),
        };
        let (hover, fg) = (t.bg_hover, t.text);
        div()
            .id("toolbar")
            .role(Role::Banner)
            .h(px(TOOLBAR_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .border_b_1()
            .border_color(t.border)
            .bg(t.bg_app)
            .pr(px(8.))
            .pl(px(TOOLBAR_PL))
            .child(
                button("machine", format!("Machine: {name}"))
                    .h(px(MACHINE_BUTTON))
                    .min_w(px(MACHINE_BUTTON))
                    .gap(px(6.))
                    .rounded(px(radius()))
                    .pl(px((MACHINE_BUTTON - 20.) / 2.))
                    .pr(px(6.))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .aria_expanded(self.dialogs.read(cx).machine_menu.is_some())
                    .on_click(cx.listener(super::window::Workspace::toggle_machine_menu))
                    .child(
                        div()
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(mark),
                    )
                    .child(
                        div()
                            .max_w(px(180.))
                            .truncate()
                            .text_color(fg)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(icon(IconName::CaretDown, 14.)),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .children(super::hot_reload::bar(&t, cx))
                    .child(
                        div()
                            .px(px(6.))
                            .text_size(px(fs_xs()))
                            .line_height(px(16.))
                            .text_color(t.text_subtle)
                            .child(patchwork_platform::VERSION),
                    ),
            )
    }

    /// Shows the orchestrator page. On the page, opens or closes its task list, as a view's icon
    /// does for the chats. The focus stays on the icon, so Space does the same again.
    fn toggle_orchestrator(&mut self, cx: &mut Context<Self>) {
        if self.page == Page::Orchestrator {
            self.orchestrator_tasks = !self.orchestrator_tasks;
        } else {
            self.page = Page::Orchestrator;
        }
        cx.notify();
    }

    pub(crate) fn activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let entry = |view: View, label: &'static str, name: IconName, cx: &mut Context<Self>| {
            let active = self.page == Page::Chats && self.sidebar_view() == Some(view);
            let count = self.changes.len();
            let badge = (view == View::Changes && count > 0).then(|| {
                div()
                    .absolute()
                    .right(px(-3.))
                    .bottom(px(-2.))
                    .min_w(px(14.))
                    .rounded_full()
                    .bg(t.accent)
                    .px(px(3.))
                    .text_size(px(patchwork_ui_theme::fs(10.)))
                    .line_height(px(14.))
                    .text_color(t.text_on_accent)
                    .flex()
                    .justify_center()
                    .child(count.to_string())
            });
            let aria = if badge.is_some() {
                format!("{label}, {count} changes")
            } else {
                label.to_string()
            };
            activity_button(label, aria, &t, active)
                .aria_toggled(active.into())
                .child(icon(name, 20.))
                .children(badge)
                .on_click(cx.listener(move |this, _, _, cx| {
                    // From the orchestrator page, a view's icon goes to that view.
                    this.view = if this.page == Page::Chats && this.sidebar_view() == Some(view) {
                        None
                    } else {
                        Some(view)
                    };
                    this.page = Page::Chats;
                    this.peeking = false;
                    cx.notify();
                }))
        };
        // Lit while its page shows, with the task list open or closed. Toggled follows the list.
        let on_page = self.page == Page::Orchestrator;
        let orchestrating = on_page && self.orchestrator_tasks;
        let on_quilts = self.page == Page::Quilts;
        div()
            .id("activity-bar")
            .role(Role::Navigation)
            .aria_label("Views")
            .w(px(ACTIVITY_BAR_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .border_r_1()
            .border_color(t.border)
            .bg(t.bg_app)
            .py(px(6.))
            .child(
                activity_button("Orchestrator", "Orchestrator".into(), &t, on_page)
                    .aria_toggled(orchestrating.into())
                    .child(icon(IconName::Butterfly, 20.))
                    .on_click(cx.listener(|this, e, window, cx| {
                        // A click leaves the focus on nothing, so typing goes to the pill and
                        // Space still opens and closes the list. A key press keeps it here.
                        if matches!(e, gpui::ClickEvent::Mouse(_)) {
                            window.focus(&this.focus, cx);
                        }
                        this.toggle_orchestrator(cx);
                    })),
            )
            .child(entry(View::Tasks, "Tasks", IconName::Chats, cx))
            .child(entry(
                View::Changes,
                "Source Control",
                IconName::GitDiff,
                cx,
            ))
            .child(
                activity_button("Quilts", "Quilts".into(), &t, on_quilts)
                    .aria_toggled(on_quilts.into())
                    .child(icon(IconName::CodeBlock, 20.))
                    .on_click(cx.listener(|this, _, _, cx| this.show_quilts_page(cx))),
            )
            .child(entry(View::Archived, "Archived", IconName::Archive, cx))
            .child(
                activity_button("Settings", "Settings".into(), &t, false)
                    .mt_auto()
                    .child(icon(IconName::GearSix, 20.))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_settings(None, window, cx)),
                    ),
            )
    }

    /// The status bar: the project and its branch at the left, and uncommitted changes at the right.
    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_status_bar(cx)
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = *theme(cx);
        let project = self.project();
        let place = project.map_or_else(|| "No project".into(), |p| p.name.clone());
        // A segment: its mark, then its words, cut short past 280px.
        let segment = |mark: IconName, text: String| {
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(6.))
                .child(div().flex_none().child(icon(mark, 12.)))
                .child(div().max_w(px(280.)).truncate().child(text))
        };
        let changes = self.changes.len();
        let changes = (changes > 0).then(|| {
            let fg = t.text;
            let label = if changes == 1 {
                "1 change".to_string()
            } else {
                format!("{changes} changes")
            };
            button("status-changes", label.clone())
                .child(segment(IconName::GitDiff, label))
                .hover(move |s| s.text_color(fg))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = Some(View::Changes);
                    this.page = Page::Chats;
                    this.peeking = false;
                    cx.notify();
                }))
        });
        div()
            .id("status-bar")
            .role(Role::ContentInfo)
            .aria_label("Status bar")
            .w_full()
            .h(px(STATUSBAR_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.))
            .border_t_1()
            .border_color(t.border)
            .bg(t.bg_app)
            .px(px(8.))
            .text_size(px(fs_xs()))
            .line_height(px(16.))
            .text_color(t.text_muted)
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(4.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(
                        segment(IconName::Desktop, place)
                            .when(project.is_none(), |s| s.text_color(t.text_subtle)),
                    )
                    .when_some(self.branch.clone(), |row, branch| {
                        row.child(segment(IconName::GitBranch, branch).text_color(t.text_subtle))
                    }),
            )
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .min_w_0()
                    .items_center()
                    .gap(px(10.))
                    .whitespace_nowrap()
                    .children(changes),
            )
            .into_any_element()
    }
}

fn activity_button(
    id: &'static str,
    label: String,
    t: &Theme,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    let base = button(id, label)
        .relative()
        .size(px(28.))
        .justify_center()
        .rounded(px(radius()));
    if active {
        base.bg(t.bg_selected).text_color(t.text)
    } else {
        base.text_color(t.text_muted)
            .hover(move |s| s.bg(hover).text_color(fg))
    }
}
