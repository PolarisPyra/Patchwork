//! Settings → Worktrees: whether a fork gets a worktree of its own, and when task worktrees are
//! cleaned up, by default and in each project.

// Rust guideline compliant 2026-10-09

use std::rc::Rc;

use gpui::{
    AnyElement, ClickEvent, Div, FontWeight, KeyDownEvent, Role, SharedString, Stateful, div,
    prelude::*, px,
};
use patchwork_core::settings::{KeepWorktrees, Settings, WorktreeCleanup};

use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, section_heading, select_box, switch, token_row};
use crate::ui::theme::{Theme, font_mono, fs_sm, fs_xs, radius_sm, theme};
use crate::ui::{button, icon};

/// The most days a cleanup rule waits.
const MAX_DAYS: u32 = 365;

/// What a project's worktree's folder is kept for, and their names.
const KEEPS: [(KeepWorktrees, &str); 3] = [
    (KeepWorktrees::Uncommitted, "Uncommitted changes"),
    (KeepWorktrees::Tracked, "Tracked changes"),
    (KeepWorktrees::AnyFiles, "Any local files"),
];

/// The cleanup rules a row changes: the default ones for None, or a project's own.
fn rules_mut<'a>(s: &'a mut Settings, project: Option<&str>) -> &'a mut WorktreeCleanup {
    match project {
        None => &mut s.worktrees.cleanup,
        Some(id) => {
            let default = s.worktrees.cleanup.clone();
            s.worktrees.projects.entry(id.to_owned()).or_insert(default)
        }
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let worktrees = d.settings.worktrees.clone();
    let fork_new = worktrees.fork_into_new;
    let fork_row = token_row("Fork into a new worktree", None, &t).child(
        switch("worktrees-fork", "Fork into a new worktree", fork_new, &t).on_click(
            cx.listener(move |d, _, _, cx| d.update(|s| s.worktrees.fork_into_new = !fork_new, cx)),
        ),
    );
    let defaults = cleanup_rows(None, "", &worktrees.cleanup, &t, d, cx);
    let projects: Vec<_> = d
        .settings
        .projects
        .iter()
        .map(|p| (p.id.clone(), p.name.clone(), p.path.clone()))
        .collect();
    let projects: Vec<_> = projects
        .into_iter()
        .map(|(id, name, path)| project_group(&id, &name, &path, &t, d, cx))
        .collect();
    div()
        .flex()
        .flex_col()
        .child(section_heading("Forks", true, &t))
        .child(fork_row)
        .child(section_heading("Cleanup", false, &t))
        .children(defaults)
        .when(!projects.is_empty(), |col| {
            col.child(section_heading("Projects", false, &t))
                .children(projects)
        })
        .into_any_element()
}

/// A project's own fork and cleanup settings, with its cleanup rows under it while it has its
/// own rules.
fn project_group(
    id: &str,
    name: &str,
    path: &str,
    t: &Theme,
    d: &mut SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let worktrees = &d.settings.worktrees;
    let own_fork = worktrees.fork_projects.get(id).copied();
    let fork_new = worktrees.forks_into_new(id);
    let own_rules = worktrees.projects.get(id).cloned();
    let custom = own_fork.is_some() || own_rules.is_some();
    let (follow, fork, rules) = (id.to_owned(), id.to_owned(), id.to_owned());
    let has_rules = own_rules.is_some();
    let head = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .py(px(8.))
        .child(
            div()
                .min_w_0()
                .flex_1()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_owned()),
                )
                .child(
                    div()
                        .truncate()
                        .font_family(font_mono())
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(path.to_owned()),
                ),
        )
        .when(custom, |row| {
            row.child(
                button(
                    SharedString::from(format!("worktrees-follow-{id}")),
                    format!("Use the default worktree settings in {name}"),
                )
                .whitespace_nowrap()
                .text_size(px(fs_sm()))
                .text_color(t.accent_text)
                .hover(gpui::Styled::underline)
                .child("Use the default")
                .on_click(cx.listener(move |d, _, _, cx| {
                    let id = follow.clone();
                    d.update(
                        |s| {
                            s.worktrees.fork_projects.remove(&id);
                            s.worktrees.projects.remove(&id);
                        },
                        cx,
                    );
                })),
            )
        })
        .child(label("New worktree forks", t))
        .child(
            switch(
                SharedString::from(format!("worktrees-fork-{id}")),
                format!("Fork into a new worktree in {name}"),
                fork_new,
                t,
            )
            .on_click(cx.listener(move |d, _, _, cx| {
                let id = fork.clone();
                d.update(
                    |s| {
                        s.worktrees.fork_projects.insert(id, !fork_new);
                    },
                    cx,
                );
            })),
        )
        .child(label("Own cleanup rules", t))
        .child(
            switch(
                SharedString::from(format!("worktrees-own-{id}")),
                format!("Own cleanup rules in {name}"),
                has_rules,
                t,
            )
            .on_click(cx.listener(move |d, _, _, cx| {
                let id = rules.clone();
                d.update(
                    |s| {
                        if has_rules {
                            s.worktrees.projects.remove(&id);
                        } else {
                            rules_mut(s, Some(&id));
                        }
                    },
                    cx,
                );
            })),
        );
    let rows = own_rules
        .map(|rules| cleanup_rows(Some(id), &format!(" in {name}"), &rules, t, d, cx))
        .unwrap_or_default();
    div()
        .id(SharedString::from(format!("worktrees-project-{id}")))
        .role(Role::Group)
        .aria_label(SharedString::from(name.to_owned()))
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(t.border)
        .child(head)
        .when(!rows.is_empty(), |g| {
            g.child(
                div()
                    .pl(px(16.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .children(rows),
            )
        })
}

/// A small name beside a project's switch.
fn label(text: &'static str, t: &Theme) -> Div {
    div()
        .whitespace_nowrap()
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .child(text)
}

/// The rows of one set of cleanup rules: the default ones for `project` None, or a project's.
/// `suffix` follows each control's name, so a screen reader tells the projects' apart.
fn cleanup_rows(
    project: Option<&str>,
    suffix: &str,
    rules: &WorktreeCleanup,
    t: &Theme,
    d: &mut SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Vec<Stateful<Div>> {
    let scope = project.unwrap_or("default").to_owned();
    let owner = project.map(str::to_owned);
    let toggle = |title: &'static str,
                  on: bool,
                  set: fn(&mut WorktreeCleanup, bool),
                  cx: &mut Context<SettingsDialog>| {
        let owner = owner.clone();
        token_row(format!("{title}{suffix}"), None, t).child(
            switch(
                SharedString::from(format!("worktrees-{scope}-{title}")),
                format!("{title}{suffix}"),
                on,
                t,
            )
            .on_click(cx.listener(move |d, _, _, cx| {
                let owner = owner.clone();
                d.update(|s| set(rules_mut(s, owner.as_deref()), !on), cx);
            })),
        )
    };
    let days = |title: &'static str,
                on: bool,
                value: u32,
                set: fn(&mut WorktreeCleanup, u32),
                row: Stateful<Div>,
                cx: &mut Context<SettingsDialog>| {
        if !on {
            return row;
        }
        let owner = owner.clone();
        row.child(day_stepper(
            SharedString::from(format!("worktrees-{scope}-{title}-days")),
            format!("Days before {}{suffix}", title.to_lowercase()),
            value,
            t,
            move |s, v| set(rules_mut(s, owner.as_deref()), v),
            cx,
        ))
    };
    let deleted = toggle(
        "Delete with deleted tasks",
        rules.with_deleted_tasks,
        |r, v| r.with_deleted_tasks = v,
        cx,
    );
    let merged = toggle("Delete merged", rules.merged, |r, v| r.merged = v, cx);
    let inactive = toggle("Delete inactive", rules.inactive, |r, v| r.inactive = v, cx);
    let inactive = days(
        "Delete inactive",
        rules.inactive,
        rules.inactive_days,
        |r, v| r.inactive_days = v,
        inactive,
        cx,
    );
    let archived = toggle("Delete archived", rules.archived, |r, v| r.archived = v, cx);
    let archived = days(
        "Delete archived",
        rules.archived,
        rules.archived_days,
        |r, v| r.archived_days = v,
        archived,
        cx,
    );
    let unchanged = toggle(
        "Delete unchanged",
        rules.unchanged,
        |r, v| r.unchanged = v,
        cx,
    );
    let keep = keep_row(&scope, owner.clone(), suffix, rules.keep, t, d, cx);
    vec![deleted, merged, inactive, archived, unchanged, keep]
}

/// The row whose dropdown picks what keeps a worktree's folder.
fn keep_row(
    scope: &str,
    owner: Option<String>,
    suffix: &str,
    keep: KeepWorktrees,
    t: &Theme,
    d: &SettingsDialog,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let key = SharedString::from(format!("worktrees-{scope}-keep"));
    let name = SharedString::from(format!("Keep when{suffix}"));
    let current = KEEPS
        .iter()
        .find(|(k, _)| *k == keep)
        .map_or("", |(_, label)| label);
    let open = d.menu_open(&key);
    let (menu_key, menu_name) = (key.clone(), name.clone());
    token_row(name.clone(), None, t).child(select_box(key, name, current, open, t).on_click(
        cx.listener(move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = KEEPS
                .iter()
                .map(|&(k, label)| {
                    let owner = owner.clone();
                    choice(weak.clone(), label, k == keep, move |d, cx| {
                        d.menu = None;
                        let owner = owner.clone();
                        d.update(|s| rules_mut(s, owner.as_deref()).keep = k, cx);
                    })
                })
                .collect();
            d.toggle_menu(
                menu_key.clone(),
                menu_name.clone(),
                items,
                e.position(),
                window,
                cx,
            );
        }),
    ))
}

/// A number of days from 0 to [`MAX_DAYS`], changed by its buttons or the arrow keys.
fn day_stepper(
    id: SharedString,
    label: String,
    value: u32,
    t: &Theme,
    set: impl Fn(&mut Settings, u32) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let set = Rc::new(set);
    let (hover, fg, muted) = (t.bg_hover, t.text, t.text_muted);
    let step = |which: &str, name: String, glyph: &'static str, to: Option<u32>| {
        let set = Rc::clone(&set);
        button(SharedString::from(format!("{id}-{which}")), name)
            .size(px(24.))
            .justify_center()
            .text_color(muted)
            .child(icon(glyph, 12.))
            .map(|b| match to {
                Some(to) => b
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let set = Rc::clone(&set);
                        d.update(|s| set(s, to), cx);
                    })),
                None => b.opacity(0.4).cursor_default(),
            })
    };
    let fewer = step(
        "fewer",
        format!("Fewer {}", label.to_lowercase()),
        "minus",
        value.checked_sub(1),
    );
    let more = step(
        "more",
        format!("More {}", label.to_lowercase()),
        "plus",
        (value < MAX_DAYS).then_some(value + 1),
    );
    let keys = Rc::clone(&set);
    let shown = if value == 1 {
        "1 day".to_owned()
    } else {
        format!("{value} days")
    };
    div()
        .id(id)
        .role(Role::SpinButton)
        .aria_label(SharedString::from(label))
        .aria_numeric_value(f64::from(value))
        .aria_min_numeric_value(0.)
        .aria_max_numeric_value(f64::from(MAX_DAYS))
        .tab_index(0)
        .focus_visible(crate::ui::focus_ring)
        .on_key_down(cx.listener(move |d, e: &KeyDownEvent, _, cx| {
            let next = match e.keystroke.key.as_str() {
                "left" | "down" => value.saturating_sub(1),
                "right" | "up" => (value + 1).min(MAX_DAYS),
                "home" => 0,
                "end" => MAX_DAYS,
                _ => return,
            };
            cx.stop_propagation();
            if next != value {
                let set = Rc::clone(&keys);
                d.update(|s| set(s, next), cx);
            }
        }))
        .flex()
        .flex_none()
        .items_center()
        .overflow_hidden()
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .child(fewer)
        .child(
            div()
                .w(px(64.))
                .border_x_1()
                .border_color(t.border_strong)
                .text_center()
                .text_size(px(fs_sm()))
                .child(shown),
        )
        .child(more)
}
