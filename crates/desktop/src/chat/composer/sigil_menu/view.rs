//! The menu drawn over the chat box.

// Rust guideline compliant 2026-10-09

use super::commands::scored;
use super::commands::Item;
use super::commands::commands;
use super::commands::file_items;
use super::commands::token;
use super::commands::{Runs, runs_of};
use gpui::{AnyElement, FontWeight, Role, SharedString, deferred, div, prelude::*, px, relative};
use patchwork_server::provider::{AgentKind, ProviderSlashCommand};
use crate::chat::composer::Composer;
use crate::chat::composer::menus::Roles;
use crate::ui::sigils::{SigilKind, Sigils};
use crate::ui::theme::{font_mono, fs_sm, radius, Theme};
use crate::ui::{button, icon, popup_shadow};

impl Composer {
    /// Works out the menu for the word before the caret, reading its list when it opens, and
    /// draws it when it shows.
    pub(in crate::chat::composer) fn sigil_menu(
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
            // Antigravity reads the message as a prompt, so there are no commands or skills to
            // offer, but an agent summoned with "&" goes with any message.
            .filter(|tk| {
                agent != AgentKind::Antigravity
                    || matches!(tk.kind, SigilKind::Files | SigilKind::Agents)
            });
        let kind = token.as_ref().map(|tk| tk.kind);
        let project = self
            .workspace
            .upgrade()
            .and_then(|w| w.read(cx).project().map(|p| p.id.clone()));
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

    fn draw_sigil_menu(
        &self,
        kind: SigilKind,
        items: &[Item],
        sigils: &Sigils,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (label, empty) = match kind {
            SigilKind::Files => ("Files", "No matching files"),
            SigilKind::Commands => ("Slash commands", "No matching command."),
            SigilKind::Skills => ("Skills", "No matching skill."),
            SigilKind::Agents => ("Agents", "No matching agent."),
        };
        let sigil = sigils.of(kind);
        let rows = items.iter().enumerate().map(|(i, item)| {
            let active = i == self.sigil.highlight;
            let hover = t.bg_hover;
            let shown = format!("{sigil}{}", item.name.get(1..).unwrap_or_default());
            let row = button(
                SharedString::from(format!("sigil-item-{i}")),
                if kind == SigilKind::Files {
                    item.name.clone()
                } else {
                    shown.clone()
                },
            )
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .w_full()
            .flex_none()
            .px(px(8.))
            .py(px(6.))
            .rounded(px(radius()))
            .text_color(t.text)
            .map(|b| {
                if active {
                    b.bg(t.bg_selected)
                } else {
                    b.hover(move |s| s.bg(hover))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| this.sigil_select(i, window, cx)));
            if kind == SigilKind::Files {
                row.font_family(font_mono())
                    .text_size(px(fs_sm()))
                    .when(item.name.ends_with('/'), |r| {
                        r.child(div().flex_none().mr(px(6.)).child(icon("folder", 13.)))
                    })
                    .child(div().min_w_0().truncate().child(item.name.clone()))
            } else {
                row.items_start()
                    .gap(px(10.))
                    .child(div().flex_none().font_family(font_mono()).child(shown))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(fs_sm()))
                            .text_color(t.text_muted)
                            .child(item.description.clone()),
                    )
            }
        });
        let list = div()
            .id("sigil-menu")
            .role(Role::ListBox)
            .aria_label(label)
            .occlude()
            .flex()
            .flex_col()
            .gap(px(1.))
            .max_h(px(512.))
            .overflow_y_scroll()
            .track_scroll(&self.sigil.scroll)
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .p(px(4.))
            .shadow(popup_shadow(t))
            .font_weight(FontWeight::NORMAL)
            .when(items.is_empty(), |list| {
                list.child(
                    div()
                        .px(px(8.))
                        .py(px(6.))
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(empty),
                )
            })
            .children(rows);
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(relative(1.))
            .pb(px(6.))
            .child(deferred(list).with_priority(1))
            .into_any_element()
    }
}
