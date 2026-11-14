//! Settings → Extensions: its tabs, its search, and the actions its pages share.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::PathBuf;
use gpui::{AnyElement, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px};
use patchwork_server::hooks::Bundle;
use patchwork_server::skills::Skill;
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, chip, segmented};
use crate::ui::theme::{font_mono, fs_xs, radius_sm, Theme, theme};
use crate::ui::icon;
use super::agents;
use super::skills;
use super::hooks;

#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum Tab {
    #[default]
    Agents,
    Skills,
    Hooks,
}

#[derive(Default)]
pub struct ExtensionsState {
    pub(super) tab: Tab,
    pub(super) error: Option<String>,
    /// Whether the dialog redraws when the agents folder changes.
    pub(super) watching_agents: bool,
    pub(super) agent_query: String,
    pub(super) primary_only: bool,
    /// The name being typed for a new agent, while its row is open.
    pub(super) draft: Option<String>,
    /// The skills, kept up to date while the dialog is open.
    pub(super) skills: Option<Vec<Skill>>,
    pub(super) skills_watch: Option<skills::SkillsWatch>,
    pub(super) skill_query: String,
    /// On or off only; None shows all.
    pub(super) skill_filter: Option<bool>,
    pub(super) bundles: Option<Vec<Bundle>>,
    pub(super) asked_bundles: bool,
    /// The hooks opened to their details, by bundle id and place.
    pub(super) open_hooks: HashSet<(String, usize)>,
}

impl SettingsDialog {
    /// Runs `work` off the main thread and shows its error, if any, under the tab.
    pub(super) fn try_run(
        &mut self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.extensions.error = None;
        self.run(work, |d, out, _| d.extensions.error = out.err(), cx);
    }

    /// Opens the file at `path` in the editor.
    pub(super) fn edit_file(&mut self, path: &str, cx: &mut Context<Self>) {
        let path = PathBuf::from(path);
        self.try_run(
            move || {
                let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
                    return Err("That is not a file".into());
                };
                patchwork_server::workspace::open_in_editor(
                    &dir.to_string_lossy(),
                    &name.to_string_lossy(),
                    None,
                )
                .map_err(|e| e.to_string())
            },
            cx,
        );
    }

    /// Opens `dir` in the file manager, making it first.
    pub(super) fn open_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.extensions.error = None;
        self.run(
            move || {
                std::fs::create_dir_all(&dir)
                    .map(|()| dir)
                    .map_err(|e| e.to_string())
            },
            |d, made, cx| match made {
                Ok(dir) => cx.open_with_system(&dir),
                Err(e) => d.extensions.error = Some(e),
            },
            cx,
        );
    }

    /// The search field over a tab's list, with its glass at the left.
    pub(super) fn search_field(
        &mut self,
        key: &'static str,
        label: &'static str,
        query: &str,
        set: fn(&mut ExtensionsState, String),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = theme(cx).text_muted;
        let field = self.field(
            key,
            label,
            query,
            label,
            FieldKind {
                live: true,
                ..Default::default()
            },
            move |d, text, cx| {
                set(&mut d.extensions, text);
                cx.notify();
            },
            window,
            cx,
        );
        div()
            .relative()
            .flex()
            .items_center()
            .ml_auto()
            .w(px(208.))
            .flex_none()
            .child(field.w_full().pl(px(26.)))
            .child(
                div()
                    .absolute()
                    .left(px(8.))
                    .text_color(muted)
                    .child(icon("magnifying-glass", 13.)),
            )
    }
}

/// A filter chip with its count, filled when it is the one showing.
pub(super) fn filter_chip(
    id: &str,
    label: &str,
    count: usize,
    on: bool,
    t: &Theme,
) -> Stateful<Div> {
    chip(
        SharedString::from(id.to_string()),
        format!("{label} {count}"),
        on,
        t,
    )
    .child(label.to_string())
    .child(div().text_color(t.text_muted).child(count.to_string()))
}

/// What a list shows when nothing is in it.
pub(super) fn empty(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .py(px(24.))
        .flex()
        .justify_center()
        .text_color(t.text_muted)
        .child(text.into())
}

/// How a folder is laid out, under an "Example" heading.
pub(super) fn example(text: &'static str, t: &Theme) -> Div {
    div()
        .child(
            div()
                .mt(px(20.))
                .mb(px(6.))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Example"),
        )
        .child(
            div()
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_editor)
                .px(px(12.))
                .py(px(10.))
                .font_family(font_mono())
                .text_size(px(fs_xs()))
                .line_height(gpui::relative(1.6))
                .text_color(t.text_muted)
                .children(
                    text.lines()
                        .map(|line| div().whitespace_nowrap().child(line.to_string())),
                ),
        )
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let tab = d.extensions.tab;
    let body = match tab {
        Tab::Agents => agents::render(d, window, cx),
        Tab::Skills => skills::render(d, window, cx),
        Tab::Hooks => hooks::render(d, cx),
    };
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .mb(px(12.))
                .border_b_1()
                .border_color(t.border)
                .pt(px(4.))
                .pb(px(12.))
                .child(segmented(
                    "extensions-tab",
                    "Extensions",
                    tab,
                    &[
                        (Tab::Agents, "Agents"),
                        (Tab::Skills, "Skills"),
                        (Tab::Hooks, "Hooks"),
                    ],
                    &t,
                    |d, tab, cx| {
                        d.extensions.tab = tab;
                        d.extensions.error = None;
                        cx.notify();
                    },
                    cx,
                )),
        )
        .child(body)
        .children(
            d.extensions
                .error
                .clone()
                .map(|e| alert("extensions-error", e, t.danger).mt(px(8.))),
        )
        .into_any_element()
}
