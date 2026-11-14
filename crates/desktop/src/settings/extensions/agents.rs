//! Extensions → Agents: the custom agents that `&name` summons, kept up
//! to date as their folder changes.

// Rust guideline compliant 2026-10-09

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{AnyElement, FontWeight, SharedString, Window, div, prelude::*, px};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, letter_badge, token_row};
use crate::ui::theme::{font_mono, FS_SM, FS_XS, theme};
use crate::ui::{BtnKind, btn, icon, icon_button};

use crate::ui::sigils::{SigilKind, default_sigil};
use super::{empty, example, filter_chip, list_box};

/// The field a new agent's name is typed in.
const DRAFT_FIELD: &str = "agent-draft";

/// How the agents folder is laid out: one Markdown file per agent, named like the agent.
const AGENTS_EXAMPLE: &str = "agents/
├── dev-engineer.md
└── doc-scout.md

dev-engineer.md
---
name: DevEngineer
description: Senior developer: plans, implements features and fixes with explicit risk control
mode: primary
model: claude-sonnet-5-5
---
You are DevEngineer. Plan before you change anything…";

impl SettingsDialog {
    /// Redraws the dialog after each change to the agents folder.
    fn watch_agents(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.extensions.watching_agents, true) {
            return;
        }
        let (changed, mut received) = mpsc::unbounded::<()>();
        self.server.agents.subscribe(move |_| {
            let _ = changed.unbounded_send(());
        });
        cx.spawn(async move |this, cx| {
            while received.next().await.is_some() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn open_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.extensions.error = None;
        self.extensions.draft = Some(String::new());
        self.drop_fields(DRAFT_FIELD);
        // The field exists once drawn, so it takes focus as the web's `autoFocus` gives it.
        let _ = render(self, window, cx);
        if let Some(area) = self.field_area(DRAFT_FIELD) {
            window.focus(&gpui::Focusable::focus_handle(area.read(cx), cx), cx);
        }
        cx.notify();
    }

    /// Closes the new agent's row, if it is open; whether it was.
    pub(in crate::settings) fn cancel_draft(&mut self, cx: &mut Context<Self>) -> bool {
        if self.extensions.draft.take().is_none() {
            return false;
        }
        self.drop_fields(DRAFT_FIELD);
        cx.notify();
        true
    }

    fn create_agent(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self
            .extensions
            .draft
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        else {
            return;
        };
        let name = name.to_string();
        let server = std::sync::Arc::clone(&self.server);
        self.extensions.error = None;
        self.run(
            move || server.agents.create(&name),
            |d, out, cx| match out {
                Ok(()) => {
                    d.cancel_draft(cx);
                }
                Err(e) => d.extensions.error = Some(e.to_string()),
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
    d.watch_agents(cx);
    let t = *theme(cx);
    let agents = d.server.agents.list();
    let dir = d.server.agents.dir().to_string_lossy().into_owned();
    let state = &d.extensions;
    let (query, primary_only) = (state.agent_query.trim().to_lowercase(), state.primary_only);
    let primaries = agents.iter().filter(|a| a.primary).count();
    let shown: Vec<_> = agents
        .iter()
        .filter(|a| {
            (!primary_only || a.primary)
                && (query.is_empty()
                    || format!(
                        "{} {} {}",
                        a.name,
                        a.title.as_deref().unwrap_or(""),
                        a.description.as_deref().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&query))
        })
        .cloned()
        .collect();
    let typed = state.agent_query.clone();
    let draft = state.draft.clone();

    let search = d.search_field(
        "agent-search",
        "Search agents",
        &typed,
        |s, text| s.agent_query = text,
        window,
        cx,
    );
    let tools = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .pb(px(12.))
        .child(
            filter_chip("agents-all", "All", agents.len(), !primary_only, &t).on_click(
                cx.listener(|d, _, _, cx| {
                    d.extensions.primary_only = false;
                    cx.notify();
                }),
            ),
        )
        .child(
            filter_chip("agents-primary", "Primary", primaries, primary_only, &t).on_click(
                cx.listener(|d, _, _, cx| {
                    d.extensions.primary_only = !d.extensions.primary_only;
                    cx.notify();
                }),
            ),
        )
        .child(search)
        .child(
            btn("agent-new", "New agent", BtnKind::Plain, false, &t)
                .px(px(10.))
                .child(icon("plus", 13.))
                .child("New agent")
                .on_click(cx.listener(|d, _, window, cx| d.open_draft(window, cx))),
        );

    let mut rows: Vec<AnyElement> = Vec::new();
    if let Some(draft) = &draft {
        let field = d.field(
            DRAFT_FIELD,
            "New agent's name",
            draft,
            "agent-name",
            FieldKind {
                live: true,
                mono: true,
                ..Default::default()
            },
            |d, text, cx| {
                if d.extensions.draft.is_some() {
                    d.extensions.draft = Some(text);
                }
                cx.notify();
            },
            window,
            cx,
        );
        d.on_submit(DRAFT_FIELD, |d, _, cx| d.create_agent(cx));
        let name = draft.trim();
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(8.))
                .child(letter_badge(if name.is_empty() { "?" } else { name }, 24.))
                .child(field.flex_1())
                .child(
                    btn(
                        "agent-create",
                        "Create",
                        BtnKind::Plain,
                        name.is_empty(),
                        &t,
                    )
                    .child("Create")
                    .when(!name.is_empty(), |b| {
                        b.on_click(cx.listener(|d, _, _, cx| d.create_agent(cx)))
                    }),
                )
                .child(
                    icon_button("agent-cancel", "Cancel", "x", 14., cx).on_click(cx.listener(
                        |d, _, _, cx| {
                            d.cancel_draft(cx);
                        },
                    )),
                )
                .into_any_element(),
        );
    }
    for agent in &shown {
        let path = agent.path.clone();
        rows.push(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(8.))
                .child(letter_badge(&agent.name, 24.))
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .flex()
                                .min_w_0()
                                .items_center()
                                .gap(px(6.))
                                .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(
                                    format!("{}{}", default_sigil(SigilKind::Agents), agent.name),
                                ))
                                .when(agent.primary, |row| {
                                    row.child(
                                        div()
                                            .flex_none()
                                            .rounded(px(4.))
                                            .bg(t.bg_accent_soft)
                                            .px(px(6.))
                                            .text_size(px(FS_XS))
                                            .text_color(t.accent_text)
                                            .child("Primary"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(FS_SM))
                                .text_color(t.text_muted)
                                .child(agent.description.clone().unwrap_or_default()),
                        ),
                )
                .children(agent.model.clone().map(|model| {
                    div()
                        .flex_none()
                        .max_w(px(180.))
                        .truncate()
                        .font_family(font_mono())
                        .text_size(px(FS_XS))
                        .text_color(t.text_muted)
                        .child(model)
                }))
                .child(
                    btn(
                        SharedString::from(format!("agent-edit-{}", agent.path)),
                        format!("Edit {}", agent.name),
                        BtnKind::Plain,
                        false,
                        &t,
                    )
                    .child("Edit")
                    .on_click(cx.listener(move |d, _, _, cx| d.edit_file(&path, cx))),
                )
                .into_any_element(),
        );
    }

    let list = if rows.is_empty() {
        empty(
            if !query.is_empty() {
                format!("Nothing matches “{}”", typed.trim())
            } else if primary_only {
                "No primary agents".into()
            } else {
                "No agents".into()
            },
            &t,
        )
    } else {
        list_box(rows, &t)
    };
    div()
        .flex()
        .flex_col()
        .child(tools)
        .child(list)
        .child(
            div()
                .mt(px(12.))
                .child(token_row("Folder", Some(&dir), &t))
                .child(token_row("In a project", Some(".patchwork/agents"), &t)),
        )
        .child(example(AGENTS_EXAMPLE, &t))
        .into_any_element()
}
