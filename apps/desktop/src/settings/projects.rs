//! Settings → Projects (`system/ProjectsPane.tsx` and `ImportsPane.tsx`): the projects Patchwork
//! knows, whether checks run after every task in each, and the conversations other agents saved
//! that can be imported.

use std::sync::Arc;

use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use patchwork_server::import::{ImportScan, ImportSource};
use patchwork_server::provider::AgentKind;

use crate::settings::controls::{alert, section_heading, switch, token_row};
use crate::settings::{SettingsDialog, SettingsEvent};
use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, theme};
use crate::ui::{AGENT_KINDS, BtnKind, agent_label, agent_logo, btn, icon_button, tooltip};

#[derive(Default)]
pub struct ProjectsState {
    error: Option<String>,
    /// What each source holds, once scanned.
    scans: Vec<ImportScan>,
    asked: bool,
    /// The source being imported.
    busy: Option<ImportSource>,
    import_error: Option<String>,
}

/// The name a source shows (`SOURCE_LABELS`): its agent's, but OpenCode's files are plain "OpenCode".
pub(crate) fn source_label(source: ImportSource) -> &'static str {
    match source {
        ImportSource::OpenCode => "OpenCode",
        _ => source_agent(source).map_or("", agent_label),
    }
}

/// The agent whose name and logo a source shows.
pub(crate) fn source_agent(source: ImportSource) -> Option<AgentKind> {
    match source {
        ImportSource::Claude => Some(AgentKind::Claude),
        ImportSource::Codex => Some(AgentKind::Codex),
        ImportSource::Antigravity => Some(AgentKind::Antigravity),
        ImportSource::OpenCode => Some(AgentKind::OpenCode),
        ImportSource::T3 => None,
    }
}

/// The sources to offer (`importSourcesFor`): each connected agent's own files, then OpenCode.
pub(crate) fn import_sources(agents: &[AgentKind]) -> Vec<ImportSource> {
    AGENT_KINDS
        .into_iter()
        .filter(|k| agents.contains(k))
        .filter_map(|k| match k {
            AgentKind::Claude => Some(ImportSource::Claude),
            AgentKind::Codex => Some(ImportSource::Codex),
            AgentKind::Antigravity => Some(ImportSource::Antigravity),
            AgentKind::OpenCode | AgentKind::Api => None,
        })
        .chain([ImportSource::OpenCode])
        .collect()
}

impl SettingsDialog {
    fn scan_imports(&mut self, sources: Vec<ImportSource>, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        self.run(
            move || {
                sources
                    .into_iter()
                    .map(|s| server.scan_imports(s))
                    .collect::<Vec<_>>()
            },
            |d, scans, _| {
                d.projects
                    .scans
                    .retain(|s| !scans.iter().any(|n| n.kind == s.kind));
                d.projects.scans.extend(scans);
            },
            cx,
        );
    }

    fn import_from(&mut self, source: ImportSource, cx: &mut Context<Self>) {
        self.projects.busy = Some(source);
        self.projects.import_error = None;
        let server = Arc::clone(&self.server);
        self.run(
            move || server.import_threads(&[source]),
            move |d, out, cx| {
                d.projects.busy = None;
                if let Err(e) = out {
                    d.projects.import_error = Some(e);
                }
                // Importing adds projects and threads on the backend.
                d.settings = d.server.store.settings();
                cx.emit(SettingsEvent::Changed);
                cx.emit(SettingsEvent::ThreadsChanged);
                d.scan_imports(vec![source], cx);
            },
            cx,
        );
    }

    fn remove_project(&mut self, id: String, cx: &mut Context<Self>) {
        self.projects.error = None;
        let server = Arc::clone(&self.server);
        self.run(
            move || server.remove_project(&id),
            |d, out, cx| {
                if let Err(e) = out {
                    d.projects.error = Some(e);
                }
                d.settings = d.server.store.settings();
                cx.emit(SettingsEvent::Changed);
                cx.emit(SettingsEvent::ThreadsChanged);
            },
            cx,
        );
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let sources = import_sources(&d.settings.agents);
    if !std::mem::replace(&mut d.projects.asked, true) {
        d.scan_imports(sources.clone(), cx);
    }
    let checks = d.settings.checks.clone();
    let after_every_task = checks.after_every_task;

    let default_row = token_row(
        "Run checks after every task",
        Some("The default for projects that have not answered for themselves. Only a task that changed a file is checked."),
        &t,
    )
    .child(
        switch("checks-default", "Run checks after every task", after_every_task, &t).on_click(cx.listener(
            move |d, _, _, cx| d.update(|s| s.checks.after_every_task = !after_every_task, cx),
        )),
    );

    let accent = t.accent_text;
    let projects = d.settings.projects.iter().map(|p| {
        let own = checks.projects.get(&p.id).copied();
        let on = own.unwrap_or(after_every_task);
        let (follow, toggle, remove) = (p.id.clone(), p.id.clone(), p.id.clone());
        div()
            .id(SharedString::from(format!("project-{}", p.id)))
            .role(gpui::Role::Group)
            .aria_label(SharedString::from(p.name.clone()))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border)
            .py(px(8.))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(p.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(FONT_MONO)
                            .text_size(px(FS_XS))
                            .text_color(t.text_muted)
                            .child(p.path.clone()),
                    ),
            )
            .when(own.is_some(), |row| {
                row.child(
                    crate::ui::button(
                        SharedString::from(format!("checks-follow-{}", p.id)),
                        format!("Use the default checks setting in {}", p.name),
                    )
                    .whitespace_nowrap()
                    .text_size(px(FS_SM))
                    .text_color(accent)
                    .hover(|s| s.underline())
                    .child("Use the default")
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let id = follow.clone();
                        d.update(
                            |s| {
                                s.checks.projects.remove(&id);
                            },
                            cx,
                        )
                    })),
                )
            })
            .child(
                div()
                    .text_size(px(FS_SM))
                    .text_color(t.text_muted)
                    .child("Checks"),
            )
            .child(
                switch(
                    SharedString::from(format!("checks-{}", p.id)),
                    format!("Run checks after every task in {}", p.name),
                    on,
                    &t,
                )
                .on_click(cx.listener(move |d, _, _, cx| {
                    let id = toggle.clone();
                    d.update(
                        |s| {
                            s.checks.projects.insert(id, !on);
                        },
                        cx,
                    )
                })),
            )
            .child(
                icon_button(
                    SharedString::from(format!("project-remove-{}", p.id)),
                    format!("Remove {}", p.name),
                    "trash",
                    14.,
                    cx,
                )
                .tooltip(tooltip("Remove project"))
                .on_click(cx.listener(move |d, _, _, cx| d.remove_project(remove.clone(), cx))),
            )
    });
    let projects: Vec<_> = projects.collect();
    let no_projects = projects.is_empty();

    let busy = d.projects.busy;
    let imports = sources.into_iter().filter_map(|source| {
        let agent = source_agent(source)?;
        let scan = d.projects.scans.iter().find(|s| s.kind == source);
        let desc = match scan {
            None => "Looking for saved conversations…".to_string(),
            Some(s) if s.new > 0 => {
                format!("{} of {} conversations not imported yet", s.new, s.found)
            }
            Some(s) if s.found > 0 => format!("All {} conversations imported", s.found),
            Some(_) => "No saved conversations".to_string(),
        };
        let hidden = scan.map_or(0, |s| s.subagents);
        let disabled = busy.is_some() || scan.is_none_or(|s| s.new == 0);
        let label = source_label(source);
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .border_b_1()
                .border_color(t.border)
                .py(px(10.))
                .child(agent_logo(agent, None, 20.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                        .child(
                            div()
                                .text_size(px(FS_SM))
                                .text_color(t.text_muted)
                                .child(desc),
                        )
                        .when(hidden > 0, |col| {
                            col.child(div().text_size(px(FS_SM)).text_color(t.text_muted).child(
                                format!(
                                    "{hidden} sub-agent {} hidden",
                                    if hidden == 1 {
                                        "conversation"
                                    } else {
                                        "conversations"
                                    }
                                ),
                            ))
                        }),
                )
                .child(
                    btn(
                        SharedString::from(format!("import-{label}")),
                        format!("Import from {label}"),
                        BtnKind::Plain,
                        disabled,
                        &t,
                    )
                    .child(if busy == Some(source) {
                        "Importing…"
                    } else {
                        "Import"
                    })
                    .when(!disabled, |b| {
                        b.on_click(cx.listener(move |d, _, _, cx| d.import_from(source, cx)))
                    }),
                ),
        )
    });
    let imports: Vec<_> = imports.collect();

    div()
        .flex()
        .flex_col()
        .child(default_row)
        .when(no_projects, |col| {
            col.child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(FS_SM))
                    .text_color(t.text_subtle)
                    .child("No projects yet"),
            )
        })
        .children(projects)
        .children(
            d.projects
                .error
                .clone()
                .map(|e| alert("projects-error", e, t.danger)),
        )
        .child(
            div().mt(px(16.)).flex().child(
                btn("run-setup", "Run setup again", BtnKind::Plain, false, &t)
                    .child("Run setup again")
                    .on_click(cx.listener(|d, _, _, cx| d.update(|s| s.onboarded = false, cx))),
            ),
        )
        .child(section_heading("Imports", false, &t))
        .children(imports)
        .children(
            d.projects
                .import_error
                .clone()
                .map(|e| alert("imports-error", e, t.danger)),
        )
        .into_any_element()
}
