//! The window: its regions laid out, the error toast, and resizing.

// Rust guideline compliant 2026-10-09

use gpui::{
    MouseButton, MouseMoveEvent, MouseUpEvent, Role, StyleRefinement, Window, deferred, div,
    prelude::*, px, relative,
};
use crate::ui::menu::Menu;
use crate::ui::theme::{FONT_UI, FS_MD, RADIUS, RADIUS_SM, STATUSBAR_H, Theme, theme};
use crate::ui::{button, display_title, icon};
use super::state::{SIDEBAR_MAX, SIDEBAR_MIN, View, Workspace};
use super::bars;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.settings.onboarded {
            return self.onboarding_view(window, cx);
        }
        let title = match &self.thread {
            Some(thread) => format!("Patchwork - {}", display_title(&thread.title)),
            None => "Patchwork".into(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        let t = *theme(cx);
        let sidebar = self.view.map(|_| {
            let width = px(self.sidebar_width);
            self.sidebar_region
                .clone()
                .cached(StyleRefinement::default().flex_none().w(width))
        });
        let canvas = self.canvas_area(window, cx);
        let panel = self.panel_area(window, cx);
        let background_dialog = self.background_dialog(window, cx);
        let error = self.error_toast(&t, cx);
        div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_action(|_: &crate::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::FocusPrev, window, cx| window.focus_prev(cx))
            .on_action(|_: &crate::ui::selection::CopySelection, _, cx| {
                crate::ui::selection::copy(cx);
            })
            .on_action(cx.listener(|this, _: &crate::TogglePalette, window, cx| {
                this.toggle_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::NewChat, window, cx| {
                let project = this.project().map(|p| p.id.clone());
                this.new_thread_in(project, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::FindInChat, window, cx| {
                if this.thread.is_some() {
                    this.open_find(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::OpenSettings, window, cx| {
                if this.settings_dialog.is_none() {
                    this.open_settings(None, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::ShowTasks, _, cx| {
                this.view = Some(View::Tasks);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowChanges, _, cx| {
                this.view = Some(View::Changes);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowArchived, _, cx| {
                this.view = Some(View::Archived);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ToggleTerminal, window, cx| {
                this.toggle_terminal(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::NewTerminal, window, cx| {
                this.new_terminal(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::TogglePanel, window, cx| {
                this.toggle_panel(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::ToggleOutput, window, cx| {
                this.toggle_output(window, cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg_app)
            .font_family(FONT_UI)
            .text_size(px(FS_MD))
            .line_height(relative(1.5))
            .text_color(t.text)
            .on_mouse_move(cx.listener(Self::on_resize))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.panel_resizing.take().is_some() {
                        this.keep_panel_height();
                        cx.notify();
                    }
                    if this.resizing.take().is_some() || this.canvas_resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.activity_bar(cx))
                    .children(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .min_h_0()
                                    .child(self.chat_region.clone().cached(
                                        StyleRefinement::default().flex_1().min_w_0().min_h_0(),
                                    ))
                                    .children(canvas),
                            )
                            .children(panel),
                    ),
            )
            .children(error)
            .children(self.update_card(&t, cx))
            .child(
                self.status_region.clone().cached(
                    StyleRefinement::default()
                        .flex_none()
                        .w_full()
                        .h(px(STATUSBAR_H)),
                ),
            )
            .children(self.palette.clone().map(|p| deferred(p).with_priority(2)))
            .children(
                self.settings_dialog
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(
                self.add_project
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(background_dialog)
            .children(
                self.project_menu
                    .as_ref()
                    .map(|m| Menu::view(m, bars::project_menu_at())),
            )
            .into_any_element()
    }
}

impl Workspace {
    /// The last error an action hit, above the status bar until dismissed.
    pub(super) fn error_toast(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let error = self.error.clone()?;
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
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(t.danger)
                    .bg(t.bg_elevated)
                    .py(px(8.))
                    .pr(px(8.))
                    .pl(px(12.))
                    .text_size(px(crate::ui::theme::FS_SM))
                    .shadow(crate::ui::popup_shadow(t))
                    .role(Role::Alert)
                    .child(div().min_w_0().child(error))
                    .child(
                        button("dismiss-error", "Dismiss")
                            .size(px(28.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("x", 14.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = None;
                                cx.notify();
                            })),
                    ),
            )
            .with_priority(1),
        )
    }

    pub(super) fn on_resize(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.panel_resizing.is_some() {
            return self.drag_panel(f32::from(event.position.y), window, cx);
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
