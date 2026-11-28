//! The menu drawn over the chat box while a `/` command, `$` skill, `@` file or `&` agent is
//! typed, as the web composer's slash and mention menus.

// Rust guideline compliant 2026-10-09

use super::commands::scored;
use super::commands::Item;
use super::commands::commands;
use super::commands::file_items;
use super::commands::FILE_LIMIT;
use super::commands::token;
use super::commands::{Runs, Token, runs_of};
use super::open_menu::Mentions;
use gpui::{
    AnyElement, FontWeight, Role, SharedString, WeakEntity, Window, deferred, div, prelude::*, px,
    relative,
};
use patchwork_model::provider::{AgentKind, ProviderSlashCommand};
use crate::composer::Composer;
use crate::composer::menus::Roles;
use crate::host::Mention;
use patchwork_views_chat::{sigils::SigilKind, sigils::Sigils};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, font_mono, fs_xs};
use patchwork_design_components::button;
use patchwork_design_components::menu::{Menu, MenuItem};
use patchwork_design_icons::icon;

impl Composer {
    /// Works out the menu for the word before the caret, reading its list when it opens, and
    /// draws it when it shows.
    pub(in crate::composer) fn sigil_menu(
        &mut self,
        agent: AgentKind,
        provider: &[ProviderSlashCommand],
        t: &Theme,
        window: &mut Window,
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
        // The host may list its own places for `@`, as the orchestrator page lists projects and
        // chats.
        let mentions = self
            .host
            .mentions(cx)
            .filter(|_| kind == Some(SigilKind::Files));
        let typed = kind.map(|kind| (kind, project));
        if typed != self.sigil.kind {
            self.sigil.kind = typed;
            match kind {
                Some(SigilKind::Files) if mentions.is_none() => self.load_files(cx),
                Some(SigilKind::Skills) => self.load_skills(cx),
                Some(SigilKind::Agents) => self.load_roles(cx),
                _ => {}
            }
        }
        if let Some(mentions) = mentions {
            let token = token.filter(|_| !self.sigil.closed);
            return self.mention_menu(&mentions, token, window, cx);
        }
        self.sigil.mentions = None;
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

impl Composer {
    /// The app's menu over the chat box, listing the host's places for the `@` word `token`:
    /// each with its own places in a menu beside it, or, once a name is typed, every place with
    /// it in its name or where it is.
    fn mention_menu(
        &mut self,
        mentions: &[Mention],
        token: Option<Token>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let listed = token.map(|tk| {
            let query = tk.query();
            let items = mention_items(mentions, &query, &cx.weak_entity());
            (tk, query, items)
        });
        let Some((tk, query, (ids, items))) = listed.filter(|(_, _, (ids, _))| !ids.is_empty())
        else {
            self.sigil.open = None;
            self.sigil.mentions = None;
            self.editor.update(cx, |e, _| e.menu_keys = false);
            return None;
        };
        self.editor.update(cx, |e, _| e.menu_keys = true);
        self.sigil.open = Some((tk, Vec::new()));
        let made_for = (query, ids);
        if self.sigil.mentions.as_ref().map(|m| &m.made_for) != Some(&made_for) {
            let menu = Menu::attached("Projects and chats", items, window, cx);
            self.sigil.mentions = Some(Mentions::new(made_for, menu, cx));
        }
        let menu = self.sigil.mentions.as_ref()?.menu.clone();
        Some(
            div()
                .absolute()
                .left_0()
                .bottom(relative(1.))
                .pb(px(6.))
                .child(deferred(menu).with_priority(1))
                .into_any_element(),
        )
    }
}

/// The rows for the host's `mentions`, and the ids of the places they pick: each place with its
/// own in a menu beside it, or, for a typed `query`, every place whose name or whereabouts hold
/// it, with where it is after its name.
fn mention_items(
    mentions: &[Mention],
    query: &str,
    composer: &WeakEntity<Composer>,
) -> (Vec<String>, Vec<MenuItem>) {
    let mut ids = Vec::new();
    let mut pick = |m: &Mention| {
        ids.push(m.id.clone());
        let (id, composer) = (m.id.clone(), composer.clone());
        MenuItem::new(m.label.clone(), move |window, cx| {
            let (id, composer) = (id.clone(), composer.clone());
            // The menu runs its row inside the composer's own key handling, so the pick waits
            // until that is done.
            window.defer(cx, move |window, cx| {
                let _ = composer.update(cx, |this, cx| this.pick_mention(&id, window, cx));
            });
        })
        .icon(m.icon)
    };
    let items = if query.is_empty() {
        fn tree(m: &Mention, pick: &mut dyn FnMut(&Mention) -> MenuItem) -> MenuItem {
            let item = if m.children.is_empty() {
                pick(m)
            } else {
                let rows = m.children.iter().map(|c| tree(c, pick)).collect();
                MenuItem::submenu(m.label.clone(), rows).icon(m.icon)
            };
            if m.separated { item.separated() } else { item }
        }
        mentions.iter().map(|m| tree(m, &mut pick)).collect()
    } else {
        mentions
            .iter()
            .flat_map(|m| {
                if m.children.is_empty() {
                    std::slice::from_ref(m)
                } else {
                    m.children.as_slice()
                }
            })
            .filter(|m| {
                m.label.to_lowercase().contains(query) || m.detail.to_lowercase().contains(query)
            })
            .take(FILE_LIMIT)
            .map(|m| pick(m).detail(m.detail.clone()))
            .collect()
    };
    (ids, items)
}
