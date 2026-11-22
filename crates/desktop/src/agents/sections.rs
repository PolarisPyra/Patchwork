//! The agent details' This conversation and Routines sections, and the Activity and Inspector
//! pages they open in place of the tab.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{AnyElement, Div, PromptLevel, Role, SharedString, Stateful, Window, div, prelude::*, px};
use patchwork_core::chats::TokenUsage;
use patchwork_ui::theme::{Theme, font_mono, fs_md, fs_sm, fs_xs, radius_sm};
use patchwork_ui::{button, display_title, format_relative, format_tokens, icon, icon_button, spinner};
use super::details::heading;
use super::home::AgentPage;

/// A page the details open in place of the tab.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Drill {
    Activity,
    Inspector,
}

/// A row of the details that acts when pressed: an icon, its name, and what is on its right.
fn row(id: impl Into<SharedString>, label: &str, glyph: &str, t: &Theme) -> Stateful<Div> {
    let hover = t.bg_hover;
    button(id.into(), label.to_string())
        .gap(px(8.))
        .h(px(30.))
        .px(px(8.))
        // Its icon lines up with the first letter of the section's heading.
        .mx(px(-8.))
        .rounded(px(radius_sm()))
        .hover(move |s| s.bg(hover))
        .text_size(px(fs_md()))
        .child(div().text_color(t.text_muted).child(icon(glyph, 15.)))
        .child(div().flex_1().min_w_0().truncate().child(label.to_string()))
}

/// A row that opens a page or another place, with a caret on its right.
fn link(id: &'static str, label: &str, glyph: &str, t: &Theme) -> Stateful<Div> {
    row(id, label, glyph, t).child(
        div()
            .text_color(t.text_muted)
            .child(icon("caret-right", 12.)),
    )
}

/// A name and its value on one line.
fn fact(label: &'static str, value: String, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(28.))
        .text_size(px(fs_md()))
        .child(div().flex_1().text_color(t.text_muted).child(label))
        .child(value)
}

impl AgentPage {
    /// The newest conversation: find in it, clear it, see what it used, see what the agent is told.
    /// None while the agent has none.
    pub(super) fn this_conversation(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let run = self.usage.as_ref()?.runs.first()?;
        let clear_id = run.chat_id.clone();
        let title = display_title(&run.title);
        Some(
            div()
                .flex()
                .flex_col()
                .child(heading("This conversation", t))
                .child(
                    row("agent-find", "Find in conversation", "magnifying-glass", t)
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(fs_xs()))
                                .text_color(t.text_muted)
                                .child("Ctrl F"),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.find_in_newest(window, cx);
                        })),
                )
                .child(
                    row(
                        "agent-start-fresh",
                        "Start fresh",
                        "arrow-counter-clockwise",
                        t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_fresh(clear_id.clone(), &title, window, cx);
                    })),
                )
                .child(
                    link("agent-activity", "Activity", "chart-bar", t)
                        .on_click(cx.listener(|this, _, _, cx| this.drill(Drill::Activity, cx))),
                )
                .child(
                    link("agent-inspector", "Inspector", "eye", t)
                        .on_click(cx.listener(|this, _, _, cx| this.drill(Drill::Inspector, cx))),
                )
                .into_any_element(),
        )
    }

    /// Opens the agent's newest conversation with the find field open.
    pub(super) fn find_in_newest(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .usage
            .as_ref()
            .and_then(|u| u.runs.first())
            .map(|r| r.chat_id.clone())
        else {
            return;
        };
        let _ = self.workspace.update(cx, |w, cx| {
            w.open_chat(&id, cx);
            w.open_find(window, cx);
        });
    }

    /// The agent's scheduled tasks: all of them in Settings, or a new one.
    pub(super) fn routines(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .child(heading("Routines", t))
            .child(
                link("agent-automations", "Automations", "calendar-blank", t).on_click(
                    cx.listener(|this, _, window, cx| {
                        let _ = this.workspace.update(cx, |w, cx| {
                            w.open_settings(Some("scheduled"), window, cx);
                        });
                    }),
                ),
            )
            .child(
                row("agent-add-routine", "Add a routine", "plus", t).on_click(cx.listener(
                    |this, _, window, cx| {
                        let agent = this.agent.clone();
                        let _ = this
                            .workspace
                            .update(cx, |w, cx| w.add_agent_routine(&agent, window, cx));
                    },
                )),
            )
            .into_any_element()
    }

    /// Opens page `to` in place of the tab, and reads what the agent is told for the Inspector.
    fn drill(&mut self, to: Drill, cx: &mut Context<Self>) {
        self.drill = Some(to);
        if to == Drill::Inspector {
            self.brief = None;
            let (core, id) = (Arc::clone(&self.core), self.agent.id.clone());
            let task = cx.background_spawn(async move { core.agent_brief_text(&id) });
            cx.spawn(async move |this, cx| {
                let brief = task.await.map_err(|e| e.to_string());
                let _ = this.update(cx, |this, cx| {
                    this.brief = Some(brief);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    /// Asks, then deletes conversation `id`'s messages so the agent starts it over.
    fn start_fresh(&self, id: String, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            "Start fresh",
            Some(&format!(
                "Clear {title}? Its messages are deleted and the agent forgets this conversation."
            )),
            &["Clear", "Cancel"],
            cx,
        );
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let cleared = cx
                .background_spawn(async move { core.clear_chat(&id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = &cleared {
                    log::warn!(error:% = e; "could not clear the agent's conversation: {{error}}");
                }
                let _ = this.workspace.update(cx, |w, cx| {
                    if let Err(e) = cleared {
                        w.error = Some(e.to_string());
                    }
                    w.reload(cx);
                });
                this.load_usage(cx);
            });
        })
        .detach();
    }

    /// Page `to` with a Back button over it.
    pub(super) fn drill_page(&self, to: Drill, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let title = match to {
            Drill::Activity => "Activity",
            Drill::Inspector => "Inspector",
        };
        let body = match to {
            Drill::Activity => self.activity(t),
            Drill::Inspector => self.inspector(t),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .pt(px(8.))
                    .ml(px(-6.))
                    .child(
                        icon_button("agent-drill-back", "Back", "caret-left", 14., cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.drill = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(heading(title, t).pt(px(0.)).pb(px(0.))),
            )
            .child(body)
            .into_any_element()
    }

    /// What the newest conversation and all of the agent's conversations used.
    fn activity(&self, t: &Theme) -> AnyElement {
        let Some(usage) = &self.usage else {
            return div().into_any_element();
        };
        let sum = |u: &TokenUsage| u.input + u.cache_read + u.cache_write + u.output;
        let mut list = div()
            .id("agent-activity-facts")
            .role(Role::List)
            .aria_label("Activity")
            .flex()
            .flex_col()
            .pt(px(8.));
        let gateway = self.agent.chat_app.is_some();
        if let Some(run) = usage.runs.first() {
            list = list
                // A chat app's agent has only the one conversation, so it is not named.
                .when(!gateway, |d| {
                    d.child(fact("Conversation", display_title(&run.title), t))
                })
                .child(fact("Started", format_relative(run.started_at), t))
                .child(fact("Input", format_tokens(run.usage.input), t))
                .child(fact("Output", format_tokens(run.usage.output), t))
                .child(fact("Cache read", format_tokens(run.usage.cache_read), t))
                .child(fact("Cache write", format_tokens(run.usage.cache_write), t));
        }
        // A chat app's agent keeps one conversation, so it has no count to show.
        list.when(!gateway, |d| {
            d.child(fact("Conversations", usage.runs.len().to_string(), t))
        })
        .child(fact("All tokens", format_tokens(sum(&usage.total)), t))
        .into_any_element()
    }

    /// What the agent is told at the start of each conversation.
    fn inspector(&self, t: &Theme) -> AnyElement {
        match &self.brief {
            None => div()
                .id("agent-brief-loading")
                .role(Role::Status)
                .aria_label("Loading")
                .p(px(8.))
                .child(spinner(16., t.text_muted))
                .into_any_element(),
            Some(Err(e)) => div()
                .id("agent-brief-error")
                .role(Role::Alert)
                .pt(px(8.))
                .text_size(px(fs_sm()))
                .text_color(t.danger)
                .child(e.clone())
                .into_any_element(),
            Some(Ok(brief)) => div()
                .id("agent-brief")
                .aria_label("What the agent is told")
                .mt(px(8.))
                .p(px(8.))
                .rounded(px(radius_sm()))
                .bg(t.bg_input)
                .border_1()
                .border_color(t.border)
                .font_family(font_mono())
                .text_size(px(fs_sm()))
                .whitespace_normal()
                .child(brief.trim().to_string())
                .into_any_element(),
        }
    }
}
