//! Setup's last step: what was chosen, and where tasks run.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, Role, SharedString, Toggled, div, prelude::*, px};

use super::{ImportResult, Onboarding};
use super::rows::{heading, picked};
use crate::ui::theme::{FS_LG, RADIUS, Theme};
use crate::ui::{agent_label, icon};

fn describe_import(imported: Option<ImportResult>) -> String {
    let Some(imported) = imported.filter(|i| i.threads > 0) else {
        return "None imported".into();
    };
    let plural = if imported.threads == 1 {
        "conversation"
    } else {
        "conversations"
    };
    let threads = format!("{} {plural}", imported.threads);
    if imported.projects == 0 {
        return format!("{threads} imported");
    }
    let plural = if imported.projects == 1 {
        "project"
    } else {
        "projects"
    };
    format!("{threads} imported into {} new {plural}", imported.projects)
}

pub fn ready_step(this: &Onboarding, t: &Theme, cx: &mut Context<Onboarding>) -> AnyElement {
    let agents = this
        .ordered_agents()
        .iter()
        .enumerate()
        .map(|(i, k)| {
            if i == 0 {
                format!("{} (default)", agent_label(*k))
            } else {
                agent_label(*k).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let project = this
        .settings
        .projects
        .iter()
        .find(|p| this.settings.active_project_id.as_deref() == Some(p.id.as_str()))
        .map_or_else(|| "None yet".to_string(), |p| p.name.clone());
    let rows = [
        ("Agents", agents),
        ("Conversations", describe_import(this.imported)),
        ("Project", project),
    ];
    let count = rows.len();
    let list = rows.into_iter().enumerate().map(|(i, (term, value))| {
        div()
            .id(SharedString::from(format!("setup-summary-{term}")))
            .role(Role::Group)
            .aria_label(term)
            .flex()
            .gap(px(16.))
            .py(px(12.))
            .when(i + 1 < count, |d| d.border_b_1().border_color(t.border))
            .child(
                div()
                    .w(px(160.))
                    .flex_none()
                    .text_color(t.text_muted)
                    .child(term),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(value),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(div().flex().flex_col().children(list))
        .child(runs_on(this, t, cx))
        .into_any_element()
}

/// Where tasks run: this computer, or one of the SSH hosts in `~/.ssh/config`, which Add Project
/// then opens on. With no hosts, a greyed row says so, and the arrow keys still reach it so
/// keyboard users and agents can read it.
fn runs_on(this: &Onboarding, t: &Theme, cx: &mut Context<Onboarding>) -> impl IntoElement {
    let hosts = this.hosts.clone().unwrap_or_default();
    let current = this
        .settings
        .default_machine
        .clone()
        .filter(|d| hosts.contains(d));
    let option = |i: usize, label: &str, checked: bool| {
        div()
            .id(SharedString::from(format!("setup-runs-on-{i}")))
            .role(Role::RadioButton)
            .aria_label(SharedString::from(label.to_string()))
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .when_some(this.runs_on.get(i), gpui::InteractiveElement::track_focus)
            .flex()
            .w_full()
            .items_center()
            .gap(px(16.))
            .rounded(px(RADIUS))
            .border_1()
            .p(px(16.))
            .text_color(t.text)
    };
    let tile = |name: &str| {
        div()
            .size(px(40.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(RADIUS))
            .bg(t.bg_selected_muted)
            .text_color(t.accent_text)
            .child(icon(name, 20.))
    };
    let label = |text: &str| {
        div()
            .flex_1()
            .min_w_0()
            .text_size(px(FS_LG))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text.to_string())
    };
    let hover = t.bg_hover;
    let choices = std::iter::once(None)
        .chain(hosts.iter().cloned().map(Some))
        .collect::<Vec<_>>();
    let rows = choices
        .iter()
        .enumerate()
        .map(|(i, machine)| {
            let checked = *machine == current;
            let name = machine.as_deref().unwrap_or("This computer");
            let pick = machine.clone();
            let row = option(i, name, checked)
                .cursor_pointer()
                .child(tile(if machine.is_some() {
                    "hard-drives"
                } else {
                    "laptop"
                }))
                .child(label(name))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let pick = pick.clone();
                    this.save(|s| s.default_machine = pick, cx);
                }));
            if checked {
                picked(row, t).tab_index(0).into_any_element()
            } else {
                row.border_color(t.border)
                    .hover(move |s| s.bg(hover))
                    .into_any_element()
            }
        })
        .collect::<Vec<_>>();
    let none = hosts.is_empty().then(|| {
        option(1, "No remote devices available", false)
            .border_color(t.border)
            .child(tile("hard-drives").opacity(0.6))
            .child(label("No remote devices available").text_color(t.text_muted))
    });
    div()
        .mt(px(8.))
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(heading("Where tasks run"))
        .child(
            div()
                .id("setup-runs-on")
                .role(Role::RadioGroup)
                .aria_label("Where tasks run")
                .flex()
                .flex_col()
                .gap(px(8.))
                // Arrow keys pick the next or previous place, as in any radio group. With no
                // hosts they only move between the two rows.
                .on_key_down(
                    cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                        let step: isize = match e.keystroke.key.as_str() {
                            "left" | "up" => -1,
                            "right" | "down" => 1,
                            _ => return,
                        };
                        cx.stop_propagation();
                        if choices.len() == 1 {
                            let at = usize::from(
                                this.runs_on.get(1).is_some_and(|f| f.is_focused(window)),
                            );
                            if let Some(next) = this.runs_on.get(1 - at).cloned() {
                                window.focus(&next, cx);
                            }
                            return;
                        }
                        let current = this.settings.default_machine.clone();
                        let at = choices.iter().position(|c| *c == current).unwrap_or(0) as isize;
                        let len = choices.len() as isize;
                        let next = ((at + step + len) % len) as usize;
                        let pick = choices.get(next).cloned().flatten();
                        this.save(|s| s.default_machine = pick, cx);
                        if let Some(focus) = this.runs_on.get(next).cloned() {
                            window.focus(&focus, cx);
                        }
                    }),
                )
                .children(rows)
                .children(none),
        )
}
