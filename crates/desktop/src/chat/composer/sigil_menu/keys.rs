//! The menu's keys, and what picking a row does.

// Rust guideline compliant 2026-10-09

use super::commands::{Runs, runs_of};
use std::ops::Range;
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
        if tk.kind != SigilKind::Files && tk.text.eq_ignore_ascii_case(&item.name) {
            return false;
        }
        let name = item.name.clone();
        // Later in a draft, a command that runs on pick runs, as a click does, and keeps the rest.
        if tk.kind == SigilKind::Commands && !tk.whole && runs_of(&name) == Some(Runs::Pick) {
            let token = tk.range.clone();
            self.run_local(&name, Some(token), window, cx);
            return true;
        }
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
        if tk.kind == SigilKind::Commands && runs_of(&name) == Some(Runs::Pick) {
            // Only the word typed goes; whatever else is written stays.
            let token = (!tk.whole).then(|| tk.range.clone());
            self.run_local(&name, token, window, cx);
            return;
        }
        self.complete(&name, window, cx);
    }

    /// Puts the pick in the draft. Later in the draft it replaces just the word typed; a pick
    /// for a draft that is only that word replaces the draft.
    fn complete(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tk, _)) = &self.sigil.open else {
            return;
        };
        let range = if tk.kind == SigilKind::Files || !tk.whole {
            tk.range.clone()
        } else {
            0..self.editor.read(cx).content.len()
        };
        let text = format!("{name} ");
        self.editor
            .update(cx, |e, cx| e.replace(range, &text, window, cx));
        self.focus(window, cx);
    }

    /// Runs a command Patchwork handles itself (`runLocalCommand`). With `token`, the command was
    /// picked later in a draft and only that word is taken out; without, the draft is cleared.
    pub(in crate::chat::composer) fn run_local(
        &mut self,
        name: &str,
        token: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        if name == "/clear" {
            return self.clear_chat(window, cx);
        }
        match token {
            Some(token) => {
                // The word goes with one space beside it, so no double space is left.
                let content = &self.editor.read(cx).content;
                let token = if content[token.end..].starts_with(' ') {
                    token.start..token.end + 1
                } else if content[..token.start].ends_with(' ') {
                    token.start - 1..token.end
                } else {
                    token
                };
                self.editor
                    .update(cx, |e, cx| e.replace(token, "", window, cx));
            }
            None => self.editor.update(cx, |e, cx| e.clear(window, cx)),
        }
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
