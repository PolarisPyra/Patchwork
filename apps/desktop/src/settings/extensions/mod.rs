//! Settings → Extensions (`Extensions` in `SettingsDialog.tsx`): what agents can be given, one
//! kind at a time: custom agents, skills, and hooks. Each tab draws itself in its own file.

mod agents;
mod hooks;
mod skills;

use std::collections::HashSet;
use std::path::PathBuf;

use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Stateful, Window, div, prelude::*, px};
use patchwork_server::plugins::Plugin;
use patchwork_server::skills::Skill;

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, chip, segmented};
pub(super) use crate::settings::controls::list_box;
use crate::theme::{FONT_MONO, FS_XS, RADIUS_SM, Theme, theme};
use crate::ui::icon;

/// What `&name` summons an agent with, and `$name` a skill (`DEFAULT_SIGILS`).
const AGENT_SIGIL: &str = "&";
const SKILL_SIGIL: &str = "$";

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Agents,
    Skills,
    Hooks,
}

#[derive(Default)]
pub struct ExtensionsState {
    tab: Tab,
    error: Option<String>,
    /// Whether the dialog redraws when the agents folder changes.
    watching_agents: bool,
    agent_query: String,
    primary_only: bool,
    /// The name being typed for a new agent, while its row is open.
    draft: Option<String>,
    /// The skills, kept up to date while the dialog is open.
    skills: Option<Vec<Skill>>,
    skills_watch: Option<skills::SkillsWatch>,
    skill_query: String,
    /// On or off only; None shows all.
    skill_filter: Option<bool>,
    plugins: Option<Vec<Plugin>>,
    asked_plugins: bool,
    /// The hooks opened to their details, by plugin id and place.
    open_hooks: HashSet<(String, usize)>,
}

impl SettingsDialog {
    /// Runs `work` off the main thread and shows its error, if any, under the tab.
    fn try_run(
        &mut self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.extensions.error = None;
        self.run(work, |d, out, _| d.extensions.error = out.err(), cx);
    }

    /// Opens the file at `path` in the editor.
    fn edit_file(&mut self, path: &str, cx: &mut Context<Self>) {
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
    fn open_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.try_run(
            move || {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                patchwork_server::system::launch::xdg_open(dir.as_os_str(), None)
                    .map_err(|e| e.to_string())
            },
            cx,
        );
    }

    /// The search field over a tab's list, with its glass at the left.
    fn search_field(
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
fn filter_chip(id: &str, label: &str, count: usize, on: bool, t: &Theme) -> Stateful<Div> {
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
fn empty(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .py(px(24.))
        .flex()
        .justify_center()
        .text_color(t.text_muted)
        .child(text.into())
}

/// How a folder is laid out, under an "Example" heading.
fn example(text: &'static str, t: &Theme) -> Div {
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
                .rounded(px(RADIUS_SM))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_editor)
                .px(px(12.))
                .py(px(10.))
                .font_family(FONT_MONO)
                .text_size(px(FS_XS))
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
