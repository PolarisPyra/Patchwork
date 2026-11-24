//! The window: its regions laid out, the error toast, and resizing.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::{
    Animation, AnimationExt, ease_out_quint, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, NavigationDirection, Role, StyleRefinement, Window, deferred, div, prelude::*,
    px, relative,
};
use crate::chat::backdrop::quilts_scheme;
use crate::chat::text_colors::in_scheme;
use patchwork_ui::controls::menu::Menu;
use patchwork_ui::theme::{font_ui, fs_md, radius, radius_sm, STATUSBAR_H, Theme, theme};
use patchwork_ui::{button, display_title, icon};
use super::window::{Page, Place, SIDEBAR_MAX, SIDEBAR_MIN, View, Workspace};
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
        // an agent's page takes the place of the chat, so its name does too.
        let agent = self
            .agent_shown
            .as_ref()
            .and_then(|id| self.agents.iter().find(|b| &b.id == id));
        let title = match (&self.chat, agent) {
            _ if self.page == Page::Orchestrator => {
                format!("{} - Orchestrator mode", patchwork_ui::APP_TITLE)
            }
            _ if self.page == Page::Quilts => format!("{} - Quilts", patchwork_ui::APP_TITLE),
            (_, Some(agent)) => format!("{} - {}", patchwork_ui::APP_TITLE, agent.name),
            (Some(chat), None) => {
                format!(
                    "{} - {}",
                    patchwork_ui::APP_TITLE,
                    display_title(&chat.title)
                )
            }
            (None, None) => patchwork_ui::APP_TITLE.into(),
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
        let quilts = if self.page == Page::Quilts {
            Some(self.quilts_page(window, cx))
        } else {
            if self.quilts.take().is_some() {
                self.cool_browser_unless_shown(cx);
            }
            self.quilt_chat = false;
            None
        };
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
            let width = px(self.sidebar_width);
            self.sidebar_region
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
        let agent_details = if beside_chat && chats {
            self.agent_details_view(window, cx)
        } else {
            None
        };
        let canvas = self.canvas_area(window, cx).filter(|_| beside_chat);
        let panel = self.panel_area(window, cx).filter(|_| beside_chat);
        let credentials = self.credentials.clone();
        let credential_dialog = credentials
            .read(cx)
            .is_open()
            .then(|| deferred(credentials).with_priority(4));
        self.time_error(cx);
        let error = self.error_toast(&t, cx);
        let quilt_chat = self.quilt_chat_card(&t, cx);
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
            .on_action(
                cx.listener(|this, _: &patchwork_ui::FocusNext, window, cx| {
                    let composer_shown = this.composer_shown();
                    if this.page == Page::Orchestrator && this.focus.is_focused(window) {
                        this.orchestrator.update(cx, |o, cx| o.focus(window, cx));
                    } else if composer_shown && this.focus.is_focused(window) {
                        let editor = this.composer.read(cx).editor.clone();
                        window.focus(&gpui::Focusable::focus_handle(editor.read(cx), cx), cx);
                    } else {
                        window.focus_next(cx);
                    }
                }),
            )
            // Escape that nothing closed with it, such as a menu or a dialog, takes the focus off
            // whatever has it: the chat box, a field, a button. Tab brings the chat box back.
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                // Escape from the chat's card, once its message box has let it go, closes it.
                if e.keystroke.key == "escape" && this.quilt_chat && !this.covered(cx) {
                    this.close_quilt_chat(window, cx);
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
                if !this.focus.is_focused(window) || this.covered(cx) {
                    return;
                }
                let keys = &e.keystroke;
                let held = keys.modifiers;
                if keys.key == "space" && !held.modified() {
                    match this.page {
                        Page::Orchestrator => this.orchestrator_tasks = !this.orchestrator_tasks,
                        Page::Quilts => {}
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
            .on_action(|_: &patchwork_ui::FocusPrev, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|this, _: &patchwork_ui::GoBack, _, cx| this.go_back(cx)))
            .on_action(cx.listener(|this, _: &patchwork_ui::GoForward, _, cx| this.go_forward(cx)))
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
                |this, _: &patchwork_ui::text::selection::CopySelection, window, cx| {
                    use patchwork_ui::text::selection::{Owner, clear, copy, has_marks, owner};
                    let chat = owner(cx) == Owner::Chat && has_marks(cx);
                    // The terminal has its own Ctrl+C: it copies or interrupts.
                    if this.dock.read(cx).terminal_focused(window) {
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
                |this, _: &patchwork_ui::text::selection::SelectAllText, window, cx| {
                    // The terminal takes Ctrl+A for the start of the line.
                    let Some(chat) = &this.chat else {
                        cx.propagate();
                        return;
                    };
                    if this.dock.read(cx).terminal_focused(window) {
                        cx.propagate();
                        return;
                    }
                    // The whole chat is more than is drawn: it copies as its transcript.
                    let text = crate::sidebar::chat_markdown(chat);
                    patchwork_ui::text::selection::select_all(text.clone(), text, window, cx);
                },
            ))
            .on_action(
                |_: &patchwork_ui::text::selection::ClearSelection, window, cx| {
                    // Escape still does what else it does where the keys are.
                    patchwork_ui::text::selection::clear(window, cx);
                    cx.propagate();
                },
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui::TogglePalette, window, cx| {
                    this.toggle_palette(window, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &patchwork_ui::NewChat, window, cx| {
                let project = this.project().map(|p| p.id.clone());
                this.new_chat_in(project, None, window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &patchwork_ui::FindInChat, window, cx| {
                    if this.chat.is_some() {
                        this.open_find(window, cx);
                    }
                }),
            )
            .on_action(cx.listener(|this, _: &patchwork_ui::CloseChatTab, _, cx| {
                if let Some(id) = this.chat.as_ref().map(|c| c.id.clone()) {
                    this.close_tab(&id, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &patchwork_ui::OpenSettings, window, cx| {
                    if this.settings_dialog.is_none() {
                        this.open_settings(None, window, cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui::ShowOrchestrator, window, cx| {
                    // Typing then goes to the pill, as after a click on its icon.
                    window.focus(&this.focus, cx);
                    this.go_to(&Place::Orchestrator, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &patchwork_ui::ShowTasks, _, cx| {
                this.view = Some(View::Tasks);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &patchwork_ui::ShowChanges, _, cx| {
                this.view = Some(View::Changes);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &patchwork_ui::ShowQuilts, _, cx| {
                this.show_quilts_page(cx);
            }))
            .on_action(cx.listener(|this, _: &patchwork_ui::ShowArchived, _, cx| {
                this.view = Some(View::Archived);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &patchwork_ui::ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui::NewTerminal, window, cx| {
                    this.new_terminal(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui::TogglePanel, window, cx| {
                    this.toggle_panel(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui::ToggleOutput, window, cx| {
                    this.toggle_output(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui::DeleteChat, window, cx| {
                    if !this.covered(cx) {
                        this.delete_selected(window, cx);
                    }
                }),
            )
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
                    .children(quilts.map(|page| {
                        // Over its background and in that background's scheme, as a chat is. Cached,
                        // so a backdrop that moves redraws only itself, not the gallery over it.
                        let page = div().flex().flex_1().min_w_0().min_h_0().child(
                            page.cached(StyleRefinement::default().flex_1().min_w_0().min_h_0()),
                        );
                        let page = match quilts_scheme(self) {
                            Some(scheme) => in_scheme(scheme, page).into_any_element(),
                            None => page.into_any_element(),
                        };
                        div()
                            .relative()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                self.quilts_backdrop_region
                                    .clone()
                                    .cached(StyleRefinement::default().absolute().inset_0()),
                            )
                            .child(page)
                            .children(quilt_chat)
                    }))
                    .children(agent_page)
                    .children((chats && self.agent_shown.is_none()).then(|| {
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
                                    .children(agent_details)
                                    .children(canvas),
                            )
                            .children(panel)
                    })),
            )
            .children(error)
            .children(self.update_card(&t, cx))
            .when(self.interrupted.read(cx).is_open(), |root| {
                root.child(deferred(self.interrupted.clone()).with_priority(1))
            })
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
            .children(
                self.background_dialog
                    .as_ref()
                    .map(|(_, d)| deferred(d.clone()).with_priority(2)),
            )
            .children(
                self.granular_dialog
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(credential_dialog)
            .children(
                self.machine_menu
                    .as_ref()
                    .map(|m| Menu::view(m, bars::machine_menu_at())),
            )
            .children(patchwork_ui::text::selection::menu(cx))
            .into_any_element()
    }
}

impl Workspace {
    /// The chat over its background, each its own cached view: a moving backdrop redraws only
    /// itself and reuses the chat's last frame.
    pub(crate) fn chat_layers(&self) -> gpui::Div {
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
    /// Starts a chat asking for a new quilt, or opens the chat a quilt came from.
    pub(crate) fn on_quilts(
        &mut self,
        _: &gpui::Entity<patchwork_pages::quilts::QuiltsPage>,
        event: &patchwork_pages::quilts::QuiltsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            patchwork_pages::quilts::QuiltsEvent::OpenChat(id) => {
                let same = self.chat.as_ref().is_some_and(|t| &t.id == id);
                if self.quilt_chat && same {
                    self.close_quilt_chat(window, cx);
                } else {
                    self.open_quilt_chat(id, window, cx);
                }
            }
            patchwork_pages::quilts::QuiltsEvent::Closed => {
                self.quilt_beside = None;
                self.cool_browser_unless_shown(cx);
                window.focus(&self.focus, cx);
            }
            patchwork_pages::quilts::QuiltsEvent::OpenFull { id, n } => {
                self.quilt_beside = None;
                self.open_quilt(id.clone(), *n, window, cx);
            }
            patchwork_pages::quilts::QuiltsEvent::Shown(open) => {
                if self.page == Page::Quilts && self.quilt_open != *open {
                    let from = self.place();
                    self.quilt_open.clone_from(open);
                    self.went_from(from);
                }
            }
            patchwork_pages::quilts::QuiltsEvent::AskFix { chat_id, text } => {
                if self.page == Page::Quilts {
                    self.open_quilt_chat(chat_id, window, cx);
                } else {
                    self.open_chat(chat_id, cx);
                }
                if self.chat.as_ref().is_some_and(|t| &t.id == chat_id) {
                    self.send(text.clone(), Vec::new(), None, None, cx);
                }
            }
        }
        cx.notify();
    }

    /// Whether a dialog or a menu is over the window. It keeps the keys to itself.
    fn covered(&self, cx: &gpui::App) -> bool {
        self.palette.is_some()
            || self.settings_dialog.is_some()
            || self.background_dialog.is_some()
            || self.granular_dialog.is_some()
            || self.credentials.read(cx).is_open()
            || self.add_project.is_some()
            || self.machine_menu.is_some()
    }

    /// The last error an action hit, above the status bar until dismissed.
    pub(crate) fn error_toast(
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
                    .text_size(px(patchwork_ui::theme::fs_sm()))
                    .shadow(patchwork_ui::popup_shadow(t))
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
                let slide = !patchwork_ui::theme::applied(cx).reduce_motion;
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

    pub(crate) fn on_resize(
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
