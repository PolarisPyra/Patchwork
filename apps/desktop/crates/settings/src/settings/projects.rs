//! Settings → Projects: the projects Patchwork knows, whether checks run after every task in each,
//! and the conversations other agents saved that can be imported.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_shared::{agent_label, agent_logo};
use std::sync::Arc;

use gpui::{AnyElement, FontWeight, SharedString, div, prelude::*, px};
use patchwork_backend::import::{ImportScan, ImportSource};

use crate::settings::controls::{Section, alert, switch, token_row};
use super::themes::project_look;
use crate::settings::{SettingsDialog, SettingsEvent};
use patchwork_ui_theme::{fs_sm, theme};
use patchwork_ui_components::{BtnKind, btn, icon_button, tooltip};

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
pub fn source_label(source: ImportSource) -> &'static str {
    match source {
        ImportSource::OpenCode => "OpenCode",
        _ => source.agent().map_or("", agent_label),
    }
}

impl SettingsDialog {
    fn scan_imports(&mut self, sources: Vec<ImportSource>, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        self.run(
            move || {
                sources
                    .into_iter()
                    .map(|s| core.scan_imports(s))
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
        let core = Arc::clone(&self.core);
        self.run(
            move || core.import_chats(&[source]),
            move |d, out, cx| {
                d.projects.busy = None;
                if let Err(e) = out {
                    d.projects.import_error = Some(e.to_string());
                }
                // Importing adds projects and chats on the backend.
                d.settings = d.core.settings();
                cx.emit(SettingsEvent::Changed);
                cx.emit(SettingsEvent::ChatsChanged);
                d.scan_imports(vec![source], cx);
            },
            cx,
        );
    }

    fn remove_project(&mut self, id: String, cx: &mut Context<Self>) {
        self.projects.error = None;
        let core = Arc::clone(&self.core);
        self.run(
            move || core.remove_project(&id),
            |d, out, cx| {
                if let Err(e) = out {
                    d.projects.error = Some(e.to_string());
                }
                d.settings = d.core.settings();
                cx.emit(SettingsEvent::Changed);
                cx.emit(SettingsEvent::ChatsChanged);
            },
            cx,
        );
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let sources = ImportSource::offered(&d.settings.agents);
    if !std::mem::replace(&mut d.projects.asked, true) {
        d.scan_imports(sources.clone(), cx);
    }
    let checks = d.settings.checks.clone();
    let after_every_task = checks.after_every_task;

    let default_row = token_row("Run checks after every task", &t).child(
        switch(
            "checks-default",
            "Run checks after every task",
            after_every_task,
            &t,
        )
        // In line with each project's switch, which has its remove button after it.
        .when(!d.settings.projects.is_empty(), |s| s.mr(px(40.)))
        .on_click(cx.listener(move |d, _, _, cx| {
            d.update(|s| s.checks.after_every_task = !after_every_task, cx);
        })),
    );

    let accent = t.accent_text;
    let named: Vec<_> = d
        .settings
        .projects
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    let mut looks: Vec<_> = named
        .iter()
        .map(|(id, name)| Some(project_look(d, id, name, cx)))
        .collect();
    let projects = d.settings.projects.iter().zip(&mut looks).map(|(p, look)| {
        let own = checks.projects.get(&p.id).copied();
        let on = own.unwrap_or(after_every_task);
        let (follow, toggle, remove) = (p.id.clone(), p.id.clone(), p.id.clone());
        div()
            .id(SharedString::from(format!("project-{}", p.id)))
            .role(gpui::Role::Group)
            .aria_label(SharedString::from(p.name.clone()))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(16.))
            .py(px(10.))
            .child(
                div().min_w_0().flex_1().child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(p.name.clone()),
                ),
            )
            .when(own.is_some(), |row| {
                row.child(
                    patchwork_ui_components::button(
                        SharedString::from(format!("checks-follow-{}", p.id)),
                        format!("Use the default checks setting in {}", p.name),
                    )
                    .whitespace_nowrap()
                    .text_size(px(fs_sm()))
                    .text_color(accent)
                    .hover(gpui::Styled::underline)
                    .child("Use the default")
                    .on_click(cx.listener(move |d, _, _, cx| {
                        let id = follow.clone();
                        d.update(
                            |s| {
                                s.checks.projects.remove(&id);
                            },
                            cx,
                        );
                    })),
                )
            })
            .children(look.take().into_iter().flatten())
            .child(
                div()
                    .text_size(px(fs_sm()))
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
                    );
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
        let agent = source.agent()?;
        let scan = d.projects.scans.iter().find(|s| s.kind == source);
        let disabled = busy.is_some() || scan.is_none_or(|s| s.new == 0);
        let label = source_label(source);
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(16.))
                .py(px(10.))
                .child(agent_logo(agent, None, 20.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label)),
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

    let run_setup = btn("run-setup", "Run setup again", BtnKind::Plain, false, &t)
        .child("Run setup again")
        .on_click(cx.listener(|d, _, _, cx| d.update(|s| s.onboarded = false, cx)));
    div()
        .flex()
        .flex_col()
        .children(
            d.projects
                .error
                .clone()
                .map(|e| alert("projects-error", e, t.danger).pt_0().pb(px(12.))),
        )
        .child(
            Section::new("Projects")
                .action(run_setup)
                .child(default_row)
                .when(no_projects, |col| {
                    col.child(
                        div()
                            .px(px(16.))
                            .py(px(10.))
                            .text_size(px(fs_sm()))
                            .text_color(t.text_subtle)
                            .child("No projects yet"),
                    )
                })
                .children(projects),
        )
        .child(
            Section::new("Imports").children(imports).children(
                d.projects
                    .import_error
                    .clone()
                    .map(|e| alert("imports-error", e, t.danger).px(px(16.)).pb(px(10.))),
            ),
        )
        .into_any_element()
}
