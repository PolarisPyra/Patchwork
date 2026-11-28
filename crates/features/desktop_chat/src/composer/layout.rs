//! The composer drawn, as the web redesign's frame: an outer surface holding the workspace row,
//! and inside it a lighter card holding the attachments, the draft and the footer.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use gpui::{
    ExternalPaths, Focusable, KeyDownEvent, MouseButton, Role, SharedString, Window, div,
    prelude::*, px,
};
use patchwork_design_editor::Paste;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs, fs_xs, theme};
use patchwork_design_components::button;
use patchwork_design_text::format_tokens;
use patchwork_design_icons::icon;

use super::draft::{Composer, PLACEHOLDER};
use super::footer::FooterState;
use super::access_picker::access_of;
use super::menus::{Menu, compact_label};
use super::model_picker::effort_name;
use super::send::queue_by_default;
use super::workspace_row::WorkspaceState;
use patchwork_views_chat::attachment_chip;
use crate::host::ChatEvent;

/// The height of a line of the draft: the web's 15px at `leading-[1.45]`.
const LINE: f32 = 22.;
/// The widest the composer grows, as the web's `max-w-[760px]`.
const MAX_W: f32 = 760.;

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_composer(window, cx)
    }
}

impl Composer {
    fn render_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let t = *theme(cx);
        let host = std::rc::Rc::clone(&self.host);
        let (agent, model, working, vision_set) = {
            let picks = self.picks(cx);
            let working = host
                .chat(cx)
                .is_some_and(patchwork_client::local::core::chats::Chat::is_replying);
            (
                picks.agent,
                picks.model,
                working,
                AppState::get(cx).settings.vision.is_some(),
            )
        };
        // A reply ending may have changed what the model's server serves.
        if working != self.was_working {
            self.recheck_model();
        }
        self.check_model(agent, model.as_deref(), working, cx);
        let notice = self.model_notice(agent, model.as_deref(), vision_set, &t, cx);
        let worktree_parts = host.worktree(&t, cx);
        let (worktree, worktree_files) = (worktree_parts.strip, worktree_parts.files);
        let (worktree_pick, worktree_told) = (worktree_parts.pick, worktree_parts.told);
        let usage = host
            .chat(cx)
            .filter(|t| AppState::get(cx).settings.tokens.show_usage && !t.messages.is_empty())
            .map(|chat| {
                patchwork_views_chat::usage::usage_line(
                    chat,
                    AppState::get(cx).settings.tokens.warn_above,
                    &t,
                )
            });
        let picks = self.picks(cx);
        let footer_access = picks.access;
        let folder_missing = host.folder_missing(cx);
        let provider_commands = host
            .chat(cx)
            .map(|t| t.slash_commands.clone())
            .unwrap_or_default();
        let chat = host.chat(cx);
        let agent = picks.agent;
        let model_label = self
            .models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id == picks.model))
            .map(|m| m.label.clone())
            .or_else(|| picks.model.clone())
            .unwrap_or_else(|| "Default".into());
        let levels = agent.model_efforts(picks.model.as_deref(), self.models_of(agent));
        // A model that takes no level has none to show beside its name.
        let effort = (!levels.is_empty()).then(|| {
            effort_name(
                picks
                    .effort
                    .as_deref()
                    .filter(|e| levels.iter().any(|l| l == e)),
            )
        });
        let effective = picks.compact.unwrap_or(picks.fallback_window);
        let full = self
            .models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id == picks.model))
            .and_then(|m| m.context_window)
            .or_else(|| chat.and_then(patchwork_client::local::core::chats::Chat::context_window));
        let limit = if effective > 0 { Some(effective) } else { full };
        // How full the context is against where it compacts, as the footer's ring.
        let context = limit.map(|limit| {
            let tokens = chat
                .and_then(patchwork_client::local::core::chats::Chat::current_context)
                .unwrap_or(0);
            let detail = format!(
                "Context: {} of {} tokens before it compacts",
                format_tokens(tokens),
                format_tokens(limit),
            );
            (tokens as f32 / limit as f32, tokens, detail)
        });
        let busy = chat.is_some_and(patchwork_client::local::core::chats::Chat::is_busy);
        if working != self.was_working {
            self.was_working = working;
            self.queue_next = None;
            if self.menu == Some(Menu::Send) {
                self.menu = None;
            }
        }
        let editing = host.editing_queued(cx);
        let queue_next = self
            .queue_next
            .unwrap_or_else(|| queue_by_default(host.as_ref(), cx));
        // The send menu checks what the button does.
        self.queues = queue_next;
        let can_compact = chat.is_some_and(|t| !t.messages.is_empty()) && !working;
        let row = WorkspaceState {
            project: host.project(cx).map(|p| (p.name.clone(), p.path.clone())),
            worktree_pick,
            worktree,
        };
        let editor = self.editor.read(cx);
        let empty = editor.content.trim().is_empty() && self.attachments.is_empty();
        let editor_focus = editor.focus_handle(cx);
        let editor_scroll = editor.scroll.clone();
        // While a turn runs, what is written steers it or waits for it, as the send button says.
        let placeholder = match (working, queue_next) {
            (false, _) => PLACEHOLDER,
            (true, false) => "Steer the reply…",
            (true, true) => "Queue the next message…",
        };
        if editor.placeholder.as_ref() != placeholder {
            self.editor.update(cx, |e, cx| {
                e.placeholder = SharedString::new_static(placeholder);
                cx.notify();
            });
        }
        let attachments: Vec<_> = self
            .attachments
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let (hover, fg) = (t.bg_hover, t.text);
                attachment_chip(format!("attachment-{i}"), a, &t).child(
                    button(
                        SharedString::from(format!("remove-{i}")),
                        format!("Remove {}", a.name),
                    )
                    .size(px(16.))
                    .justify_center()
                    .rounded_full()
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .child(icon(IconName::X, 10.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if i < this.attachments.len() {
                            this.attachments.remove(i);
                        }
                        // The button pressed is gone; the keyboard carries on in the editor.
                        window.focus(&this.editor.focus_handle(cx), cx);
                        cx.notify();
                    })),
                )
            })
            .collect();
        let access = access_of(footer_access);
        let footer = FooterState {
            picks,
            model_label,
            effort,
            context,
            compact: compact_label(effective),
            access,
            working,
            busy,
            editing,
            empty,
            folder_missing,
            queue_next,
            can_compact,
        };
        let row = self.workspace_row(row, &footer.picks, &t, window, cx);
        let footer = self.footer(&footer, &t, window, cx);
        let sigil_menu = self.sigil_menu(agent, &provider_commands, &t, window, cx);
        // What the message attaches, at the top of the card above the draft.
        let attached = (!attachments.is_empty()).then(|| {
            div()
                .id("composer-attachments")
                .role(Role::List)
                .aria_label("Attachments")
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .px(px(8.))
                .pt(px(8.))
                .children(attachments)
        });
        let card = div()
            .relative()
            .flex()
            .flex_col()
            .cursor_text()
            .rounded(px(16.))
            .border_1()
            .border_color(t.text.opacity(0.07))
            .bg(t.text.opacity(0.045))
            .children(sigil_menu)
            .children(attached)
            .child(
                div()
                    .id("composer-editor")
                    .w_full()
                    .min_w_0()
                    .min_h(px(44.))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .track_scroll(&editor_scroll)
                    .px(px(12.))
                    .pt(px(10.))
                    .pb(px(2.))
                    .text_size(px(fs(15.)))
                    .line_height(px(LINE))
                    // The field is only as tall as its text; a right-click on the box around it
                    // opens the field's Edit menu too.
                    .on_mouse_down(MouseButton::Right, {
                        let editor = self.editor.clone();
                        move |e, window, cx| {
                            editor.update(cx, |ed, cx| ed.open_menu(e, window, cx));
                        }
                    })
                    .child(self.editor.clone()),
            )
            .child(footer);
        let frame = div()
            .id("composer-frame")
            .relative()
            .flex()
            .flex_col()
            .rounded(px(20.))
            .border_1()
            .border_color(t.text.opacity(0.055))
            .bg(t.bg_editor.opacity(0.82))
            .px(px(3.))
            .pb(px(3.))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let keys = &e.keystroke;
                let m = &keys.modifiers;
                if keys.key == "escape" {
                    if this.sigil_escape(window, cx) {
                        return cx.stop_propagation();
                    }
                    // As the web's: Escape in the draft stops a running reply.
                    let running = this
                        .host
                        .chat(cx)
                        .is_some_and(patchwork_client::local::core::chats::Chat::is_replying);
                    if running && this.editor.focus_handle(cx).is_focused(window) {
                        cx.emit(ChatEvent::Stop);
                        cx.stop_propagation();
                    }
                    return;
                }
                if keys.key == "up" && m.alt && !m.control && !m.shift && !m.platform {
                    // Handled once this update is over: it reads this composer, which cannot be
                    // read from in here.
                    cx.emit(ChatEvent::EditLatestQueued);
                    return cx.stop_propagation();
                }
                // The web's formatting keys, which the "+" menu lists beside Bold and Italic.
                let plain = (m.control || m.platform) && !m.alt && !m.shift;
                if plain && this.editor.focus_handle(cx).is_focused(window) {
                    let marks = match keys.key.as_str() {
                        "b" => "**",
                        "i" => "*",
                        _ => return,
                    };
                    this.format(marks, marks, window, cx);
                    cx.stop_propagation();
                }
            }))
            // A picture or a file on the clipboard is attached; text is the editor's to paste.
            .capture_action(cx.listener(|this, _: &Paste, _, cx| {
                if this.paste_attachments(cx) {
                    cx.stop_propagation();
                }
            }))
            // Files, folders or pictures held over the chat box light it up; dropped, they are
            // attached to the message.
            .drag_over::<ExternalPaths>({
                let (accent, glow) = (t.accent, t.bg_hover);
                move |style, _, _, _| style.border_color(accent).bg(glow)
            })
            .on_drop(cx.listener(|this, files: &ExternalPaths, window, cx| {
                // A drop is the start of a message: the usage popover goes, as a press outside
                // it would close it, and the keys go to the chat box.
                cx.emit(ChatEvent::DismissUsage);
                this.attach_paths(files.paths(), cx);
                this.focus(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                // A control, or the editor itself, has already answered the press.
                if window.default_prevented() {
                    return;
                }
                if !editor_focus.is_focused(window) {
                    window.focus(&editor_focus, cx);
                }
                // Without this the workspace, which is focusable too, takes the focus back.
                window.prevent_default();
            })
            .child(row)
            .child(card);
        // The token use, on the line just above the frame.
        let usage = usage.map(|usage| {
            div()
                .mb(px(6.))
                .px(px(16.))
                .flex()
                .min_w_0()
                .text_size(px(fs_xs()))
                .line_height(px(16.))
                .child(usage)
        });
        div()
            .id("composer")
            .role(Role::Group)
            .aria_label("Composer")
            .w_full()
            .max_w(px(MAX_W))
            .mx_auto()
            .flex()
            .flex_col()
            .children(notice)
            .children(worktree_told)
            .children(worktree_files)
            .children(usage)
            .child(frame)
            .into_any_element()
    }
}
