//! Opening the picker and its keys.

// Rust guideline compliant 2026-10-09

use super::rows::ModelPicker;
use super::rows::row_key;
use super::rows::read_favorites;
use gpui::{Focusable, KeyDownEvent, Window, prelude::*};
use patchwork_server::provider::AgentKind;
use crate::chat::composer::Composer;
use crate::ui::text_area::TextArea;

impl Composer {
    /// Escape closes the picker, and Ctrl+1 to Ctrl+9 pick from the top of the list. Up, Down
    /// and Enter come from the search field.
    pub(super) fn picker_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keys = &event.keystroke;
        if keys.key == "escape" {
            self.close_menu(window, cx);
        } else if keys.modifiers.control
            && let Some(n) = keys
                .key
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=9).contains(n))
        {
            if let Some(pick) = self.menu_picks.get(n - 1).cloned() {
                self.apply(pick, window, cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
    }

    /// Opens the picker on the favorites if one is still listed, else on the chat's agent; and on
    /// its Legacy models panel when the chat's model is a legacy one.
    pub(in crate::chat::composer) fn open_model_picker(
        &mut self,
        agents: &[AgentKind],
        current: (AgentKind, Option<String>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let favorites = read_favorites(&workspace.read(cx).server);
        let has_favorite = agents.iter().any(|&kind| {
            self.models_of(kind)
                .unwrap_or_default()
                .iter()
                .any(|m| favorites.contains(&row_key(kind, m)))
        });
        let (agent, model) = current;
        let active_legacy = self
            .models_of(agent)
            .unwrap_or_default()
            .iter()
            .find(|m| m.id == model)
            .is_some_and(|m| m.is_legacy && !favorites.contains(&row_key(agent, m)));
        let t = *crate::ui::theme::theme(cx);
        let search = cx.new(|cx| {
            let mut field = TextArea::new("Search models...", "Search models", cx);
            field.single_line = true;
            field.text_color = t.text;
            field.placeholder_color = t.text_subtle;
            field.selection_color = t.accent.opacity(0.3);
            field
        });
        cx.subscribe_in(
            &search,
            window,
            |this, _, _: &crate::ui::text_area::Changed, _, cx| {
                this.highlight = 0;
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &search,
            window,
            |this, _, arrow: &crate::ui::text_area::Arrow, _, cx| {
                let count = this.menu_picks.len();
                if count > 0 {
                    this.highlight =
                        (this.highlight as isize + arrow.0).rem_euclid(count as isize) as usize;
                }
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &search,
            window,
            |this, _, _: &crate::ui::text_area::Submitted, window, cx| {
                if let Some(pick) = this.menu_picks.get(this.highlight).cloned() {
                    this.apply(pick, window, cx);
                }
            },
        )
        .detach();
        let focus = search.read(cx).focus_handle(cx);
        self.picker = Some(ModelPicker {
            search,
            provider: if agents.len() > 1 && has_favorite {
                None
            } else {
                Some(agent)
            },
            legacy_open: active_legacy.then_some(agent),
            favorites,
        });
        // The chat's model starts highlighted.
        let rows = self.model_rows(agents, cx);
        self.highlight = rows
            .iter()
            .position(|r| r.kind == agent && r.item.id == model)
            .unwrap_or(0);
        window.focus(&focus, cx);
    }
}
