//! The bars around the window's content: the toolbar with the project dropdown, the activity bar
//! that picks the sidebar's view, and the status bar.

// Rust guideline compliant 2026-10-09

use gpui::{ClickEvent, FontWeight, Pixels, Point, Role, Window, div, point, prelude::*, px, relative};
use patchwork_server::threads::{MessageStatus, Role as MessageRole};

use super::{View, Workspace, is_busy};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::{ACTIVITY_BAR_W, FS_XS, RADIUS, RADIUS_SM, STATUSBAR_H, TOOLBAR_H, Theme, theme};
use crate::ui::{agent_label, button, display_title, icon, project_tile, spinner};

/// The toolbar's left padding, which lines the project tile up with the activity bar's icons.
const TOOLBAR_PL: f32 = (ACTIVITY_BAR_W + 4. - 20.) / 2. - 6.;

/// Where the project menu opens: 4px under the project button.
pub(super) fn project_menu_at() -> Point<Pixels> {
    point(px(TOOLBAR_PL), px(f32::midpoint(TOOLBAR_H, 26.) + 4.))
}

impl Workspace {
    /// Opens the project menu: every project, the current one marked, then "Open folder…". A
    /// press on the button while it is open closes it.
    fn toggle_project_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let closing_press = matches!(e, ClickEvent::Mouse(click)
            if self.project_menu_dismissed.take() == Some(click.down.position));
        if closing_press || self.project_menu.take().is_some() {
            cx.notify();
            return;
        }
        let this = cx.entity().downgrade();
        let current = self.project().map(|p| p.id.clone());
        let mut items: Vec<MenuItem> = self
            .settings
            .projects
            .iter()
            .map(|p| {
                let (id, this) = (p.id.clone(), this.clone());
                MenuItem::new(p.name.clone(), move |_, cx| {
                    let _ = this.update(cx, |w, cx| w.select_project(&id, cx));
                })
                .radio(current.as_ref() == Some(&p.id))
            })
            .collect();
        let open = MenuItem::new("Open folder…", move |window, cx| {
            let _ = this.update(cx, |w, cx| w.open_folder(window, cx));
        })
        .icon("folder-plus");
        items.push(if items.is_empty() {
            open
        } else {
            open.separated()
        });
        let menu = Menu::open("Projects", items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, window, cx| {
            this.project_menu_dismissed = e.0;
            this.project_menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        self.project_menu = Some(menu);
        cx.notify();
    }

    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        let name = self
            .project()
            .map_or_else(|| "No project".into(), |p| p.name.clone());
        let tile = match self.project() {
            Some(project) => project_tile(&project.name, 20., cx).into_any_element(),
            None => div()
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_SM))
                .bg(t.bg_accent_soft)
                .text_color(t.accent_text)
                .child(icon("chats", 12.))
                .into_any_element(),
        };
        let hover = t.bg_hover;
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
                button("project", format!("Project: {name}"))
                    .h(px(26.))
                    .gap(px(6.))
                    .rounded(px(RADIUS_SM))
                    .pl(px(4.))
                    .pr(px(6.))
                    .hover(move |s| s.bg(hover))
                    .aria_expanded(self.project_menu.is_some())
                    .on_click(cx.listener(super::state::Workspace::toggle_project_menu))
                    .child(tile)
                    .child(
                        div()
                            .max_w(px(180.))
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(icon("caret-down", 14.)),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
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
            .child(
                activity_button(SharedId("Settings"), "Settings".into(), &t, false)
                    .mt_auto()
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
