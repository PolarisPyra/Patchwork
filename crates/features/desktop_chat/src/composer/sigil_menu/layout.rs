//! The menu drawn over the chat box while a `/` command, `$` skill, `@` file or `&` agent is
//! typed, as the web composer's slash and mention menus.

// Rust guideline compliant 2026-10-09

use super::commands::scored;
use super::commands::Item;
use super::commands::commands;
use super::commands::file_items;
use super::commands::token;
use super::commands::{Runs, runs_of};
use gpui::{AnyElement, FontWeight, Role, SharedString, deferred, div, prelude::*, px, relative};
use patchwork_model::provider::{AgentKind, ProviderSlashCommand};
use crate::composer::Composer;
use crate::composer::menus::Roles;
use patchwork_views_chat::{sigils::SigilKind, sigils::Sigils};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, font_mono, fs_xs};
use patchwork_design_components::button;
use patchwork_design_icons::icon;

impl Composer {
    /// Works out the menu for the word before the caret, reading its list when it opens, and
    /// draws it when it shows.
    pub(in crate::composer) fn sigil_menu(
        &mut self,
        agent: AgentKind,
        provider: &[ProviderSlashCommand],
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sigils = cx.try_global::<Sigils>().cloned().unwrap_or_default();
        let editor = self.editor.read(cx);
        let token = editor
            .caret()
            .and_then(|caret| token(&editor.content, caret, &sigils))
            // Antigravity has no skills to offer; Patchwork's own commands run for it as for
            // any agent.
            .filter(|tk| agent != AgentKind::Antigravity || tk.kind != SigilKind::Skills);
        let kind = token.as_ref().map(|tk| tk.kind);
        let project = self.host.project(cx).map(|p| p.id.clone());
        let typed = kind.map(|kind| (kind, project));
        if typed != self.sigil.kind {
            self.sigil.kind = typed;
            match kind {
                Some(SigilKind::Files) => self.load_files(cx),
                Some(SigilKind::Skills) => self.load_skills(cx),
                Some(SigilKind::Agents) => self.load_roles(cx),
                _ => {}
            }
        }
        let open = token.filter(|_| !self.sigil.closed).and_then(|tk| {
            let query = tk.query();
            let items = match tk.kind {
                SigilKind::Files => return Some((file_items(&self.sigil.files, &query), tk)),
                // A command that runs on send only runs as the whole message, so later in a draft
                // it is not offered: picking it would have to replace what is written.
                SigilKind::Commands => commands(agent, provider)
                    .into_iter()
                    .filter(|item| tk.whole || runs_of(&item.name) != Some(Runs::Send))
                    .collect(),
                SigilKind::Skills => self
                    .sigil
                    .skills
                    .iter()
                    .map(|(name, description)| Item {
                        name: format!("${name}"),
                        description: description.clone().unwrap_or_else(|| "Skill".into()),
                    })
                    .collect(),
                SigilKind::Agents => match &self.roles {
                    Roles::Loaded(agents) => agents
                        .iter()
                        .map(|a| Item {
                            name: format!("&{}", a.name),
                            description: a.description.clone().unwrap_or_else(|| "Agent".into()),
                        })
                        .collect(),
                    Roles::Loading => Vec::new(),
                },
            };
            let items = scored(items, &query);
            // Later in a draft, a "/" that matches nothing is a path, so no menu shows for it.
            (tk.whole || !items.is_empty()).then_some((items, tk))
        });
        let has_items = open.as_ref().is_some_and(|(items, _)| !items.is_empty());
        self.editor.update(cx, |e, _| e.menu_keys = has_items);
        let Some((items, tk)) = open else {
            self.sigil.open = None;
            return None;
        };
        self.sigil.highlight = self.sigil.highlight.min(items.len().saturating_sub(1));
        let drawn = self.draw_sigil_menu(tk.kind, &items, &sigils, t, cx);
        self.sigil.open = Some((tk, items));
        Some(drawn)
    }

    /// The menu, as the web's slash and mention menus: a heading naming the list, then a row per
    /// item with its icon, its name in the code font and what it does, or for a file its folder.
    fn draw_sigil_menu(
        &self,
        kind: SigilKind,
        items: &[Item],
        sigils: &Sigils,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (label, empty, lead) = match kind {
            SigilKind::Files => ("Files", "No matching files", IconName::File),
            SigilKind::Commands => ("Commands", "No matching command.", IconName::Terminal),
            SigilKind::Skills => ("Skills", "No matching skill.", IconName::Sparkle),
            SigilKind::Agents => ("Agents", "No matching agent.", IconName::Robot),
        };
        let sigil = sigils.of(kind);
        let rows = items.iter().enumerate().map(|(i, item)| {
            let active = i == self.sigil.highlight;
            let hover = t.bg_hover;
            let files = kind == SigilKind::Files;
            let shown = format!("{sigil}{}", item.name.get(1..).unwrap_or_default());
            let row = button(
                SharedString::from(format!("sigil-item-{i}")),
                if files {
                    item.name.clone()
                } else {
                    shown.clone()
                },
            )
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .w_full()
            .flex_none()
            .h(px(if files { 28. } else { 32. }))
            .gap(px(8.))
            .px(px(8.))
            .rounded(px(4.))
            .text_size(px(fs_xs()))
            .text_color(t.text)
            .map(|b| {
                if active {
                    b.bg(t.bg_selected)
                } else {
                    b.hover(move |s| s.bg(hover))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| this.sigil_select(i, window, cx)));
            if files {
                // The name first and its folder after it, quieter, as the web's mention rows.
                let folder_at = item
                    .name
                    .trim_end_matches('/')
                    .rfind('/')
                    .map_or(0, |at| at + 1);
                let (folder, name) = item.name.split_at(folder_at);
                let is_folder = item.name.ends_with('/');
                row.font_family(font_mono())
                    .child(
                        icon(
                            if is_folder {
                                IconName::FolderSimple
                            } else {
                                IconName::File
                            },
                            12.,
                        )
                        .flex_none()
                        .text_color(t.text_muted),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(relative(0.6))
                            .truncate()
                            .child(name.to_owned()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(t.text_subtle)
                            .child(folder.to_owned()),
                    )
            } else {
                row.child(icon(lead, 13.).flex_none().text_color(t.text_muted))
                    .child(div().flex_none().font_family(font_mono()).child(shown))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(t.text_muted)
                            .child(item.description.clone()),
                    )
            }
        });
        let list = div()
            .id("sigil-menu")
            .role(Role::ListBox)
            .aria_label(label)
            .flex()
            .flex_col()
            .max_h(px(480.))
            .overflow_y_scroll()
            .track_scroll(&self.sigil.scroll)
            .when(items.is_empty(), |list| {
                list.child(
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .text_size(px(fs_xs()))
                        .text_color(t.text_muted)
                        .child(empty),
                )
            })
            .children(rows);
        // The heading sits outside the scrolled list, so the keys' row numbers stay the list's.
        let card = div()
            .occlude()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(8.))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .p(px(4.))
            .shadow(patchwork_design_components::suggestion_shadow(t))
            .font_weight(FontWeight::NORMAL)
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(fs_xs()))
                    .text_color(t.text_subtle)
                    .child(label),
            )
            .child(list);
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(relative(1.))
            .pb(px(6.))
            .child(deferred(card).with_priority(1))
            .into_any_element()
    }
}
