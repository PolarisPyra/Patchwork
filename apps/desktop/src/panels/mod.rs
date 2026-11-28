//! The window's frame and the panels in it: the toolbar, the activity bar and the sidebar, then
//! `panels.home` (the orchestrator page) or the Quilts page, or an agent's page, or
//! `panels.chat` with `panels.details` beside the chat and `panels.dock` under it, and the status
//! bar, the error toast and the dialogs over them all.

// Rust guideline compliant 2026-10-09

mod chat;
mod details;
pub(crate) mod dock;
mod home;
mod keys;

use gpui::{
    MouseButton, MouseMoveEvent, MouseUpEvent, Role, StyleRefinement, Window, deferred, div,
    prelude::*, px, relative,
};
use patchwork_client::AppState;
use patchwork_design_components::button;
use patchwork_design_text::display_title;
use patchwork_design_icons::icon;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{STATUSBAR_H, Theme, font_ui, fs_md, radius, radius_sm, theme};

use crate::nav::Page;
use crate::window::{SIDEBAR_MAX, SIDEBAR_MIN, Workspace};

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_workspace(window, cx)
    }
}

impl Workspace {
    fn render_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !AppState::get(cx).settings.onboarded {
            return self.onboarding_view(window, cx);
        }
        self.follow_project_look(window, cx);
        self.sync_title(window, cx);
        self.sync_mobile(cx);
        self.remember_place(cx);
        let t = *theme(cx);
        let orchestrating = self.page == Page::Orchestrator;
        let quilts = self.quilts_shown(window, cx);
        // The panel beside a chat closes with its browser once another chat is open, so a page
        // no one sees is not drawn.
        let chat = self.chat.as_ref().map(|t| &t.id);
        if self
            .quilt_beside
            .as_ref()
            .is_some_and(|(of, _)| Some(of) != chat)
        {
            self.quilt_beside = None;
            self.cool_browser_unless_shown(cx);
        }
        let chats = self.page == Page::Chats;
        let sidebar = self.sidebar_view().map(|_| {
            // A narrow window gives the sidebar at most half the room beside the activity bar,
            // so the chat keeps the rest; the width set by dragging comes back as it widens.
            let room =
                f32::from(window.viewport_size().width) - patchwork_design_theme::ACTIVITY_BAR_W;
            let width = px(self.sidebar_width.min((room / 2.).max(160.)));
            self.sidebar
                .clone()
                .cached(StyleRefinement::default().flex_none().w(width))
        });
        let agent_page = if chats {
            self.agent_page_view(window, cx)
        } else {
            None
        };
        // The chat's file panel and terminal belong to the chat an agent's page takes the place of.
        let beside_chat = agent_page.is_none();
        let details = self.details_panel(chats, beside_chat, window, cx);
        let panel = self.panel_area(window, cx).filter(|_| beside_chat);
        let error = self.error_toast(&t, cx);
        let quilt_chat = self.quilt_chat_card(&t, cx);
        let root = div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace");
        keys::window_keys(root, cx)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg_app)
            .font_family(font_ui())
            .text_size(px(fs_md()))
            .line_height(relative(1.5))
            .text_color(t.text)
            .on_mouse_move(cx.listener(Self::on_resize))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.dock.update(cx, |d, _| d.end_handle_drag()) {
                        cx.notify();
                    }
                    if this.resizing.take().is_some() || this.canvas_resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.toolbar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.activity_bar(cx))
                    .children(sidebar)
                    .children(orchestrating.then(|| self.home_panel(cx)))
                    .children(quilts.map(|page| self.quilts_panel(page, quilt_chat, cx)))
                    .children(agent_page)
                    .children(
                        (chats && self.agent_shown.is_none())
                            .then(|| self.chat_panel(details, panel)),
                    ),
            )
            .children(error)
            .children(self.context_menu_view(cx))
            .children(self.update_card(&t, cx))
            .children({
                let interrupted = self.dialogs.read(cx).interrupted.clone();
                interrupted
                    .read(cx)
                    .is_open()
                    .then(|| deferred(interrupted).with_priority(1))
            })
            .child(
                self.status_region.clone().cached(
                    StyleRefinement::default()
                        .flex_none()
                        .w_full()
                        .h(px(STATUSBAR_H)),
                ),
            )
            .child(self.dialogs.clone())
            .children(patchwork_design_text::selection::menu(cx))
            .into_any_element()
    }

    /// Sets the window's title to what fills it. The orchestrator page belongs to no chat, so
    /// the open one stays out of its title, and an agent's page takes the place of the chat, so
    /// its name does too.
    fn sync_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self
            .agent_shown
            .as_ref()
            .and_then(|id| AppState::get(cx).agents.iter().find(|b| &b.id == id));
        let title = match (&self.chat, agent) {
            _ if self.page == Page::Orchestrator => {
                format!("{} - Orchestrator mode", patchwork_views_actions::APP_TITLE)
            }
            _ if self.page == Page::Quilts => {
                format!("{} - Quilts", patchwork_views_actions::APP_TITLE)
            }
            (_, Some(agent)) => format!("{} - {}", patchwork_views_actions::APP_TITLE, agent.name),
            (Some(chat), None) => {
                format!(
                    "{} - {}",
                    patchwork_views_actions::APP_TITLE,
                    display_title(&chat.title)
                )
            }
            (None, None) => patchwork_views_actions::APP_TITLE.into(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
    }
}

impl Workspace {
    /// The last error an action hit, above the status bar until dismissed.
    pub(crate) fn error_toast(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let error = AppState::get(cx).error.clone()?;
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            deferred(
                div()
                    .id("error-toast")
                    .absolute()
                    .right(px(16.))
                    .bottom(px(STATUSBAR_H + 12.))
                    .max_w(px(420.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    // The surface of the app's other popups; the warning icon says it is an error.
                    .rounded(px(radius()))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .py(px(8.))
                    .pr(px(8.))
                    .pl(px(12.))
                    .text_size(px(patchwork_design_theme::fs_sm()))
                    .text_color(t.text)
                    .shadow(patchwork_design_components::popup_shadow(t))
                    .role(Role::Alert)
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.danger)
                            .child(icon(IconName::Warning, 14.)),
                    )
                    .child(div().min_w_0().flex_1().child(error.clone()))
                    // An agent that would not start is worth a bug report, so its logs are a
                    // click away; they say how long its command was and what Windows answered.
                    .when(error.starts_with("Failed to start agent"), |toast| {
                        toast.child(
                            button("save-logs", "Save logs")
                                .h(px(28.))
                                .px(px(8.))
                                .flex_none()
                                .items_center()
                                .rounded(px(radius_sm()))
                                .text_color(t.text_muted)
                                .hover(move |s| s.bg(hover).text_color(fg))
                                .child("Save logs")
                                .on_click(cx.listener(|this, _, _, cx| this.save_logs(cx))),
                        )
                    })
                    .child(
                        button("dismiss-error", "Dismiss")
                            .size(px(28.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(radius_sm()))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon(IconName::X, 14.))
                            .on_click(cx.listener(|_, _, _, cx| {
                                AppState::set_error(cx, None);
                                cx.notify();
                            })),
                    ),
            )
            .with_priority(1),
        )
    }

    /// Asks where to save a zip of the logs, as Settings' Export logs does, and writes it there.
    /// The error stays until it is dismissed; a failure to save takes its place.
    fn save_logs(&mut self, cx: &mut Context<Self>) {
        let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S");
        let home = patchwork_platform::files::paths::home_dir().unwrap_or_default();
        let picked = cx.prompt_for_new_path(&home, Some(&format!("patchwork-logs-{stamp}.zip")));
        let core = std::sync::Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let saved = cx
                .background_spawn(async move { core.export_logs(&path) })
                .await;
            if let Err(e) = saved {
                let _ = this.update(cx, |_, cx| {
                    AppState::set_error(cx, Some(format!("Could not save the logs: {e}")));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn on_resize(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dock.read(cx).is_resizing() {
            let y = f32::from(event.position.y);
            self.dock.update(cx, |d, cx| d.drag_handle(y, window, cx));
            return cx.notify();
        }
        if let Some((start_x, start_width)) = self.canvas_resizing {
            let width = start_width - (f32::from(event.position.x) - start_x);
            self.canvas_width = Some(width);
            return cx.notify();
        }
        if let Some((start_x, start_width)) = self.resizing {
            let width = start_width + f32::from(event.position.x) - start_x;
            self.sidebar_width = width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
            cx.notify();
        }
    }
}
