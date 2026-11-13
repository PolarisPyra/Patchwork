//! The bars around the window's content: the toolbar with the machine dropdown, the activity bar
//! that picks the sidebar's view, and the status bar.

// Rust guideline compliant 2026-10-09

use gpui::{
    App, ClickEvent, FontWeight, Pixels, Point, Role, Window, div, point, prelude::*, px, relative,
};
use patchwork_server::system::device::Kind;
use patchwork_server::threads::{MessageStatus, Role as MessageRole};

use super::{View, Workspace, is_busy};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::{ACTIVITY_BAR_W, FS_XS, RADIUS, RADIUS_SM, STATUSBAR_H, TOOLBAR_H, Theme, theme};
use crate::ui::{agent_label, button, display_title, icon, kind_icon, spinner};

/// The machine button's size, the same as the activity bar's buttons.
const MACHINE_BUTTON: f32 = 28.;

/// The toolbar's left padding, which puts the machine button's icon in the column of the activity
/// bar's icons: that bar's content, inside its 1px right border, centers 28px buttons.
const TOOLBAR_PL: f32 = (ACTIVITY_BAR_W - 1. - MACHINE_BUTTON) / 2.;

/// Where the machine menu opens: 4px under the machine button.
pub(super) fn machine_menu_at() -> Point<Pixels> {
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
        let picked = self.settings.default_machine.as_deref();
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
        let picked = self.settings.default_machine.clone();
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
        self.settings.default_machine.clone_from(&host);
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
            .server
            .store
            .update_settings(|s| s.default_machine = host)
        {
            log::warn!(error:% = e; "could not record the machine: {{error}}");
        }
        if elsewhere && let Some(id) = first {
            self.select_project(&id, cx);
        }
        cx.notify();
    }

    /// Opens the machine menu: this computer and every SSH host, the picked one marked. A press
    /// on the button while it is open closes it.
    fn toggle_machine_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let closing_press = matches!(e, ClickEvent::Mouse(click)
            if self.machine_menu_dismissed.take() == Some(click.down.position));
        if closing_press || self.machine_menu.take().is_some() {
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
                .icon(kind.map_or("hard-drives", kind_icon))
                .radio(picked.as_ref() == Some(&m.name))
            })
            .collect();
        let menu = Menu::open("Machines", items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, window, cx| {
            this.machine_menu_dismissed = e.0;
            this.machine_menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        self.machine_menu = Some(menu);
        cx.notify();
    }

    pub(super) fn toolbar(
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
                    .rounded(px(RADIUS))
                    .pl(px((MACHINE_BUTTON - 20.) / 2.))
                    .pr(px(6.))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .aria_expanded(self.machine_menu.is_some())
                    .on_click(cx.listener(super::state::Workspace::toggle_machine_menu))
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
                    .child(icon("caret-down", 14.)),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .rounded(px(RADIUS_SM))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.text_tint_6)
                            .px(px(8.))
                            .py(px(1.))
                            .text_size(px(FS_XS))
                            .line_height(px(16.))
                            .text_color(t.text_muted)
                            .child(patchwork_server::VERSION),
                    ),
            )
    }

    pub(super) fn activity_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let entry =
            |view: View, label: &'static str, name: &'static str, cx: &mut Context<Self>| {
                let active = self.view == Some(view);
                let count = self.changes.len();
                let badge = (view == View::Changes && count > 0).then(|| {
                    div()
                        .absolute()
                        .right(px(-4.))
                        .bottom(px(-4.))
                        .min_w(px(16.))
                        .rounded_full()
                        .bg(t.accent)
                        .px(px(4.))
                        .text_size(px(10.))
                        .line_height(px(16.))
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
                activity_button(SharedId(label), aria, &t, active)
                    .aria_toggled(if active {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .child(icon(name, 20.))
                    .children(badge)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.view = if this.view == Some(view) {
                            None
                        } else {
                            Some(view)
                        };
                        cx.notify();
                    }))
            };
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
            .child(entry(View::Tasks, "Tasks", "chats", cx))
            .child(entry(View::Changes, "Source Control", "git-diff", cx))
            .child(entry(View::Archived, "Archived", "archive", cx))
            .child(entry(View::Devices, "Devices", "devices", cx).mt_auto())
            .child(
                activity_button(SharedId("Settings"), "Settings".into(), &t, false)
                    .child(icon("gear-six", 20.))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_settings(None, window, cx)),
                    ),
            )
    }

    pub(super) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let project = self
            .project()
            .map_or_else(|| "No project".into(), |p| p.name.clone());
        let thread = self.thread.as_ref();
        let last = thread.and_then(|t| t.messages.last());
        let working = thread.is_some_and(is_busy);
        let done = last.is_some_and(|m| {
            m.status == MessageStatus::Complete && m.role == MessageRole::Assistant
        });
        let agent = thread
            .map(|t| t.agent)
            .or(self.settings.agents.first().copied())
            .unwrap_or(patchwork_server::provider::AgentKind::Claude);
        let plan_mode = thread.is_some_and(|t| t.plan_mode);
        let (hover, fg) = (t.bg_hover, t.text);
        let state = if working {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(spinner(12., t.text_muted))
                .child(format!("{} working…", agent_label(agent)))
        } else if done {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child("Done")
                .child(icon("check-bold", 12.).text_color(t.success))
        } else {
            div().child("Idle")
        };
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
            .px(px(10.))
            .text_size(px(FS_XS))
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
                    .child(project)
                    .when_some(self.branch.clone(), |row, branch| {
                        row.child(icon("caret-right", 12.))
                            .child(icon("git-branch", 12.))
                            .child(branch)
                    }),
            )
            .child(div().flex_1())
            .when_some(thread, |bar, thread| {
                bar.child(
                    div()
                        .h(px(20.))
                        .min_w_0()
                        .max_w(relative(0.4))
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .pr(px(6.))
                        .child(icon("circle-fill", 8.).text_color(t.accent))
                        .child(div().truncate().child(display_title(&thread.title))),
                )
            })
            .child(
                button(
                    "open-changes",
                    format!("Open changes: {}", self.changes.len()),
                )
                .h(px(20.))
                .rounded(px(3.))
                .px(px(6.))
                .hover(move |s| s.bg(hover).text_color(fg))
                .child(format!("{} changes", self.changes.len()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.view = Some(View::Changes);
                    this.change_focus = None;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .child(format!(
                        "Chat Mode: {}",
                        if plan_mode { "Plan" } else { "Default" }
                    )),
            )
            .child(
                div()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .child(state),
            )
    }
}

/// An id for a static label.
struct SharedId(&'static str);

impl From<SharedId> for gpui::ElementId {
    fn from(id: SharedId) -> Self {
        gpui::ElementId::Name(id.0.into())
    }
}

fn activity_button(
    id: SharedId,
    label: String,
    t: &Theme,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    let base = button(id, label)
        .relative()
        .size(px(28.))
        .justify_center()
        .rounded(px(RADIUS));
    if active {
        base.bg(t.bg_selected).text_color(t.text)
    } else {
        base.text_color(t.text_muted)
            .hover(move |s| s.bg(hover).text_color(fg))
    }
}
