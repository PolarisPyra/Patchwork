//! The sidebar's Bots section: each bot, which opens its page and is renamed or deleted from its
//! menu, with its latest runs under it.

// Rust guideline compliant 2026-10-09

use super::menu::Target;
use gpui::{
    AnyElement, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*, px,
};
use patchwork_core::bots::Bot;
use patchwork_core::threads::{ApprovalStatus, Thread};
use crate::ui::avatar::{Mood, avatar};
use crate::ui::theme::fs_sm;
use crate::ui::{agent_label, agent_logo, button, icon, icon_button, now_ms};
use crate::ui::theme::{radius_sm, theme};
use crate::workspace::{Workspace, is_busy};
use super::render::{empty_row, section_label};
use super::tree::{chat_app_presence, phone_mark, presence_word, thread_row};

/// The most runs listed under a bot; its page lists them all.
const RUNS_SHOWN: usize = 5;

/// The Bots heading, then each bot and its latest runs, in the order they are drawn. `runs` are
/// the bots' runs that are not archived, and `rows` gets each run for Shift+click.
pub(super) fn bots_section(
    w: &Workspace,
    runs: &[&Thread],
    first: bool,
    rows: &mut Vec<Target>,
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let new_bot = icon_button("new-bot", "New bot", "plus", 18., cx)
        .on_click(cx.listener(|this, _, _, cx| this.new_bot(cx)))
        .into_any_element();
    let mut out = vec![section_label("Bots", first, Some(new_bot), &t).into_any_element()];
    // An archived bot is listed on the Archived page instead.
    let bots: Vec<&Bot> = w.bots.iter().filter(|b| b.archived_at.is_none()).collect();
    if bots.is_empty() {
        out.push(empty_row("No bots yet", &t).into_any_element());
    }
    for bot in bots {
        let mut mine: Vec<&Thread> = runs
            .iter()
            .copied()
            .filter(|th| th.bot_id.as_ref() == Some(&bot.id))
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
        let chat = bot
            .thread_id
            .as_ref()
            .filter(|id| mine.iter().any(|th| &th.id == *id));
        let shown = w.bot_shown.as_ref() == Some(&bot.id)
            || (chat.is_some() && chat.map(String::as_str) == selected);
        let (fill, hover) = if shown {
            (t.bg_accent_soft, t.tree_selected)
        } else {
            (gpui::transparent_black(), t.tree_hover)
        };
        let (id, delete, archive) = (bot.id.clone(), bot.id.clone(), bot.id.clone());
        let group = SharedString::from(format!("bot-row:{}", bot.id));
        let (btn_hover, fg) = (t.border_strong, t.text);
        let target = Target::Bot(bot.id.clone());
        let name = w.rename_field(&target, cx).unwrap_or_else(|| {
            div()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .child(bot.name.clone())
                .into_any_element()
        });
        // The latest thing said in its chat, or its newest run, or its greeting before it has
        // run. A chat app's bot is named for what it is, since its chat shows what was said.
        let said = if bot.chat_app.is_some() {
            "Gateway conversation".to_string()
        } else {
            chat.and_then(|id| mine.iter().find(|th| &th.id == id))
                .or(mine.first())
                .and_then(|th| th.messages.iter().rev().find(|m| !m.text.trim().is_empty()))
                .map_or_else(
                    || format!("Hi, I'm {}. What would you like me to do?", bot.name),
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
                .id(SharedString::from(format!("bot:{}", bot.id)))
                .group(group.clone())
                .relative()
                .role(Role::TreeItem)
                .aria_level(1)
                .aria_label(bot.name.clone())
                .aria_selected(shown)
                .tab_index(0)
                .h(px(crate::ui::theme::roomy(52.)))
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
                .child(avatar(&bot.id, &bot.name, mood, 32.))
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
                                        .children(bot.chat_app.map(|platform| {
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
                                        SharedString::from(format!("archive-bot:{}", bot.id)),
                                        format!("Archive {}", bot.name),
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
                                    .child(icon("archive", 15.))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.set_bot_archived(&archive, true, cx);
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .id("agent")
                                .role(Role::Image)
                                .aria_label(agent_label(bot.agent))
                                .size(px(16.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(agent_logo(bot.agent, bot.model.as_deref(), 14.)),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_bot(&id, cx)))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        this.open_context_menu(target.clone(), e, window, cx);
                    }),
                )
                .on_action(cx.listener(move |this, _: &crate::DeleteChat, window, cx| {
                    this.delete_bot(&delete, window, cx);
                }))
                .into_any_element(),
        );
        let chat = chat.cloned();
        for th in mine
            .into_iter()
            .filter(|th| Some(&th.id) != chat.as_ref())
            .take(RUNS_SHOWN)
        {
            rows.push(Target::Thread(th.id.clone()));
            out.push(thread_row(w, th, 1, selected, cx).into_any_element());
        }
    }
    out
}
