//! The menu's keys, and what picking a row does.

// Rust guideline compliant 2026-10-09

use super::commands::local_command;
use super::commands::Runs;
use super::commands::SLASH_COMMANDS;
use std::sync::Arc;
use gpui::{PromptLevel, Window, prelude::*};
use patchwork_server::provider::AgentKind;
use crate::chat::composer::Composer;
use crate::chat::composer::menus::Menu;
use crate::ui::sigils::{SigilKind, Sigils};

impl Composer {
    /// Up or Down while the menu has the keys.
    pub(in crate::chat::composer) fn sigil_arrow(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.sigil.open.as_ref().map_or(0, |(_, items)| items.len());
        if count > 0 {
            let next = (self.sigil.highlight as isize + step).rem_euclid(count as isize) as usize;
            self.sigil.highlight = next;
            self.sigil.scroll.scroll_to_item(next);
            cx.notify();
        }
    }

    /// Tab: a folder opens, listing what is in it; anything else is picked.
    pub(in crate::chat::composer) fn sigil_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((tk, items)) = &self.sigil.open else {
            return;
        };
        let Some(item) = items.get(self.sigil.highlight) else {
            return;
        };
        if tk.kind == SigilKind::Files && item.name.ends_with('/') {
            let sigils = cx.try_global::<Sigils>().cloned().unwrap_or_default();
            let text = format!("{}{}", sigils.of(SigilKind::Files), item.name);
            let range = tk.range.clone();
            self.editor
                .update(cx, |e, cx| e.replace(range, &text, window, cx));
            return;
        }
        let name = item.name.clone();
        self.complete(&name, window, cx);
    }

    /// Enter picks the highlighted entry, unless nothing is listed or the command is typed in
    /// full, which sends it. True when it picked.
    pub(in crate::chat::composer) fn sigil_enter(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((tk, items)) = &self.sigil.open else {
            return false;
        };
        let Some(item) = items.get(self.sigil.highlight) else {
            return false;
        };
        if tk.kind != SigilKind::Files && tk.text.to_lowercase() == item.name {
            return false;
        }
        let name = item.name.clone();
        self.complete(&name, window, cx);
        true
    }

    /// Escape hides the open menu until the draft changes. True when one was open.
    pub(in crate::chat::composer) fn sigil_escape(&mut self, cx: &mut Context<Self>) -> bool {
        if self.sigil.open.is_none() {
            return false;
        }
        self.sigil.closed = true;
        cx.notify();
        true
    }

    /// A row was clicked: a command that runs on pick runs at once, and anything else goes in.
    pub(super) fn sigil_select(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tk, items)) = &self.sigil.open else {
            return;
        };
        let Some(item) = items.get(i) else {
            return;
        };
        let name = item.name.clone();
        let picked = SLASH_COMMANDS
            .iter()
            .any(|&(n, _, runs)| n == name && runs == Runs::Pick);
        let local = local_command(&name).is_some();
        if tk.kind == SigilKind::Commands && (tk.whole || local) && picked {
            self.run_local(&name, window, cx);
            return;
        }
        self.complete(&name, window, cx);
    }

    /// Puts the pick in the draft. Later in the draft it replaces just the word typed; a pick
    /// for a draft that is only that word, or a local command, which only runs as the whole
    /// message, replaces the draft.
    fn complete(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tk, _)) = &self.sigil.open else {
            return;
        };
        let range = if tk.kind == SigilKind::Files || (!tk.whole && local_command(name).is_none()) {
            tk.range.clone()
        } else {
            0..self.editor.read(cx).content.len()
        };
        let text = format!("{name} ");
        self.editor
            .update(cx, |e, cx| e.replace(range, &text, window, cx));
        self.focus(window, cx);
    }

    /// Runs a command Patchwork handles itself (`runLocalCommand`), clearing the draft.
    pub(in crate::chat::composer) fn run_local(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if name == "/clear" {
            return self.clear_chat(window, cx);
        }
        self.editor.update(cx, |e, cx| e.clear(window, cx));
        match name {
            "/compact" => workspace.update(cx, |w, cx| w.send_command("/compact", cx)),
            "/model" => self.open_menu(Menu::Model, window, cx),
            "/plan" | "/default" => {
                let on = name == "/plan";
                let picks = self.picks(workspace.read(cx));
                let thread = workspace.read(cx).thread.as_ref().map(|t| t.id.clone());
                if picks.agent == AgentKind::Antigravity {
                    workspace.update(cx, |w, cx| {
                        w.error = Some("Plan mode is not supported by Antigravity.".into());
                        cx.notify();
                    });
                } else if let Some(id) = thread {
                    workspace.update(cx, |w, cx| {
                        w.change(
                            "change plan mode",
                            move |server| server.set_thread_plan_mode(&id, on),
                            cx,
                        );
                    });
                } else {
                    self.draft.plan_mode = on;
                }
            }
            // /new starts a fresh chat; the current one is kept in the sidebar.
            "/new" => workspace.update(cx, crate::workspace::Workspace::new_chat),
            _ => {}
        }
        cx.notify();
    }

    /// /clear asks first: the messages are deleted and cannot be recovered.
    fn clear_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let chat = workspace.read(cx).thread.as_ref().map(|thread| {
            let detail = format!(
                "Clear {}? Its messages are deleted and the agent forgets this conversation.",
                crate::ui::display_title(&thread.title)
            );
            (thread.id.clone(), detail)
        });
        let Some((id, detail)) = chat else {
            self.editor.update(cx, |e, cx| e.clear(window, cx));
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "Clear chat",
            Some(&detail),
            &["Clear", "Cancel"],
            cx,
        );
        let server = Arc::clone(&workspace.read(cx).server);
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let cleared = cx
                .background_spawn(async move { server.clear_thread(&id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.editor.update(cx, |e, cx| e.clear(window, cx));
                this.attachments.clear();
                let _ = this.workspace.update(cx, |w, cx| {
                    if let Err(e) = cleared {
                        w.error = Some(e.to_string());
                    }
                    w.reload(cx);
                });
            });
        })
        .detach();
    }
}
