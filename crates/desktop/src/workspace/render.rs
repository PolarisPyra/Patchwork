//! The window: its regions laid out, the error toast, and resizing.

// Rust guideline compliant 2026-10-09

use gpui::{
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection, Role,
    StyleRefinement, Window, deferred, div, prelude::*, px, relative,
};
use crate::ui::menu::Menu;
use crate::ui::theme::{FONT_UI, FS_MD, RADIUS, RADIUS_SM, STATUSBAR_H, Theme, theme};
use crate::ui::{button, display_title, icon};
use super::state::{Page, SIDEBAR_MAX, SIDEBAR_MIN, View, Workspace};
use super::bars;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.settings.onboarded {
            return self.onboarding_view(window, cx);
        }
        let title = match &self.thread {
            Some(thread) => format!("{} - {}", crate::APP_TITLE, display_title(&thread.title)),
            None => crate::APP_TITLE.into(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        self.sync_mobile(cx);
        let t = *theme(cx);
        let orchestrating = self.page == Page::Orchestrator;
        let sidebar = self.sidebar_view().map(|_| {
            let width = px(self.sidebar_width);
            self.sidebar_region
                .clone()
                .cached(StyleRefinement::default().flex_none().w(width))
        });
        let canvas = self.canvas_area(window, cx);
        let panel = self.panel_area(window, cx);
        let background_dialog = self.background_dialog(window, cx);
        let granular_dialog = self.granular_dialog(window, cx);
        let error = self.error_toast(&t, cx);
        div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace")
            // Any press lets go of the row the Delete key would act on; a click on a row picks it
            // again once the press is over.
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, _| {
                this.picking.clicked = None;
            }))
            // Tab with nothing in particular focused jumps to the composer, as "Jump to the
            // composer" says; from any control it moves on.
            .on_action(cx.listener(|this, _: &crate::FocusNext, window, cx| {
                let composer_shown = this.thread.as_ref().is_none_or(|t| t.parent.is_none());
                if this.page == Page::Orchestrator && this.focus.is_focused(window) {
                    this.orchestrator.update(cx, |o, cx| o.focus(window, cx));
                } else if composer_shown && this.focus.is_focused(window) {
                    let editor = this.composer.read(cx).editor.clone();
                    window.focus(&gpui::Focusable::focus_handle(editor.read(cx), cx), cx);
                } else {
                    window.focus_next(cx);
                }
            }))
            // Escape that nothing closed with it, such as a menu or a dialog, takes the focus off
            // whatever has it: the chat box, a field, a button. Tab brings the chat box back.
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" && !this.focus.is_focused(window) {
                    window.focus(&this.focus, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            // With nothing in particular focused, such as after a click on the chat, Space opens
            // and closes the sidebar. On the orchestrator page any other character starts a
            // request in the pill.
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if !this.focus.is_focused(window) || this.covered() {
                    return;
                }
                let keys = &e.keystroke;
                let held = keys.modifiers;
                if keys.key == "space" && !held.modified() {
                    match this.page {
                        Page::Orchestrator => this.orchestrator_tasks = !this.orchestrator_tasks,
                        Page::Chats => {
                            this.view = if this.view.is_some() {
                                None
                            } else {
                                Some(View::Tasks)
                            };
                        }
                    }
                    cx.stop_propagation();
                    cx.notify();
                } else if this.page == Page::Orchestrator
                    && let Some(typed) = keys.key_char.as_deref()
                    && !(held.control || held.alt || held.platform || held.function)
                    && !typed.is_empty()
                    && !typed.chars().any(char::is_control)
                {
                    this.orchestrator
                        .update(cx, |o, cx| o.type_text(typed, window, cx));
                    cx.stop_propagation();
                }
            }))
            .on_action(|_: &crate::FocusPrev, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|this, _: &crate::GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &crate::GoForward, _, cx| this.go_forward(cx)))
            // The mouse's side buttons go back and forward, as in a browser.
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.go_back(cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.go_forward(cx)),
            )
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
                this.page = Page::Chats;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowChanges, _, cx| {
                this.view = Some(View::Changes);
                this.page = Page::Chats;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowArchived, _, cx| {
                this.view = Some(View::Archived);
                this.page = Page::Chats;
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
            .on_action(cx.listener(|this, _: &crate::DeleteChat, window, cx| {
                if !this.covered() {
                    this.delete_selected(window, cx);
                }
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
            .child(self.toolbar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.activity_bar(cx))
                    .children(sidebar)
                    .children(orchestrating.then(|| {
                        self.orchestrator
                            .clone()
                            .cached(StyleRefinement::default().flex_1().min_w_0().min_h_0())
                    }))
                    .children((!orchestrating).then(|| {
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
                            .children(panel)
                    })),
            )
            .children(error)
            .children(self.update_card(&t, cx))
            .children(self.interrupted_card(&t, cx))
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
            .children(granular_dialog)
            .children(
                self.machine_menu
                    .as_ref()
                    .map(|m| Menu::view(m, bars::machine_menu_at())),
            )
            .children(crate::ui::selection::menu(cx))
            .into_any_element()
    }
}

impl Workspace {
    /// Whether a dialog or a menu is over the window. It keeps the keys to itself.
    fn covered(&self) -> bool {
        self.palette.is_some()
            || self.settings_dialog.is_some()
            || self.background_dialog.is_some()
            || self.granular_dialog.is_some()
            || self.add_project.is_some()
            || self.machine_menu.is_some()
    }

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
