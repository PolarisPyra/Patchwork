//! The window's own keys and actions: Tab and Escape, Space for the sidebar, back and forward,
//! copying, search, panes and tabs, and the pages the activity bar shows.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_chat::prelude::*;
use gpui::{MouseButton, MouseDownEvent, NavigationDirection, Stateful, prelude::*};
use gpui::Div;

use patchwork_desktop_chat::panes::{Dir, Zone};
use crate::nav::{Page, Place, View};
use crate::window::Workspace;

impl Workspace {
    /// Whether a dialog or a menu is over the window. It keeps the keys to itself.
    fn covered(&self, cx: &gpui::App) -> bool {
        self.dialogs.read(cx).covered(cx)
    }
}

/// `root` answering the window's keys and actions.
pub(super) fn window_keys(root: Stateful<Div>, cx: &mut Context<Workspace>) -> Stateful<Div> {
    root
        // Any press lets go of the row the Delete key would act on; a click on a row picks it
        // again once the press is over.
        .capture_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.sidebar.update(cx, |s, _| s.clear_clicked());
            this.core.note_activity();
        }))
        // Tab with nothing in particular focused jumps to the composer, as "Jump to the
        // composer" says; from any control it moves on.
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusNext, window, cx| {
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
                    Page::Orchestrator => {
                        this.orchestrator_tasks = !this.orchestrator_tasks;
                    }
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
        .on_action(|_: &patchwork_views_actions::FocusPrev, window, cx| window.focus_prev(cx))
        .on_action(cx.listener(|this, _: &patchwork_views_actions::GoBack, _, cx| this.go_back(cx)))
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::GoForward, _, cx| {
                this.go_forward(cx);
            }),
        )
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
            |this, _: &patchwork_design_text::selection::CopySelection, window, cx| {
                use patchwork_design_text::{
                    selection::Owner, selection::clear, selection::copy, selection::has_marks,
                    selection::owner,
                };
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
            |this, _: &patchwork_design_text::selection::SelectAllText, window, cx| {
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
                let text = crate::row_menu::chat_markdown(chat);
                patchwork_design_text::selection::select_all(text.clone(), text, window, cx);
            },
        ))
        .on_action(
            |_: &patchwork_design_text::selection::ClearSelection, window, cx| {
                // Escape still does what else it does where the keys are.
                patchwork_design_text::selection::clear(window, cx);
                cx.propagate();
            },
        )
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::TogglePalette, window, cx| {
                this.toggle_palette(window, cx);
            },
        ))
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::NewChat, window, cx| {
                let project = this.project(cx).map(|p| p.id.clone());
                this.new_chat_in(project, None, window, cx);
            }),
        )
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::FindInChat, window, cx| {
                if this.chat.is_some() {
                    this.open_find(window, cx);
                }
            },
        ))
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::CloseChatTab, _, cx| {
                if let Some(id) = this.chat.as_ref().map(|c| c.id.clone()) {
                    this.close_tab(&id, cx);
                }
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::SplitPaneRight, _, cx| {
                this.split_pane(Zone::Right, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::SplitPaneDown, _, cx| {
                this.split_pane(Zone::Bottom, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPaneLeft, _, cx| {
                this.focus_pane_toward(Dir::Left, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPaneRight, _, cx| {
                this.focus_pane_toward(Dir::Right, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPaneUp, _, cx| {
                this.focus_pane_toward(Dir::Up, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPaneDown, _, cx| {
                this.focus_pane_toward(Dir::Down, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPane1, _, cx| {
                this.focus_pane_number(1, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPane2, _, cx| {
                this.focus_pane_number(2, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPane3, _, cx| {
                this.focus_pane_number(3, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::FocusPane4, _, cx| {
                this.focus_pane_number(4, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::NextTab, _, cx| {
                this.step_tab(false, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::PrevTab, _, cx| {
                this.step_tab(true, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::ReopenTab, _, cx| {
                this.reopen_tab(cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::MoveTabLeft, _, cx| {
                this.move_tab_toward(Dir::Left, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::MoveTabRight, _, cx| {
                this.move_tab_toward(Dir::Right, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::MoveTabUp, _, cx| {
                this.move_tab_toward(Dir::Up, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::MoveTabDown, _, cx| {
                this.move_tab_toward(Dir::Down, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::SwapPaneLeft, _, cx| {
                this.swap_pane_toward(Dir::Left, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::SwapPaneRight, _, cx| {
                this.swap_pane_toward(Dir::Right, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::SwapPaneUp, _, cx| {
                this.swap_pane_toward(Dir::Up, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::SwapPaneDown, _, cx| {
                this.swap_pane_toward(Dir::Down, cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::MaximizePane, _, cx| {
                this.toggle_maximized(cx);
            }),
        )
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::OpenSettings, window, cx| {
                if this.dialogs.read(cx).settings.is_none() {
                    this.open_settings(None, window, cx);
                }
            },
        ))
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::ShowOrchestrator, window, cx| {
                // Typing then goes to the pill, as after a click on its icon.
                window.focus(&this.focus, cx);
                this.go_to(&Place::Orchestrator, cx);
            },
        ))
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::ShowTasks, _, cx| {
                this.view = Some(View::Tasks);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::ShowChanges, _, cx| {
                this.view = Some(View::Changes);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::ShowIssues, _, cx| {
                this.view = Some(View::Issues);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::ShowQuilts, _, cx| {
                this.show_quilts_page(cx);
            }),
        )
        .on_action(
            cx.listener(|this, _: &patchwork_views_actions::ShowArchived, _, cx| {
                this.view = Some(View::Archived);
                this.page = Page::Chats;
                this.peeking = false;
                cx.notify();
            }),
        )
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::ToggleTerminal, window, cx| {
                this.toggle_terminal(window, cx);
            },
        ))
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::NewTerminal, window, cx| {
                this.new_terminal(window, cx);
            },
        ))
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::TogglePanel, window, cx| {
                this.toggle_panel(window, cx);
            },
        ))
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::ToggleOutput, window, cx| {
                this.toggle_output(window, cx);
            },
        ))
        .on_action(cx.listener(
            |this, _: &patchwork_views_actions::DeleteChat, window, cx| {
                if !this.covered(cx) {
                    this.delete_selected(window, cx);
                }
            },
        ))
}
