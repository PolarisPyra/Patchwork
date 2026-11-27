//! Settings → Agents, Skills and Hooks: the three pages of what agents can be given, their
//! search, their filter, and the actions they share.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::PathBuf;
use gpui::{AnyElement, Div, SharedString, Stateful, Window, div, prelude::*, px};
use patchwork_backend::extensions::hooks::Bundle;
use patchwork_backend::extensions::skills::Skill;
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, choices, search_box, select_box};
use patchwork_ui_theme::theme;
use super::agents;
use super::skills;
use super::hooks;

/// Which of the three pages is drawn.
#[derive(Clone, Copy, PartialEq, Default)]
pub(in crate::settings) enum Tab {
    #[default]
    Agents,
    Skills,
    Hooks,
}

#[derive(Default)]
pub struct ExtensionsState {
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
                patchwork_platform::open_in_editor(
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
                patchwork_platform::files::paths::make_folder(&dir)
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
        let t = *theme(cx);
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
        search_box(field, &t).w(px(208.)).flex_none()
    }
}

/// The dropdown at the right of a list's heading that narrows the list, each option with its
/// count, in place of a row of chips.
pub(in crate::settings) fn filter_select<T: Clone + PartialEq + 'static>(
    d: &SettingsDialog,
    key: &'static str,
    label: &'static str,
    options: Vec<(T, &'static str, usize)>,
    chosen: T,
    set: impl Fn(&mut SettingsDialog, T, &mut Context<SettingsDialog>) + 'static,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let t = *theme(cx);
    let current = options
        .iter()
        .find(|(value, _, _)| *value == chosen)
        .map_or_else(String::new, |(_, name, count)| format!("{name} · {count}"));
    let options = options
        .into_iter()
        .map(|(value, name, count)| (value, SharedString::from(format!("{name} · {count}"))))
        .collect();
    select_box(key, label, current, d.menu_open(key), &t)
        .min_w(px(112.))
        .on_click(choices(key, label, options, chosen, set, cx))
}

pub fn render(
    d: &mut SettingsDialog,
    tab: Tab,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let body = match tab {
        Tab::Agents => agents::render(d, window, cx),
        Tab::Skills => skills::render(d, window, cx),
        Tab::Hooks => hooks::render(d, cx),
    };
    div()
        .flex()
        .flex_col()
        .children(
            d.extensions
                .error
                .clone()
                .map(|e| alert("extensions-error", e, t.danger).pt_0().pb(px(12.))),
        )
        .child(body)
        .into_any_element()
}
