//! The composer drawn: the draft, its attachments, and the chips under it.

// Rust guideline compliant 2026-10-09

use gpui::{KeyDownEvent, Focusable, MouseButton, Role, SharedString, Window, div, prelude::*, px};
use crate::chat::user::attachment_chip;
use crate::ui::theme::{FS_MD, RADIUS, RADIUS_LG, theme};
use crate::ui::{agent_logo, button, icon, tooltip};
use crate::workspace::{is_busy, is_working};
use super::menus::{Menu, access_of, compact_label, effort_label, efforts};
use super::chips::{chip, context_of, fill_ring, tool_button};
use super::send::queue_by_default;
use super::state::Composer;

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
        let ring = match limit {
            Some(limit) => fill_ring(
                thread.and_then(context_of).unwrap_or(0) as f32 / limit as f32,
                &t,
            ),
            None => icon("arrows-in-simple", 16.).into_any_element(),
        };
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
        self.can_queue = w.queued().is_empty();
        let queue_next = self.queue_next.unwrap_or_else(|| queue_by_default(w));
        let can_compact = thread.is_some_and(|t| !t.messages.is_empty()) && !working;
        let editor = self.editor.read(cx);
        let empty = editor.content.trim().is_empty() && self.attachments.is_empty();
        let focused = editor.focus_handle(cx).is_focused(window);
        let editor_focus = editor.focus_handle(cx);

        let format_editor = self.editor.clone();
        let format = |id: &'static str,
                      label: &'static str,
                      name: &'static str,
                      before: &'static str,
                      after: &'static str| {
            let editor = format_editor.clone();
            tool_button(id, label, name, &t).on_click(move |_, window, cx| {
                editor.update(cx, |e, cx| {
                    if after.is_empty() {
                        e.prefix_line(before, window, cx);
                    } else {
                        e.surround(before, after, window, cx);
                    }
                });
            })
        };
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
                    .child(icon("x", 10.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.attachments.remove(i);
                        cx.notify();
                    })),
                )
            })
            .collect();
        let stop = busy.then(|| {
            let hover = t.accent_hover;
            button(
                "stop",
                if working {
                    "Stop"
                } else {
                    "Stop background commands"
                },
            )
            .size(px(32.))
            .flex_none()
            .justify_center()
            .rounded(px(RADIUS))
            .bg(t.bg_selected_muted)
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
            let base = button("send", send_label)
                .tooltip(tooltip(send_tip))
                .size(px(32.))
                .flex_none()
                .justify_center()
                .map(|b| {
                    if split {
                        b.rounded_l(px(RADIUS))
                    } else {
                        b.rounded(px(RADIUS))
                    }
                })
                .child(icon(if editing { "check" } else { "arrow-up" }, 16.));
            let blocked = folder_missing
                || (empty && !editing)
                || (working && queue_next && !self.can_queue && !editing);
            if blocked {
                base.bg(t.bg_selected_muted)
                    .text_color(t.text_subtle)
                    .cursor_default()
            } else {
                let hover = t.accent_hover;
                base.bg(t.accent)
                    .text_color(t.text_on_accent)
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, window, cx| this.send(false, true, window, cx)))
            }
        });
        let send_mode = (split && !empty).then(|| {
            let hover = t.accent_hover;
            let caret = button("send-mode", "Choose queue or steer")
                .tooltip(tooltip("Choose queue or steer"))
                .h(px(32.))
                .w(px(24.))
                .flex_none()
                .justify_center()
                .rounded_r(px(RADIUS))
                .border_l_1()
                .border_color(t.text_on_accent.opacity(0.2))
                .bg(t.accent)
                .text_color(t.text_on_accent)
                .hover(move |s| s.bg(hover))
                .child(icon("caret-down", 12.));
            self.chip_with_menu(Menu::Send, caret, &picks, can_compact, &t, window, cx)
        });
        let model_chip = self.chip_with_menu(
            Menu::Model,
            chip(
                "model",
                format!("Choose model: {model_label}"),
                agent_logo(agent, picks.model.as_deref(), 16.),
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
                    icon("lightbulb", 16.).into_any_element(),
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
        let compact_chip = self.chip_with_menu(
            Menu::Compact,
            chip(
                "compact",
                format!("Compact at: {compact}"),
                ring,
                compact,
                &t,
            ),
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
                icon(access_icon, 16.).into_any_element(),
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
                icon("robot", 16.).into_any_element(),
                role.unwrap_or_else(|| "Agent".into()),
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let sigil_menu = self.sigil_menu(picks.agent, &provider_commands, &t, cx);
        let frame = div()
            .id("composer")
            .role(Role::Group)
            .aria_label("Composer")
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(RADIUS_LG))
            .pt(px(14.))
            .pr(px(12.))
            .pb(px(10.))
            .pl(px(12.))
            .border_1()
            .border_color(if focused { t.accent } else { t.border_strong })
            .bg(t.bg_editor)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" && this.sigil_escape(cx) {
                    return cx.stop_propagation();
                }
                let m = &e.keystroke.modifiers;
                if e.keystroke.key == "up" && m.alt && !m.control && !m.shift && !m.platform {
                    let _ = this
                        .workspace
                        .update(cx, crate::workspace::Workspace::edit_latest_queued);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !editor_focus.is_focused(window) {
                    window.focus(&editor_focus, cx);
                }
                // Without this the workspace, which is focusable too, takes the focus back.
                window.prevent_default();
            })
            .children(sigil_menu)
            .when(!attachments.is_empty(), |c| {
                c.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .px(px(6.))
                        .children(attachments),
                )
            })
            .child(
                div()
                    .id("composer-editor")
                    .min_h(px(64.))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .px(px(6.))
                    .text_size(px(FS_MD))
                    .line_height(px(20.))
                    .child(self.editor.clone()),
            )
            .child(
                div()
                    .id("composer-controls")
                    .role(Role::Toolbar)
                    .aria_label("Composer controls")
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
                    .child(
                        div()
                            .id("formatting")
                            .role(Role::Toolbar)
                            .aria_label("Formatting")
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(format("bold", "Bold", "text-b", "**", "**"))
                            .child(format("italic", "Italic", "text-italic", "*", "*"))
                            .child(format(
                                "strike",
                                "Strikethrough",
                                "text-strikethrough",
                                "~~",
                                "~~",
                            ))
                            .child(format("inline-code", "Inline code", "code", "`", "`"))
                            .child(format(
                                "code-block",
                                "Code block",
                                "code-block",
                                "```\n",
                                "\n```",
                            ))
                            .child(format("bullets", "Bulleted list", "list-bullets", "- ", ""))
                            .child(format(
                                "numbers",
                                "Numbered list",
                                "list-numbers",
                                "1. ",
                                "",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                tool_button("attach-files", "Attach files", "paperclip", &t)
                                    .on_click(cx.listener(|this, _, _, cx| this.attach(false, cx))),
                            )
                            .child(
                                tool_button("attach-folder", "Attach folder", "folder", &t)
                                    .on_click(cx.listener(|this, _, _, cx| this.attach(true, cx))),
                            ),
                    )
                    .children(stop)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .children(send)
                            .children(send_mode),
                    ),
            );
        div()
            .flex()
            .flex_col()
            .children(notice)
            .children(usage)
            .child(frame)
            .into_any_element()
    }
}
