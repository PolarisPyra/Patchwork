//! Extensions → Skills: the skills folder every agent loads, and the
//! skills in it, kept up to date as it changes.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::Sizable as _;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{AnyElement, FontWeight, PathPromptOptions, SharedString, Window, div, prelude::*, px};
use patchwork_client::local::core::Core;
use patchwork_client::local::core::extensions::skills::{self, Skill};

use crate::settings::SettingsDialog;
use crate::settings::controls::{Section, letter_badge, switch, token_row};
use patchwork_design_icons::IconName;
use patchwork_design_theme::theme;

use patchwork_design_icons::icon;

use patchwork_views_chat::{sigils::SigilKind, sigils::default_sigil};
use crate::settings::extensions::{empty, example, filter_select};

/// How a skills folder is laid out: one folder per skill, named like the skill, with its SKILL.md
/// and anything the skill refers to.
const SKILLS_EXAMPLE: &str = "skills/
├── release-notes/
│   ├── SKILL.md
│   └── template.md
└── review-ui/
    ├── SKILL.md
    └── scripts/
        └── measure.sh

release-notes/SKILL.md
---
name: release-notes
description: Draft release notes from the commits since the last tag
---
Read template.md, then list the commits since the last tag…";

/// The watch on the skills folder, stopped when the dialog closes.
pub struct SkillsWatch(Arc<Core>);

impl Drop for SkillsWatch {
    fn drop(&mut self) {
        self.0.unwatch_skills();
    }
}

impl SettingsDialog {
    /// Lists the skills, then again after each change to their folder. A folder that can't be
    /// watched still lists its skills.
    fn watch_skills(&mut self, cx: &mut Context<Self>) {
        if self.extensions.skills_watch.is_some() {
            return;
        }
        self.extensions.skills_watch = Some(SkillsWatch(Arc::clone(&self.core)));
        let (changed, mut received) = mpsc::unbounded::<Vec<Skill>>();
        let core = Arc::clone(&self.core);
        self.run(
            move || {
                let watched = core.watch_skills(move |list| {
                    let _ = changed.unbounded_send(list);
                });
                (skills::all(&core.settings()), watched.err())
            },
            |d, (list, error), _| {
                d.extensions.skills = Some(list);
                if error.is_some() {
                    d.extensions.error = error.map(|e| e.to_string());
                }
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            while let Some(list) = received.next().await {
                if this
                    .update(cx, |d, cx| {
                        d.extensions.skills = Some(list);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// Uses `dir` as the skills folder, or the default for None, and watches it.
    fn choose_skills_dir(&mut self, dir: Option<String>, cx: &mut Context<Self>) {
        self.extensions.error = None;
        self.update(|s| s.skills_dir = dir, cx);
        self.extensions.skills_watch = None;
        self.watch_skills(cx);
    }

    fn pick_skills_dir(&mut self, cx: &mut Context<Self>) {
        patchwork_design_tasks::pick_paths(
            cx,
            PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: None,
            },
            move |d, paths, cx| {
                let Some(dir) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                    return;
                };
                d.choose_skills_dir(Some(dir), cx);
            },
        );
    }

    fn toggle_skill(&mut self, name: String, on: bool, cx: &mut Context<Self>) {
        if let Some(skill) = self
            .extensions
            .skills
            .iter_mut()
            .flatten()
            .find(|s| s.name == name)
        {
            skill.enabled = on;
        }
        self.update(
            |s| {
                s.disabled_skills.retain(|n| *n != name);
                if !on {
                    s.disabled_skills.push(name);
                }
            },
            cx,
        );
    }
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    d.watch_skills(cx);
    let t = *theme(cx);
    let Some(all) = d.extensions.skills.clone() else {
        return div().into_any_element();
    };
    let dir = skills::dir(&d.settings);
    let custom_dir = d.settings.skills_dir.is_some();
    let filter = d.extensions.skill_filter;
    let typed = d.extensions.skill_query.clone();
    let query = typed.trim().to_lowercase();
    let on = all.iter().filter(|s| s.enabled).count();
    let shown: Vec<&Skill> = all
        .iter()
        .filter(|s| {
            filter.is_none_or(|f| s.enabled == f)
                && (query.is_empty()
                    || format!("{} {}", s.name, s.description.as_deref().unwrap_or(""))
                        .to_lowercase()
                        .contains(&query))
        })
        .collect();

    let search = d.search_field(
        "skill-search",
        "Search skills",
        &typed,
        |s, text| s.skill_query = text,
        window,
        cx,
    );
    let filter = filter_select(
        d,
        "skills-filter",
        "Show",
        vec![
            (None, "All", all.len()),
            (Some(true), "On", on),
            (Some(false), "Off", all.len() - on),
        ],
        filter,
        |d, want, cx| {
            d.extensions.skill_filter = want;
            cx.notify();
        },
        cx,
    );
    let open_dir = dir.clone();
    let tools = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(filter)
        .child(search)
        .child(
            Button::new("skills-open")
                .small()
                .accessibility_label("Open folder")
                .px(px(10.))
                .child(icon(IconName::FolderSimple, 13.))
                .child("Open folder")
                .on_click(cx.listener(move |d, _, _, cx| d.open_folder(open_dir.clone(), cx))),
        );

    let rows: Vec<AnyElement> = shown
        .iter()
        .map(|skill| {
            let (path, name, enabled) = (skill.path.clone(), skill.name.clone(), skill.enabled);
            let label = format!("{}{}", default_sigil(SigilKind::Skills), skill.name);
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(10.))
                .when(!enabled, |row| row.opacity(0.55))
                .child(letter_badge(&skill.name, 24.))
                .child(
                    div().min_w_0().flex_1().child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label.clone()),
                    ),
                )
                .child(
                    Button::new(SharedString::from(format!("skill-edit-{}", skill.path)))
                        .small()
                        .accessibility_label(format!("Edit {}", skill.name))
                        .child("Edit")
                        .on_click(cx.listener(move |d, _, _, cx| d.edit_file(&path, cx))),
                )
                .child(
                    switch(
                        SharedString::from(format!("skill-on-{}", skill.path)),
                        label,
                        enabled,
                        &t,
                    )
                    .on_click(
                        cx.listener(move |d, _, _, cx| d.toggle_skill(name.clone(), !enabled, cx)),
                    ),
                )
                .into_any_element()
        })
        .collect();
    let list = if rows.is_empty() {
        vec![
            empty(
                if query.is_empty() {
                    "No skills".to_string()
                } else {
                    format!("Nothing matches “{}”", typed.trim())
                },
                &t,
            )
            .into_any_element(),
        ]
    } else {
        rows
    };

    let folder = token_row("Folder", &t)
        .when(custom_dir, |row| {
            row.child(
                Button::new("skills-default")
                    .small()
                    .label("Use default")
                    .on_click(cx.listener(|d, _, _, cx| d.choose_skills_dir(None, cx))),
            )
        })
        .child(
            Button::new("skills-change")
                .small()
                .label("Change…")
                .on_click(cx.listener(|d, _, _, cx| d.pick_skills_dir(cx))),
        );
    div()
        .flex()
        .flex_col()
        .child(Section::new("Skills").action(tools).children(list))
        .child(Section::new("Where they live").child(folder))
        .child(example(SKILLS_EXAMPLE, &t))
        .into_any_element()
}
