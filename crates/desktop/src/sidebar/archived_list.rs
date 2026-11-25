//! `ArchivedView`: agents, chats and tasks moved out of the task list, under the project and
//! folder each came from, in their task-list order. Restoring one puts it back where it was.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{
    AnyElement, Div, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*,
    px,
};
use patchwork_core::agents::Agent;
use patchwork_core::chats::Chat;

use super::tasks::row_menu::Target;
use super::{empty_row, twistie};
use super::tasks::chat_row::{TREE_GUTTER, indent_guide, tree_row};
use patchwork_ui::controls::avatar::{Mood, avatar};
use patchwork_ui::theme::{fs_md, fs_sm, radius_sm, theme};
use patchwork_ui::{agent_label, agent_logo, button, display_title, icon, scrollbar};
use crate::workspace::Workspace;

pub fn archived_view(w: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let t = *theme(cx);
    let archived = w
        .chats
        .iter()
        .filter(|th| th.archived_at.is_some() && th.parent.is_none())
        .cloned();
    let selected = w.chat.as_ref().map(|th| th.id.clone());
    let selected = selected.as_deref();
    // An agent's runs are listed under the agent, not with the chats and projects.
    let agent_of = |th: &Chat| {
        th.agent_id
            .as_ref()
            .and_then(|id| w.agents.iter().find(|b| &b.id == id))
            .cloned()
    };
    let (runs, archived): (Vec<Arc<Chat>>, Vec<Arc<Chat>>) =
        archived.partition(|th| agent_of(th).is_some());
    let run_agents: Vec<Agent> = w
        .agents
        .iter()
        .filter(|b| runs.iter().any(|th| th.agent_id.as_ref() == Some(&b.id)))
        .cloned()
        .collect();
    let chats: Vec<&Chat> = archived
        .iter()
        .map(Arc::as_ref)
        .filter(|th| th.project_id.is_none())
        .collect();

    let agents: Vec<&Agent> = w
        .agents
        .iter()
        .filter(|b| b.archived_at.is_some())
        .collect();
    let empty = archived.is_empty() && runs.is_empty() && agents.is_empty();

    let (muted, fg) = (t.text_muted, t.text);
    let heading = div()
        .h(px(32.))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .pl(px(12.))
        .pr(px(18.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::SEMIBOLD)
        .child("Archived")
        .when(!empty, |s| {
            s.child(
                button("archived-collapse-all", "Collapse all")
                    .gap(px(4.))
                    .font_weight(FontWeight::NORMAL)
                    .text_size(px(fs_sm()))
                    .text_color(muted)
                    .hover(move |s| s.text_color(fg))
                    .child(icon("arrows-in-line-vertical", 15.))
                    .child("Collapse all")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let projects = this.settings.projects.iter().map(|p| p.id.clone());
                        this.archived_collapsed.insert("chats".into());
                        this.archived_collapsed.insert("agents".into());
                        let runs = this.agents.iter().map(|b| format!("agent:{}", b.id));
                        this.archived_collapsed.extend(runs);
                        this.archived_collapsed.extend(projects);
                        cx.notify();
                    })),
            )
        });

    let mut groups = div().mx(px(-8.)).flex_none();
    if !agents.is_empty() {
        groups = groups.child(agents_group(w, &agents, cx));
    }
    for agent in &run_agents {
        let mine: Vec<&Chat> = runs
            .iter()
            .map(Arc::as_ref)
            .filter(|th| th.agent_id.as_ref() == Some(&agent.id))
            .collect();
        let rows = mine
            .iter()
            .map(|th| archived_row(th, false, selected, cx))
            .collect();
        let key = format!("agent:{}", agent.id);
        groups = groups.child(group(
            w,
            &key,
            agent.name.clone(),
            false,
            &mine,
            16.,
            rows,
            cx,
        ));
    }
    if !chats.is_empty() {
        let mut rows = Vec::new();
        for th in &chats {
            rows.push(archived_row(th, false, selected, cx));
        }
        groups = groups.child(group(
            w,
            "chats",
            "Chats".into(),
            false,
            &chats,
            16.,
            rows,
            cx,
        ));
    }
    for p in &w.settings.projects {
        let tasks: Vec<&Chat> = archived
            .iter()
            .map(Arc::as_ref)
            .filter(|th| th.project_id.as_ref() == Some(&p.id))
            .collect();
        if tasks.is_empty() {
            continue;
        }
        let mut rows = Vec::new();
        for f in w.folders.iter().filter(|f| f.project_id == p.id) {
            let inside: Vec<&Chat> = tasks
                .iter()
                .copied()
                .filter(|th| th.folder_id.as_ref() == Some(&f.id))
                .collect();
            if inside.is_empty() {
                continue;
            }
            let mut folder_rows = Vec::new();
            for th in &inside {
                folder_rows.push(archived_row(th, true, selected, cx));
            }
            let key = format!("folder:{}", f.id);
            rows.push(
                group(w, &key, f.name.clone(), true, &inside, 24., folder_rows, cx)
                    .into_any_element(),
            );
        }
        let folder_ids: Vec<&str> = w
            .folders
            .iter()
            .filter(|f| f.project_id == p.id)
            .map(|f| f.id.as_str())
            .collect();
        let top: Vec<&Chat> = tasks
            .iter()
            .copied()
            .filter(|th| {
                !th.folder_id
                    .as_deref()
                    .is_some_and(|f| folder_ids.contains(&f))
            })
            .collect();
        for th in &top {
            rows.push(archived_row(th, false, selected, cx));
        }
        groups = groups.child(group(
            w,
            &p.id,
            p.name.clone(),
            false,
            &tasks,
            16.,
            rows,
            cx,
        ));
    }

    let tree = div()
        .id("archived")
        .role(Role::Tree)
        .aria_label("Archived")
        .group("tree")
        .track_scroll(&w.tree_scroll)
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(1.))
        .size_full()
        .p(px(8.))
        // Rows end where the Tasks tree's do.
        .pr(px(8. + TREE_GUTTER))
        .when(empty, |s| {
            s.child(empty_row("Nothing archived", &t).mt(px(-10.)).px(px(4.)))
        })
        .child(groups);
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(heading)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(tree)
                .children(scrollbar(&w.tree_scroll, 4., 0., t.border_strong)),
        )
}

/// A project's, folder's or the chats' header, which opens and closes the rows under it. Its menu
/// deletes every archived chat or task under it.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a separate piece of what it draws"
)]
fn group(
    w: &Workspace,
    key: &str,
    name: String,
    folder: bool,
    inside: &[&Chat],
    guide: f32,
    rows: Vec<AnyElement>,
    cx: &mut Context<Workspace>,
) -> Div {
    let t = *theme(cx);
    let open = !w.archived_collapsed.contains(key);
    let (toggle_key, ids) = (
        key.to_string(),
        inside.iter().map(|th| th.id.clone()).collect::<Vec<_>>(),
    );
    let label = div().flex_1().min_w_0().truncate().child(name.clone());
    let header = tree_row(
        SharedString::from(format!("archived:{key}")),
        name,
        if folder { 2 } else { 1 },
        open,
        false,
        &t,
    )
    .pl(px(if folder { 12. } else { 4. }))
    .child(twistie(open))
    .child(if folder {
        label.text_size(px(fs_md()))
    } else {
        label.font_weight(FontWeight::MEDIUM)
    })
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, e: &MouseDownEvent, window, cx| {
            this.open_context_menu(Target::Archived(ids.clone()), e, window, cx);
        }),
    )
    .on_click(cx.listener(move |this, _, _, cx| {
        if !this.archived_collapsed.remove(&toggle_key) {
            this.archived_collapsed.insert(toggle_key.clone());
        }
        cx.notify();
    }));
    div().flex_none().child(header).when(open, |s| {
        s.child(
            div()
                .relative()
                .flex()
                .flex_col()
                .py(px(1.))
                .child(indent_guide(guide))
                .children(rows),
        )
    })
}

/// The archived agents' header, which opens and closes them, and each agent with its restore button.
fn agents_group(w: &Workspace, agents: &[&Agent], cx: &mut Context<Workspace>) -> Div {
    let t = *theme(cx);
    let open = !w.archived_collapsed.contains("agents");
    let header = tree_row(
        "archived:agents".into(),
        "Agents".into(),
        1,
        open,
        false,
        &t,
    )
    .pl(px(4.))
    .child(twistie(open))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .font_weight(FontWeight::MEDIUM)
            .child("Agents"),
    )
    .on_click(cx.listener(|this, _, _, cx| {
        if !this.archived_collapsed.remove("agents") {
            this.archived_collapsed.insert("agents".into());
        }
        cx.notify();
    }));
    let (btn_hover, fg, row_hover) = (t.border_strong, t.text, t.tree_hover);
    let rows = agents.iter().map(|agent| {
        let restore = agent.id.clone();
        let target = Target::Agent(agent.id.clone());
        div()
            .id(SharedString::from(format!("archived-agent:{}", agent.id)))
            .role(Role::TreeItem)
            .aria_level(2)
            .aria_label(agent.name.clone())
            .h(px(patchwork_ui::theme::roomy(28.)))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .ml(px(30.))
            .mr(px(4.))
            .pl(px(8.))
            .pr(px(8.))
            .mb(px(2.))
            .rounded(px(radius_sm()))
            .text_color(t.text)
            .hover(move |s| s.bg(row_hover))
            .child(avatar(&agent.id, &agent.name, Mood::Idle, 16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(agent.name.clone()),
            )
            .child(
                button(
                    SharedString::from(format!("restore-agent:{}", agent.id)),
                    format!("Restore {}", agent.name),
                )
                .size(px(20.))
                .flex_none()
                .justify_center()
                .rounded(px(radius_sm()))
                .text_color(t.text_muted)
                .hover(move |s| s.bg(btn_hover).text_color(fg))
                .child(icon("arrow-counter-clockwise", 16.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_agent_archived(&restore, false, cx);
                })),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    this.open_context_menu(target.clone(), e, window, cx);
                }),
            )
    });
    let rows: Vec<_> = rows.collect();
    div().flex_none().child(header).when(open, |s| {
        s.child(
            div()
                .relative()
                .flex()
                .flex_col()
                .py(px(1.))
                .child(indent_guide(16.))
                .children(rows),
        )
    })
}

/// An archived chat or task: its title, the restore button it shows while hovered, and its agent.
fn archived_row(
    th: &Chat,
    nested: bool,
    selected: Option<&str>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let t = *theme(cx);
    let picked = selected == Some(th.id.as_str());
    let title = display_title(&th.title);
    let group = SharedString::from(format!("row:{}", th.id));
    let (fill, hover) = if picked {
        (t.bg_accent_soft, t.tree_selected)
    } else {
        (gpui::transparent_black(), t.tree_hover)
    };
    let (btn_hover, fg) = (t.border_strong, t.text);
    let (id, restore_id) = (th.id.clone(), th.id.clone());
    let target = Target::Chat(th.id.clone());
    div()
        .id(SharedString::from(format!("archived-chat:{}", th.id)))
        .group(group.clone())
        .role(Role::TreeItem)
        .aria_level(if nested { 3 } else { 2 })
        .aria_selected(picked)
        .aria_label(title.clone())
        .tab_index(0)
        .h(px(patchwork_ui::theme::roomy(28.)))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .ml(px(if nested { 38. } else { 30. }))
        .mr(px(4.))
        .pl(px(8.))
        .pr(px(8.))
        .mb(px(2.))
        .rounded(px(radius_sm()))
        .cursor_pointer()
        .text_color(t.text)
        .bg(fill)
        .hover(move |s| s.bg(hover))
        .child(div().flex_1().min_w_0().truncate().child(title.clone()))
        .child(
            button(
                SharedString::from(format!("restore:{}", th.id)),
                format!("Restore {title}"),
            )
            .size(px(20.))
            .flex_none()
            .justify_center()
            .rounded(px(radius_sm()))
            .text_color(t.text_muted)
            .hover(move |s| s.bg(btn_hover).text_color(fg))
            .opacity(0.)
            .group_hover(group, |s| s.opacity(1.))
            .focus(|s| s.opacity(1.))
            .child(icon("arrow-counter-clockwise", 16.))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                let id = restore_id.clone();
                this.change(
                    "restore the chat",
                    move |core| core.set_chat_archived(&id, false),
                    cx,
                );
            })),
        )
        .child(
            div()
                .id("agent")
                .role(Role::Image)
                .size(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .aria_label(agent_label(th.agent))
                .child(agent_logo(th.agent, th.model.as_deref(), 14.)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.open_context_menu(target.clone(), e, window, cx);
            }),
        )
        .on_click(cx.listener(move |this, _, _, cx| this.open_chat(&id, cx)))
        .into_any_element()
}
