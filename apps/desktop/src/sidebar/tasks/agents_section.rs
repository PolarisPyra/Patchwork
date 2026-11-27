//! The sidebar's Agents section: each agent, which opens its page and is renamed or deleted from its
//! menu, with its latest runs under it.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_shared::{agent_label, agent_logo};
use patchwork_platform::now_ms;
use super::row_menu::Target;
use gpui::{
    AnyElement, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*, px,
};
use patchwork_backend::agents::Agent;
use patchwork_backend::chats::{ApprovalStatus, Chat};
use patchwork_ui_components::avatar::{Mood, avatar};
use patchwork_ui_theme::fs_sm;
use patchwork_ui_components::{button, icon_button};
use patchwork_ui_theme::icon;
use patchwork_ui_theme::{IconName, radius_sm, theme};
use crate::workspace::{Workspace, is_busy};
use crate::sidebar::sections::{empty_row, section_label};
use super::chat_row::{chat_app_presence, phone_mark, presence_word, chat_row};

/// The most runs listed under an agent; its page lists them all.
const RUNS_SHOWN: usize = 5;

/// The Agents heading, then each agent and its latest runs, in the order they are drawn. `runs` are
/// the agents' runs that are not archived, and `rows` gets each run for Shift+click.
pub(super) fn agents_section(
    w: &Workspace,
    runs: &[&Chat],
    first: bool,
    rows: &mut Vec<Target>,
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let new_agent = icon_button("new-agent", "New agent", IconName::Plus, 18., cx)
        .on_click(cx.listener(|this, _, _, cx| this.new_agent(cx)))
        .into_any_element();
    let mut out = vec![section_label("Agents", first, Some(new_agent), &t).into_any_element()];
    // An archived agent is listed on the Archived page instead.
    let agents: Vec<&Agent> = w
        .agents
        .iter()
        .filter(|b| b.archived_at.is_none())
        .collect();
    if agents.is_empty() {
        out.push(empty_row("No agents yet", &t).into_any_element());
    }
    for agent in agents {
        let mut mine: Vec<&Chat> = runs
            .iter()
            .copied()
            .filter(|th| th.agent_id.as_ref() == Some(&agent.id))
            .collect();
        mine.sort_by_key(|th| std::cmp::Reverse(th.created_at));
        let waiting = mine.iter().any(|th| {
            th.messages.last().is_some_and(|m| {
                m.approvals
                    .iter()
                    .any(|a| a.status == ApprovalStatus::Pending)
            })
        });
        let mood = if waiting {
            Mood::Waiting
        } else if mine.iter().any(|th| is_busy(th)) {
            Mood::Working
        } else {
            Mood::Idle
        };
        // Its chat opens from its row, so it is not listed under it.
        let chat = agent
            .chat_id
            .as_ref()
            .filter(|id| mine.iter().any(|th| &th.id == *id));
        let shown = w.agent_shown.as_ref() == Some(&agent.id)
            || (chat.is_some() && chat.map(String::as_str) == selected);
        let (fill, hover) = if shown {
            (t.bg_accent_soft, t.tree_selected)
        } else {
            (gpui::transparent_black(), t.tree_hover)
        };
        let (id, delete, archive) = (agent.id.clone(), agent.id.clone(), agent.id.clone());
        let group = SharedString::from(format!("agent-row:{}", agent.id));
        let (btn_hover, fg) = (t.border_strong, t.text);
        let target = Target::Agent(agent.id.clone());
        let name = w.rename_field(&target, cx).unwrap_or_else(|| {
            div()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(agent.name.clone())
                .into_any_element()
        });
        // The latest thing said in its chat, or its newest run, or its greeting before it has
        // run. A chat app's agent is named for what it is, since its chat shows what was said.
        let said = if agent.chat_app.is_some() {
            "Gateway conversation".to_string()
        } else {
            chat.and_then(|id| mine.iter().find(|th| &th.id == id))
                .or(mine.first())
                .and_then(|th| th.messages.iter().rev().find(|m| !m.text.trim().is_empty()))
                .map_or_else(
                    || format!("Hi, I'm {}. What would you like me to do?", agent.name),
                    |m| {
                        m.text
                            .chars()
                            .take(200)
                            .collect::<String>()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                    },
                )
        };
        out.push(
            div()
                .id(SharedString::from(format!("agent:{}", agent.id)))
                .group(group.clone())
                .relative()
                .role(Role::TreeItem)
                .aria_level(1)
                .aria_label(agent.name.clone())
                .aria_selected(shown)
                .tab_index(0)
                .h(px(patchwork_ui_theme::roomy(52.)))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(10.))
                // The same edges as a chat's row.
                .ml(px(-2.))
                .mr(px(-4.))
                .mb(px(2.))
                .pl(px(8.))
                .pr(px(8.))
                .rounded(px(radius_sm()))
                .cursor_pointer()
                .text_color(t.text)
                .bg(fill)
                .hover(move |s| s.bg(hover))
                .child(avatar(&agent.id, &agent.name, mood, 40.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(name)
                        .child(
                            div()
                                .truncate()
                                .text_size(px(fs_sm()))
                                .text_color(t.text_muted)
                                .child(said),
                        ),
                )
                // The archive button takes the chat app's place while the row is hovered, as a
                // chat's row trades its time for it, and the agent's logo stays in the column of
                // the logos on the rows below.
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            div()
                                .w(px(20.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .group_hover(group.clone(), gpui::Styled::invisible)
                                        .children(agent.chat_app.map(|platform| {
                                            let in_use = mine.iter().any(|th| {
                                                w.chat_apps
                                                    .get(&th.id)
                                                    .is_some_and(|apps| apps.in_use(now_ms()))
                                            });
                                            let presence = chat_app_presence(w, platform, in_use);
                                            let label = format!(
                                                "{}: {}",
                                                platform.label(),
                                                presence_word(presence)
                                            );
                                            phone_mark("chat-app", label, presence, &t)
                                        })),
                                )
                                .child(
                                    button(
                                        SharedString::from(format!("archive-agent:{}", agent.id)),
                                        format!("Archive {}", agent.name),
                                    )
                                    .absolute()
                                    .size(px(20.))
                                    .justify_center()
                                    .rounded(px(radius_sm()))
                                    .text_color(t.text_muted)
                                    .hover(move |s| s.bg(btn_hover).text_color(fg))
                                    .opacity(0.)
                                    .group_hover(group, |s| s.opacity(1.))
                                    .focus(|s| s.opacity(1.))
                                    .child(icon(IconName::Archive, 15.))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.set_agent_archived(&archive, true, cx);
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .id("agent")
                                .role(Role::Image)
                                .aria_label(agent_label(agent.agent))
                                .size(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(agent_logo(agent.agent, agent.model.as_deref(), 14.)),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_agent(&id, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.open_context_menu(target.clone(), e, window, cx);
                    }),
                )
                .on_action(cx.listener(
                    move |this, _: &patchwork_ui_components::DeleteChat, window, cx| {
                        this.delete_agent(&delete, window, cx);
                    },
                ))
                .into_any_element(),
        );
        let chat = chat.cloned();
        for th in mine
            .into_iter()
            .filter(|th| Some(&th.id) != chat.as_ref())
            .take(RUNS_SHOWN)
        {
            rows.push(Target::Chat(th.id.clone()));
            out.push(chat_row(w, th, 1, selected, cx).into_any_element());
        }
    }
    out
}
