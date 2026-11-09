//! Setup's last step (`ReadyStep.tsx`): what was chosen, and where tasks run.

use gpui::{AnyElement, Context, FontWeight, Role, SharedString, Toggled, div, prelude::*, px};

use super::{ImportResult, Onboarding};
use super::rows::{heading, picked};
use crate::ui::theme::{FS_LG, RADIUS, Theme};
use crate::ui::{agent_label, badge, icon};

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
        .map_or("None yet".to_string(), |p| p.name.clone());
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

/// Where tasks run. Only this computer for now; a remote device is listed but can't be picked
/// yet, and the arrow keys still reach it so keyboard users and agents can read why.
fn runs_on(this: &Onboarding, t: &Theme, cx: &mut Context<Onboarding>) -> impl IntoElement {
    let option = |id: &'static str, label: &'static str, checked: bool| {
        div()
            .id(id)
            .role(Role::RadioButton)
            .aria_label(label)
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
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
    let label = |text: &'static str| {
        div()
            .flex_1()
            .text_size(px(FS_LG))
            .font_weight(FontWeight::SEMIBOLD)
            .child(text)
    };
    let [here, remote] = &this.runs_on;
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
                // Arrow keys move between the options, as in any radio group.
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    if !matches!(e.keystroke.key.as_str(), "up" | "down" | "left" | "right") {
                        return;
                    }
                    cx.stop_propagation();
                    let at = usize::from(this.runs_on[1].is_focused(window));
                    let next = this.runs_on[1 - at].clone();
                    window.focus(&next, cx);
                }))
                .child(
                    picked(option("setup-this-computer", "This computer", true), t)
                        .track_focus(here)
                        .tab_index(0)
                        .child(tile("laptop"))
                        .child(label("This computer")),
                )
                .child(
                    option("setup-remote-device", "Remote device", false)
                        .track_focus(remote)
                        .aria_description("Coming soon")
                        .border_color(t.border)
                        .child(tile("hard-drives").opacity(0.6))
                        .child(label("Remote device").text_color(t.text_muted))
                        .child(badge("Coming soon", t)),
                ),
        )
}
