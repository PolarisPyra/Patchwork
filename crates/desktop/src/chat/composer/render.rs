//! The composer drawn: its attachments in a pill above, the pill with the draft and the send
//! button, and the chips under it.

// Rust guideline compliant 2026-10-09

use gpui::{
    ExternalPaths, Focusable, KeyDownEvent, MouseButton, Role, SharedString, Window, div,
    prelude::*, px,
};
use crate::ui::text_area::Paste;
use crate::chat::user::attachment_chip;
use crate::ui::pill::{PILL_PAD, SEND, pill, send_button};
use crate::ui::theme::{fs_md, fs_xs, theme};
use crate::ui::{agent_logo, button, format_tokens, icon, tooltip};
use crate::workspace::{Workspace, is_busy, is_working};
use super::menus::{Menu, access_of, compact_label, effort_label, efforts};
use super::chips::{chip, context_of, fill_ring};
use super::send::queue_by_default;
use super::state::Composer;

/// The height of a line of the draft.
const LINE: f32 = 20.;

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let (agent, model, working, vision_set) = {
            let w = workspace.read(cx);
            let picks = self.picks(w);
            let working = w.thread.as_ref().is_some_and(is_working);
            (
                picks.agent,
                picks.model,
                working,
                w.settings.vision.is_some(),
            )
        };
        // A reply ending may have changed what the model's server serves.
        if working != self.was_working {
            self.recheck_model();
        }
        self.check_model(agent, model.as_deref(), working, cx);
        let notice = self.model_notice(agent, model.as_deref(), vision_set, &t, cx);
        let (worktree, worktree_files) =
            workspace.update(cx, |w, cx| crate::chat::worktree::worktree_strip(w, &t, cx));
        let w = workspace.read(cx);
        let usage = w
            .thread
            .as_ref()
            .filter(|t| w.settings.tokens.show_usage && !t.messages.is_empty())
            .map(|thread| crate::chat::usage_line(thread, w.settings.tokens.warn_above, &t));
        let picks = self.picks(w);
        let folder_missing = w.folder_missing();
        let provider_commands = w
            .thread
            .as_ref()
            .map(|t| t.slash_commands.clone())
            .unwrap_or_default();
        let thread = w.thread.as_ref();
        let agent = picks.agent;
        let model_label = self
            .models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id == picks.model))
            .map(|m| m.label.clone())
            .or_else(|| picks.model.clone())
            .unwrap_or_else(|| "Default".into());
        let levels = efforts(agent, picks.model.as_deref(), self.models_of(agent));
        let effort = picks
            .effort
            .as_ref()
            .filter(|e| levels.contains(e))
            .map_or_else(|| "Default".into(), |e| effort_label(e));
        let effective = picks.compact.unwrap_or(picks.fallback_window);
        let full = self
            .models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id == picks.model))
            .and_then(|m| m.context_window);
        let limit = if effective > 0 { Some(effective) } else { full };
        // How full the context is against where it compacts, as the Compact chip's ring.
        let context = limit.map(|limit| {
            let tokens = thread.and_then(context_of).unwrap_or(0);
            let used = tokens as f32 / limit as f32;
            let detail = format!(
                "Context: {} of {} tokens before it compacts",
                format_tokens(tokens),
                format_tokens(limit),
            );
            (fill_ring(used, &t), detail)
        });
        let compact = compact_label(effective);
        let (access, access_icon) = access_of(picks.access);
        let working = thread.is_some_and(is_working);
        let busy = thread.is_some_and(is_busy);
        if working != self.was_working {
            self.was_working = working;
            self.queue_next = None;
            if self.menu == Some(Menu::Send) {
                self.menu = None;
            }
        }
        let editing = w.editing_queued();
        let queue_next = self.queue_next.unwrap_or_else(|| queue_by_default(w));
        // The send menu checks what the button does.
        self.queues = queue_next;
        let can_compact = thread.is_some_and(|t| !t.messages.is_empty()) && !working;
        let editor = self.editor.read(cx);
        let empty = editor.content.trim().is_empty() && self.attachments.is_empty();
        let focused = editor.focus_handle(cx).is_focused(window);
        let editor_focus = editor.focus_handle(cx);
        let editor_scroll = editor.scroll.clone();
        let attachments: Vec<_> = self
            .attachments
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let (hover, fg) = (t.bg_hover, t.text);
                // The pill around them is their frame, so a chip has none of its own.
                attachment_chip(format!("attachment-{i}"), a, &t)
                    .bg(gpui::transparent_black())
                    .border_color(gpui::transparent_black())
                    .child(
                        button(
                            SharedString::from(format!("remove-{i}")),
                            format!("Remove {}", a.name),
                        )
                        .size(px(16.))
                        .justify_center()
                        .rounded_full()
                        .text_color(t.text_muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .child(icon("x", 10.))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if i < this.attachments.len() {
                                    this.attachments.remove(i);
                                }
                                // The button pressed is gone; the keyboard carries on in the editor.
                                window.focus(&this.editor.focus_handle(cx), cx);
                                cx.notify();
                            },
                        )),
                    )
            })
            .collect();
        let stop = busy.then(|| {
            let (bg, hover) = (t.text.opacity(0.14), t.text.opacity(0.22));
            button(
                "stop",
                if working {
                    "Stop"
                } else {
                    "Stop background commands"
                },
            )
            .size(px(SEND))
            .flex_none()
            .justify_center()
            .rounded_full()
            .bg(bg)
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .child(icon("square-fill", 12.))
            .on_click(cx.listener(|this, _, _, cx| {
                let _ = this.workspace.update(cx, crate::workspace::Workspace::stop);
            }))
        });
        let send_label = if editing {
            "Update queued message"
        } else if !working {
            "Send"
        } else if queue_next {
            "Queue message"
        } else {
            "Steer message"
        };
        let send_tip = if editing {
            "Update queued message"
        } else if !working {
            "Send"
        } else if queue_next {
            "Queue for after the turn"
        } else {
            "Steer the running turn"
        };
        let split = working && !editing;
        let send = (!working || !empty || editing).then(|| {
            let blocked = folder_missing || (empty && !editing);
            let base = send_button("send", send_label, !blocked, t.text, t.bg_editor)
                .tooltip(tooltip(send_tip))
                .map(|b| {
                    if split {
                        b.rounded_l_full()
                    } else {
                        b.rounded_full()
                    }
                })
                .child(icon(if editing { "check" } else { "arrow-up" }, 16.));
            if blocked {
                base
            } else {
                base.on_click(cx.listener(|this, _, window, cx| this.send(false, window, cx)))
            }
        });
        let send_mode = (split && !empty).then(|| {
            let hover = t.text.opacity(0.8);
            let caret = button("send-mode", "Choose queue or steer")
                .tooltip(tooltip("Choose queue or steer"))
                .h(px(SEND))
                .w(px(28.))
                .flex_none()
                .justify_center()
                .rounded_r_full()
                .border_l_1()
                .border_color(t.bg_editor.opacity(0.2))
                .bg(t.text)
                .text_color(t.bg_editor)
                .hover(move |s| s.bg(hover))
                .child(icon("caret-down", 12.));
            self.chip_with_menu(Menu::Send, caret, &picks, can_compact, &t, window, cx)
        });
        let model_chip = self.chip_with_menu(
            Menu::Model,
            chip(
                "model",
                format!("Choose model: {model_label}"),
                agent_logo(agent, picks.model.as_deref(), 14.),
                model_label,
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        // A model that takes no level has nothing to pick, so its chip would only take up room.
        let effort_chip = (!levels.is_empty()).then(|| {
            self.chip_with_menu(
                Menu::Effort,
                chip(
                    "effort",
                    format!("Effort: {effort}"),
                    icon("lightbulb", 14.).into_any_element(),
                    effort,
                    &t,
                ),
                &picks,
                can_compact,
                &t,
                window,
                cx,
            )
        });
        let compact_chip = match context {
            Some((ring, detail)) => chip(
                "compact",
                format!("{detail}. Compact at: {compact}"),
                ring,
                compact,
                &t,
            )
            .tooltip(tooltip(detail)),
            None => chip(
                "compact",
                format!("Compact at: {compact}"),
                icon("arrows-in-simple", 14.).into_any_element(),
                compact,
                &t,
            ),
        };
        let compact_chip = self.chip_with_menu(
            Menu::Compact,
            compact_chip,
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let access_chip = self.chip_with_menu(
            Menu::Access,
            chip(
                "access",
                format!("Access: {access}"),
                icon(access_icon, 14.).into_any_element(),
                access.into(),
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let role = picks.role.clone();
        let agent_chip = self.chip_with_menu(
            Menu::Agent,
            chip(
                "agent-role",
                match &role {
                    Some(role) => format!("Agent: {role}"),
                    None => "Agent: none".into(),
                },
                icon("robot", 14.).into_any_element(),
                role.unwrap_or_else(|| "Agent".into()),
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let attach = {
            let (shade, fg) = (t.text.opacity(0.08), t.text);
            let plus = button("attach", "Attach")
                .size(px(SEND))
                .flex_none()
                .justify_center()
                .rounded_full()
                .text_color(t.text.opacity(0.7))
                .hover(move |s| s.bg(shade).text_color(fg))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .child(icon("plus", 16.));
            self.chip_with_menu(Menu::Attach, plus, &picks, can_compact, &t, window, cx)
        };
        let format_chip = self.chip_with_menu(
            Menu::Format,
            chip(
                "format",
                "Format".into(),
                icon("text-b", 14.).into_any_element(),
                "Format".into(),
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let sigil_menu = self.sigil_menu(picks.agent, &provider_commands, &t, cx);
        let border = if focused {
            t.accent.opacity(0.6)
        } else {
            t.text.opacity(0.10)
        };
        // What the message attaches, in its own pill above the composer's.
        let attached = (!attachments.is_empty()).then(|| {
            pill(
                "composer-attachments",
                t.text.opacity(0.10),
                t.bg_editor.opacity(0.82),
                t.shadow.opacity(0.5),
            )
            .role(Role::List)
            .aria_label("Attachments")
            .min_w_0()
            .p(px(6.))
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(attachments)
        });
        let pill = pill(
            "composer-pill",
            border,
            t.bg_editor.opacity(0.82),
            t.shadow.opacity(0.5),
        )
        .relative()
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(PILL_PAD))
        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
            if e.keystroke.key == "escape" && this.sigil_escape(cx) {
                return cx.stop_propagation();
            }
            let m = &e.keystroke.modifiers;
            if e.keystroke.key == "up" && m.alt && !m.control && !m.shift && !m.platform {
                // Deferred: it reads this composer, which cannot be read from in here.
                let workspace = this.workspace.clone();
                cx.defer(move |cx| {
                    let _ = workspace.update(cx, crate::workspace::Workspace::edit_latest_queued);
                });
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
            let _ = this.workspace.update(cx, Workspace::dismiss_usage);
            this.attach_paths(files.paths(), cx);
            this.focus(window, cx);
            cx.stop_propagation();
        }))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            // A chip, or the editor itself, has already answered the press.
            if window.default_prevented() {
                return;
            }
            if !editor_focus.is_focused(window) {
                window.focus(&editor_focus, cx);
            }
            // Without this the workspace, which is focusable too, takes the focus back.
            window.prevent_default();
        })
        .children(sigil_menu)
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(8.))
                .child(attach)
                .child(
                    div()
                        .id("composer-editor")
                        .flex_1()
                        // Closer to the attach button than the row's gap.
                        .ml(px(-4.))
                        .min_w_0()
                        .min_h(px(SEND))
                        .max_h(px(280.))
                        .overflow_y_scroll()
                        .track_scroll(&editor_scroll)
                        .pr(px(6.))
                        .py(px((SEND - LINE) / 2.))
                        .text_size(px(fs_md()))
                        .line_height(px(LINE))
                        // The field is only as tall as its text; a right-click on the box around
                        // it opens the field's Edit menu too.
                        .on_mouse_down(MouseButton::Right, {
                            let editor = self.editor.clone();
                            move |e, window, cx| {
                                editor.update(cx, |ed, cx| ed.open_menu(e, window, cx));
                            }
                        })
                        .child(self.editor.clone()),
                )
                .children(stop)
                // Only with a send button, so an empty box does not push Stop from the end.
                .when(send.is_some(), |row| {
                    row.child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .children(send)
                            .children(send_mode),
                    )
                }),
        );
        let controls = div()
            .id("composer-controls")
            .role(Role::Toolbar)
            .aria_label("Composer controls")
            .mt(px(6.))
            .px(px(12.))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(model_chip)
            .children(effort_chip)
            .child(compact_chip)
            .child(access_chip)
            .child(agent_chip)
            .child(div().flex_1())
            // The worktree's status, at the end of the line before Format.
            .children(worktree.map(|status| {
                div()
                    .min_w_0()
                    .text_size(px(fs_xs()))
                    .line_height(px(16.))
                    .child(status)
            }))
            .child(format_chip);
        // The attachments and the token use share the line just above the pill.
        let status_line = (attached.is_some() || usage.is_some()).then(|| {
            div()
                .mb(px(6.))
                .flex()
                .items_end()
                .gap_x(px(12.))
                .children(attached)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .min_w_0()
                        // In line with where the pill under it stops being round.
                        .px(px(crate::ui::pill::PILL_RADIUS))
                        .text_size(px(fs_xs()))
                        .line_height(px(16.))
                        .children(usage),
                )
        });
        div()
            .id("composer")
            .role(Role::Group)
            .aria_label("Composer")
            .flex()
            .flex_col()
            .children(notice)
            .children(worktree_files)
            .children(status_line)
            .child(pill)
            .child(controls)
            .into_any_element()
    }
}
