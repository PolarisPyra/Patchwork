//! The row under the draft, as the web composer's footer: the "+" menu, the model and the agent
//! role at the left; the context ring, access, Stop and the round send button at the right.

// Rust guideline compliant 2026-10-09

use patchwork_views_logos::agent_logo;
use gpui::{AnyElement, Div, MouseButton, Role, Stateful, Window, div, prelude::*, px};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs};
use patchwork_design_components::{button, tooltip};
use patchwork_design_icons::icon;

use super::chips::{chip, fill_ring, trigger};
use super::draft::{Composer, Picks};
use super::menus::Menu;

/// The send and stop discs, as the web's `size-7`.
const DISC: f32 = 28.;

/// What the footer shows, read from the window before any of its buttons is built.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is a separate fact about the turn that the buttons read"
)]
pub(super) struct FooterState {
    pub picks: Picks,
    pub model_label: String,
    /// The effort's name beside the model, when the model takes one.
    pub effort: Option<String>,
    /// How full the context is, the tokens in it and the tooltip saying so, when its limit is
    /// known.
    pub context: Option<(f32, u64, String)>,
    pub compact: String,
    pub access: (&'static str, IconName),
    pub working: bool,
    pub busy: bool,
    pub editing: bool,
    pub empty: bool,
    pub folder_missing: bool,
    pub queue_next: bool,
    pub can_compact: bool,
}

impl Composer {
    /// The footer row inside the card.
    pub(super) fn footer(
        &mut self,
        s: &FooterState,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let picks = &s.picks;
        let attach = {
            let (hover, fg) = (t.bg_hover, t.text);
            let plus = button("attach", "Add context")
                .size(px(24.))
                .flex_none()
                .justify_center()
                .rounded_full()
                .text_color(t.text_muted)
                .hover(move |s| s.bg(hover).text_color(fg))
                .tooltip(tooltip("Add context and formatting"))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .child(icon(IconName::Plus, 16.));
            self.chip_with_menu(Menu::Attach, plus, picks, s.can_compact, t, window, cx)
        };
        let model = {
            let mut aria = format!("Choose model: {}", s.model_label);
            if let Some(effort) = &s.effort {
                aria = format!("{aria}, effort {effort}");
            }
            let button = trigger("model", aria, t)
                .child(agent_logo(picks.agent, picks.model.as_deref(), 14.))
                .child(
                    div()
                        .min_w_0()
                        .max_w(px(180.))
                        .truncate()
                        .child(s.model_label.clone()),
                )
                .children(
                    s.effort
                        .clone()
                        .map(|effort| div().flex_none().text_color(t.text_muted).child(effort)),
                )
                .child(icon(IconName::CaretDown, 12.).text_color(t.text_muted));
            self.chip_with_menu(Menu::Model, button, picks, s.can_compact, t, window, cx)
        };
        // The agent role the chat runs as: Rust's own, kept beside the model.
        let role = picks.role.clone();
        let agent_role = self.chip_with_menu(
            Menu::Agent,
            chip(
                "agent-role",
                match &role {
                    Some(role) => format!("Agent: {role}"),
                    None => "Agent: none".into(),
                },
                icon(IconName::Robot, 14.).into_any_element(),
                role.unwrap_or_else(|| "Agent".into()),
                t,
            ),
            picks,
            s.can_compact,
            t,
            window,
            cx,
        );
        let context = self.context_trigger(s, t, window, cx);
        let (access, access_icon) = s.access;
        let access = self.chip_with_menu(
            Menu::Access,
            trigger("access", format!("Access: {access}"), t)
                .tooltip(tooltip("What the agent may do without asking"))
                .child(icon(access_icon, 14.))
                .child(div().min_w_0().max_w(px(128.)).truncate().child(access))
                .child(icon(IconName::CaretDown, 14.).text_color(t.text_muted)),
            picks,
            s.can_compact,
            t,
            window,
            cx,
        );
        let stop = s.busy.then(|| {
            let (bg, hover) = (t.bg_selected_muted, t.bg_hover);
            let label = if s.working {
                "Stop the reply"
            } else {
                "Stop background commands"
            };
            button("stop", label)
                .size(px(DISC))
                .flex_none()
                .justify_center()
                .rounded_full()
                .bg(bg)
                .text_color(t.text)
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip(if s.working { "Stop (Esc)" } else { label }))
                .child(icon(IconName::StopFill, 12.))
                .on_click(cx.listener(|_, _, _, cx| {
                    cx.emit(crate::host::ChatEvent::Stop);
                }))
        });
        let send = self.send_controls(s, t, window, cx);
        div()
            .id("composer-controls")
            .role(Role::Toolbar)
            .aria_label("Composer controls")
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.))
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(attach)
                    .child(model)
                    .child(agent_role),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(context)
                    .child(access)
                    .children(stop)
                    .children(send),
            )
    }

    /// How full the context is, as the web's ring and count, opening the Compact menu.
    fn context_trigger(
        &mut self,
        s: &FooterState,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let compact = &s.compact;
        let button = match &s.context {
            Some((used, tokens, detail)) => {
                trigger("compact", format!("{detail}. Compact at: {compact}"), t)
                    .h(px(DISC))
                    .px(px(8.))
                    .text_color(t.text_muted)
                    .tooltip(tooltip(detail.clone()))
                    .child(fill_ring(*used, t))
                    .child(patchwork_design_text::format_tokens(*tokens))
            }
            None => chip(
                "compact",
                format!("Compact at: {compact}"),
                icon(IconName::ArrowsInSimple, 14.).into_any_element(),
                compact.clone(),
                t,
            ),
        };
        self.chip_with_menu(
            Menu::Compact,
            button,
            &s.picks,
            s.can_compact,
            t,
            window,
            cx,
        )
    }

    /// The round send button, and while a turn runs the caret choosing whether it queues or
    /// steers, which the web's lacks. None when a turn runs and there is nothing to send.
    fn send_controls(
        &mut self,
        s: &FooterState,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if s.working && s.empty && !s.editing {
            return None;
        }
        let (label, tip) = if s.editing {
            ("Update queued message", "Update queued message")
        } else if !s.working {
            ("Send message", "Send message (Enter)")
        } else if s.queue_next {
            ("Queue message", "Queue for after the turn (Enter)")
        } else {
            ("Steer message", "Steer the running turn (Enter)")
        };
        let blocked = s.folder_missing || (s.empty && !s.editing);
        let split = s.working && !s.editing;
        let (fill, glyph) = (t.text, t.bg_editor);
        let disc = button("send", label)
            .h(px(DISC))
            .w(px(DISC))
            .flex_none()
            .justify_center()
            .map(|b| {
                if split {
                    b.rounded_l_full()
                } else {
                    b.rounded_full()
                }
            })
            .tooltip(tooltip(if blocked {
                "Write something first"
            } else {
                tip
            }))
            .map(|b| {
                if blocked {
                    b.bg(t.bg_selected_muted)
                        .text_color(t.text_subtle)
                        .cursor_default()
                } else {
                    b.bg(fill)
                        .text_color(glyph)
                        .cursor_pointer()
                        .hover(move |s| s.bg(fill.opacity(0.9)))
                        .on_click(cx.listener(|this, _, window, cx| this.send(false, window, cx)))
                }
            })
            .child(icon(
                if s.editing {
                    IconName::Check
                } else {
                    IconName::ArrowUpBold
                },
                16.,
            ));
        let mode = (split && !s.empty).then(|| {
            let hover = fill.opacity(0.9);
            let caret = button("send-mode", "Choose queue or steer")
                .tooltip(tooltip("Choose queue or steer"))
                .h(px(DISC))
                .w(px(22.))
                .flex_none()
                .justify_center()
                .rounded_r_full()
                .border_l_1()
                .border_color(glyph.opacity(0.2))
                .bg(fill)
                .text_color(glyph)
                .hover(move |s| s.bg(hover))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .child(icon(IconName::CaretDown, 12.));
            self.chip_with_menu(Menu::Send, caret, &s.picks, s.can_compact, t, window, cx)
        });
        Some(
            div()
                .ml(px(4.))
                .flex()
                .flex_none()
                .items_center()
                .text_size(px(fs(14.)))
                .child(disc)
                .children(mode)
                .into_any_element(),
        )
    }
}
