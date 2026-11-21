//! The window: its regions laid out, the error toast, and resizing.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::{
    Animation, AnimationExt, ease_out_quint, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, NavigationDirection, Role, StyleRefinement, Window, deferred, div, prelude::*,
    px, relative,
};
use crate::ui::menu::Menu;
use crate::ui::theme::{font_ui, fs_md, radius, radius_sm, STATUSBAR_H, Theme, theme};
use crate::ui::{button, display_title, icon};
use super::state::{Page, Place, SIDEBAR_MAX, SIDEBAR_MIN, View, Workspace};
use super::bars;

/// How long the error toast stays before it slides away, and how long the slide takes.
const ERROR_SHOWN: Duration = Duration::from_secs(5);
const ERROR_SLIDE: Duration = Duration::from_millis(200);
/// How often a toast the mouse holds checks whether it was let go.
const ERROR_HOLD: Duration = Duration::from_millis(250);

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.settings.onboarded {
            return self.onboarding_view(window, cx);
        }
        self.follow_project_look(window, cx);
        // The orchestrator page belongs to no chat, so the open one stays out of its title, and
        // a bot's page takes the place of the chat, so its name does too.
        let bot = self
            .bot_shown
            .as_ref()
            .and_then(|id| self.bots.iter().find(|b| &b.id == id));
        let title = match (&self.thread, bot) {
            _ if self.page == Page::Orchestrator => {
                format!("{} - Orchestrator mode", crate::APP_TITLE)
            }
            _ if self.page == Page::Artifacts => format!("{} - Artifacts", crate::APP_TITLE),
            (_, Some(bot)) => format!("{} - {}", crate::APP_TITLE, bot.name),
            (Some(thread), None) => {
                format!("{} - {}", crate::APP_TITLE, display_title(&thread.title))
            }
            (None, None) => crate::APP_TITLE.into(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        self.sync_mobile(cx);
        self.remember_place(cx);
        let t = *theme(cx);
        let orchestrating = self.page == Page::Orchestrator;
        // The page and its browser are made when it shows and closed when it is left.
        let artifacts = if self.page == Page::Artifacts {
            Some(self.artifacts_page(window, cx))
        } else {
            if self.artifacts.take().is_some() {
                self.cool_browser_unless_shown(cx);
            }
            self.artifact_chat = false;
            None
        };
        // The panel beside a chat closes with its browser once another chat is open, so a page
        // no one sees is not drawn.
        let thread = self.thread.as_ref().map(|t| &t.id);
        if self
            .artifact_beside
            .as_ref()
            .is_some_and(|(of, _)| Some(of) != thread)
        {
            self.artifact_beside = None;
            self.cool_browser_unless_shown(cx);
        }
        let chats = self.page == Page::Chats;
        let sidebar = self.sidebar_view().map(|_| {
            let width = px(self.sidebar_width);
            self.sidebar_region
                .clone()
                .cached(StyleRefinement::default().flex_none().w(width))
        });
        let bot_page = if chats {
            self.bot_page_view(window, cx)
        } else {
            None
        };
        // The chat's file panel and terminal belong to the chat a bot's page takes the place of.
        let beside_chat = bot_page.is_none();
        let bot_details = if beside_chat && chats {
            self.bot_details_view(window, cx)
        } else {
            None
        };
        let canvas = self.canvas_area(window, cx).filter(|_| beside_chat);
        let panel = self.panel_area(window, cx).filter(|_| beside_chat);
        let background_dialog = self.background_dialog(window, cx);
        let granular_dialog = self.granular_dialog(window, cx);
        let credential_dialog = self.credential_dialog(cx);
        self.time_error(cx);
        let error = self.error_toast(&t, cx);
        let artifact_chat = self.artifact_chat_card(&t, cx);
        div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace")
            // Any press lets go of the row the Delete key would act on; a click on a row picks it
            // again once the press is over.
            .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, _| {
                this.picking.clicked = None;
                this.core.note_activity();
            }))
            // Tab with nothing in particular focused jumps to the composer, as "Jump to the
            // composer" says; from any control it moves on.
            .on_action(cx.listener(|this, _: &crate::FocusNext, window, cx| {
                let composer_shown = this.composer_shown();
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
                // Escape from the chat's card, once its message box has let it go, closes it.
                if e.keystroke.key == "escape" && this.artifact_chat && !this.covered() {
                    this.close_artifact_chat(window, cx);
                    cx.stop_propagation();
                    return;
                }
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
                        Page::Artifacts => {}
                        Page::Chats => {
                            let open = this.sidebar_view().is_some();
                            // Looking in on a chat from the orchestrator ends here.
                            this.peeking = false;
                            this.view = if open { None } else { Some(View::Tasks) };
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
            .on_action(cx.listener(
                |this, _: &crate::ui::selection::CopySelection, window, cx| {
                    use crate::ui::selection::{Owner, clear, copy, has_marks, owner};
                    let chat = owner(cx) == Owner::Chat && has_marks(cx);
                    // The terminal has its own Ctrl+C: it copies or interrupts.
                    if this.panel.read(cx).terminal_focused(window) {
                        if !chat {
                            cx.propagate();
                            return;
                        }
                        // As the terminal does with its own, the copied text lets go, so the
                        // next Ctrl+C interrupts.
                        copy(cx);
                        clear(window, cx);
                        return;
                    }
                    copy(cx);
                },
            ))
            .on_action(cx.listener(
                |this, _: &crate::ui::selection::SelectAllText, window, cx| {
                    // The terminal takes Ctrl+A for the start of the line.
                    let Some(thread) = &this.thread else {
                        cx.propagate();
                        return;
                    };
                    if this.panel.read(cx).terminal_focused(window) {
                        cx.propagate();
                        return;
                    }
                    // The whole chat is more than is drawn: it copies as its transcript.
                    let text = crate::sidebar::thread_markdown(thread);
                    crate::ui::selection::select_all(text.clone(), text, window, cx);
                },
            ))
            .on_action(|_: &crate::ui::selection::ClearSelection, window, cx| {
                // Escape still does what else it does where the keys are.
                crate::ui::selection::clear(window, cx);
                cx.propagate();
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
            .on_action(
                cx.listener(|this, _: &crate::ShowOrchestrator, window, cx| {
                    // Typing then goes to the pill, as after a click on its icon.
                    window.focus(&this.focus, cx);
                    this.go_to(&Place::Orchestrator, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &crate::ShowTasks, _, cx| {
                this.view = Some(View::Tasks);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowChanges, _, cx| {
                this.view = Some(View::Changes);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowArtifacts, _, cx| {
                this.page = Page::Artifacts;
                this.peeking = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowArchived, _, cx| {
                this.view = Some(View::Archived);
                this.page = Page::Chats;
                this.peeking = false;
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
            .font_family(font_ui())
            .text_size(px(fs_md()))
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
                        // The backdrop is its own view under the page, so as it moves the page's
                        // frame is reused.
                        div()
                            .relative()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                self.orchestrator_backdrop
                                    .clone()
                                    .cached(StyleRefinement::default().absolute().inset_0()),
                            )
                            .child(
                                self.orchestrator.clone().cached(
                                    StyleRefinement::default().flex_1().min_w_0().min_h_0(),
                                ),
                            )
                    }))
                    .children(artifacts.map(|page| {
                        div()
                            .relative()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(page)
                            .children(artifact_chat)
                    }))
                    .children(bot_page)
                    .children((chats && self.bot_shown.is_none()).then(|| {
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
                                    .child(self.chat_layers())
                                    .children(bot_details)
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
            // Cached, so a moving chat backdrop under them does not draw them again each frame.
            .children(
                self.palette
                    .clone()
                    .map(|p| deferred(p.cached(overlay())).with_priority(2)),
            )
            .children(
                self.settings_dialog
                    .clone()
                    .map(|d| deferred(d.cached(overlay())).with_priority(2)),
            )
            .children(
                self.add_project
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(background_dialog)
            .children(granular_dialog)
            .children(credential_dialog)
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
    /// The chat over its background, each its own cached view: a moving backdrop redraws only
    /// itself and reuses the chat's last frame.
    pub(super) fn chat_layers(&self) -> gpui::Div {
        div()
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                self.backdrop_region
                    .clone()
                    .cached(StyleRefinement::default().absolute().inset_0()),
            )
            .child(
                self.chat_region
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().min_h_0()),
            )
    }
}

impl Workspace {
    /// Starts a chat asking for a new artifact, or opens the chat an artifact came from.
    pub(super) fn on_artifacts(
        &mut self,
        _: &gpui::Entity<crate::artifacts::ArtifactsPage>,
        event: &crate::artifacts::ArtifactsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            crate::artifacts::ArtifactsEvent::OpenChat(id) => {
                let same = self.thread.as_ref().is_some_and(|t| &t.id == id);
                if self.artifact_chat && same {
                    self.close_artifact_chat(window, cx);
                } else {
                    self.open_artifact_chat(id, window, cx);
                }
            }
            crate::artifacts::ArtifactsEvent::Closed => {
                self.artifact_beside = None;
                self.cool_browser_unless_shown(cx);
                window.focus(&self.focus, cx);
            }
            crate::artifacts::ArtifactsEvent::OpenFull { id, n } => {
                self.artifact_beside = None;
                self.open_artifact(id.clone(), *n, window, cx);
            }
            crate::artifacts::ArtifactsEvent::AskFix { thread_id, text } => {
                if self.page == Page::Artifacts {
                    self.open_artifact_chat(thread_id, window, cx);
                } else {
                    self.open_thread(thread_id, cx);
                }
                if self.thread.as_ref().is_some_and(|t| &t.id == thread_id) {
                    self.send(text.clone(), Vec::new(), None, None, cx);
                }
            }
            crate::artifacts::ArtifactsEvent::MakeNew(kind) => {
                self.page = Page::Chats;
                self.peeking = false;
                self.new_chat(cx);
                self.composer.update(cx, |c, cx| {
                    c.set_draft(kind.request(), Vec::new(), cx);
                    c.focus(window, cx);
                });
            }
        }
        cx.notify();
    }

    /// Whether a dialog or a menu is over the window. It keeps the keys to itself.
    fn covered(&self) -> bool {
        self.palette.is_some()
            || self.settings_dialog.is_some()
            || self.background_dialog.is_some()
            || self.granular_dialog.is_some()
            || self.credentials.is_open()
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
        let leaving = self.error_leaving;
        Some(
            deferred(
                div()
                    .id("error-toast")
                    .on_hover(cx.listener(|this, hovered: &bool, _, _| {
                        this.error_hovered = *hovered;
                    }))
                    .absolute()
                    .right(px(16.))
                    .bottom(px(STATUSBAR_H + 12.))
                    .max_w(px(420.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(radius()))
                    .border_1()
                    .border_color(t.danger)
                    .bg(t.bg_elevated)
                    .py(px(8.))
                    .pr(px(8.))
                    .pl(px(12.))
                    .text_size(px(crate::ui::theme::fs_sm()))
                    .shadow(crate::ui::popup_shadow(t))
                    .role(Role::Alert)
                    .child(div().min_w_0().child(error))
                    .child(
                        button("dismiss-error", "Dismiss")
                            .size(px(28.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(radius_sm()))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("x", 14.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .with_animation(
                        if leaving {
                            "error-toast-leave"
                        } else {
                            "error-toast"
                        },
                        Animation::new(ERROR_SLIDE).with_easing(ease_out_quint()),
                        move |toast, delta| {
                            if leaving {
                                toast.right(px(16. - 436. * delta)).opacity(1. - delta)
                            } else {
                                toast
                            }
                        },
                    ),
            )
            .with_priority(1),
        )
    }

    /// Starts, once for each error, the timer that slides the error toast away after
    /// [`ERROR_SHOWN`]. The mouse over the toast holds it; with reduced motion it goes without
    /// sliding.
    fn time_error(&mut self, cx: &mut Context<Self>) {
        let Some(error) = self.error.clone() else {
            self.error_timer = None;
            self.error_leaving = false;
            self.error_hovered = false;
            return;
        };
        if self
            .error_timer
            .as_ref()
            .is_some_and(|(timed, _)| *timed == error)
        {
            return;
        }
        self.error_leaving = false;
        let shown = error.clone();
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ERROR_SHOWN).await;
            while this
                .update(cx, |this, _| this.error_hovered)
                .unwrap_or(false)
            {
                cx.background_executor().timer(ERROR_HOLD).await;
            }
            let slide = this.update(cx, |this, cx| {
                let slide = !crate::ui::theme::applied(cx).reduce_motion;
                this.error_leaving = slide;
                cx.notify();
                slide
            });
            if slide.unwrap_or(false) {
                cx.background_executor().timer(ERROR_SLIDE).await;
            }
            // The window closing drops the toast with it, so there is nothing to do then.
            let _ = this.update(cx, |this, cx| {
                if this.error.as_ref() == Some(&shown) {
                    this.error = None;
                    this.error_leaving = false;
                    cx.notify();
                }
            });
        });
        self.error_timer = Some((error, timer));
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

/// The window-filling place of a dialog drawn over everything.
fn overlay() -> StyleRefinement {
    StyleRefinement::default()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}
