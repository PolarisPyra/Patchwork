//! Extensions → Skills (`agents/SkillsPane.tsx`): the skills folder every agent loads, and the
//! skills in it, kept up to date as it changes.

use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, Context, FontWeight, PathPromptOptions, SharedString, Window, div, prelude::*, px,
};
use patchwork_server::Server;
use patchwork_server::skills::{self, Skill};

use crate::settings::SettingsDialog;
use crate::settings::controls::{letter_badge, switch, token_row};
use crate::ui::theme::{FS_SM, theme};
use crate::ui::{BtnKind, btn, icon};

use super::{SKILL_SIGIL, empty, example, filter_chip, list_box};

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
pub struct SkillsWatch(Arc<Server>);

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
        self.extensions.skills_watch = Some(SkillsWatch(Arc::clone(&self.server)));
        let (changed, mut received) = mpsc::unbounded::<Vec<Skill>>();
        let server = Arc::clone(&self.server);
        self.run(
            move || {
                let watched = server.watch_skills(move |list| {
                    let _ = changed.unbounded_send(list);
                });
                (skills::all(&server.store.settings()), watched.err())
            },
            |d, (list, error), _| {
                d.extensions.skills = Some(list);
                if error.is_some() {
                    d.extensions.error = error;
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
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(dir) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                return;
            };
            let _ = this.update(cx, |d, cx| d.choose_skills_dir(Some(dir), cx));
        })
        .detach();
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
    let pick = |want: Option<bool>| {
        cx.listener(move |d: &mut SettingsDialog, _, _, cx| {
            let current = d.extensions.skill_filter;
            d.extensions.skill_filter = if want.is_some() && current == want {
                None
            } else {
                want
            };
            cx.notify();
        })
    };
    let open_dir = dir.clone();
    let tools = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pb(px(12.))
        .child(
            filter_chip("skills-all", "All", all.len(), filter.is_none(), &t).on_click(pick(None)),
        )
        .child(
            filter_chip("skills-on", "On", on, filter == Some(true), &t).on_click(pick(Some(true))),
        )
        .child(
            filter_chip(
                "skills-off",
                "Off",
                all.len() - on,
                filter == Some(false),
                &t,
            )
            .on_click(pick(Some(false))),
        )
        .child(search)
        .child(
            btn("skills-open", "Open folder", BtnKind::Plain, false, &t)
                .px(px(10.))
                .child(icon("folder-simple", 13.))
                .child("Open folder")
                .on_click(cx.listener(move |d, _, _, cx| d.open_folder(open_dir.clone(), cx))),
        );

    let rows: Vec<AnyElement> = shown
        .iter()
        .map(|skill| {
            let (path, name, enabled) = (skill.path.clone(), skill.name.clone(), skill.enabled);
            let label = format!("{SKILL_SIGIL}{}", skill.name);
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(8.))
                .when(!enabled, |row| row.opacity(0.55))
                .child(letter_badge(&skill.name, 24.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(FS_SM))
                                .text_color(t.text_muted)
                                .child(skill.description.clone().unwrap_or_default()),
                        ),
                )
                .child(
                    btn(
                        SharedString::from(format!("skill-edit-{}", skill.path)),
                        format!("Edit {}", skill.name),
                        BtnKind::Plain,
                        false,
                        &t,
                    )
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
        empty(
            if query.is_empty() {
                "No skills".to_string()
            } else {
                format!("Nothing matches “{}”", typed.trim())
            },
            &t,
        )
    } else {
        list_box(rows, &t)
    };

    let folder = token_row("Folder", Some(&dir.to_string_lossy()), &t)
        .when(custom_dir, |row| {
            row.child(
                btn("skills-default", "Use default", BtnKind::Plain, false, &t)
                    .child("Use default")
                    .on_click(cx.listener(|d, _, _, cx| d.choose_skills_dir(None, cx))),
            )
        })
        .child(
            btn("skills-change", "Change…", BtnKind::Plain, false, &t)
                .child("Change…")
                .on_click(cx.listener(|d, _, _, cx| d.pick_skills_dir(cx))),
        );
    div()
        .flex()
        .flex_col()
        .child(tools)
        .child(list)
        .child(div().mt(px(12.)).child(folder))
        .child(example(SKILLS_EXAMPLE, &t))
        .into_any_element()
}
