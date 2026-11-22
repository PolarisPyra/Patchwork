//! Drawing the card: rows for the chat's agent, the agent that reviews its replies, and its
//! fallback, and the buttons under them.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FontWeight, KeyDownEvent, MouseButton, Role, SharedString, Stateful, Window, div,
    prelude::*, px,
};

use super::menus::below;
use super::card::{GranularDialog, GranularScope, Part, Slot, agent_of, pick_of};
use patchwork_ui::effort_label;
use patchwork_ui::controls::dialog;
use patchwork_ui::theme::{fs_md, fs_sm, radius, Theme, theme};
use patchwork_ui::{BtnKind, agent_label, agent_logo, btn, button, icon, icon_button};

/// The height of every select on the card.
const SELECT_HEIGHT: f32 = 32.;

/// What a select shows.
struct Field {
    shown: SharedString,
    /// Nothing is picked, so what shows is what the agent would use itself, drawn muted.
    unset: bool,
    mark: Option<AnyElement>,
}

impl Field {
    /// A model or an effort, which is the agent's default until one is picked.
    fn value(picked: Option<String>) -> Self {
        Self {
            unset: picked.is_none(),
            shown: picked.unwrap_or_else(|| "Default".into()).into(),
            mark: None,
        }
    }
}

/// A bordered dropdown named `label`, with its value at the left and a caret at the right.
fn select(
    id: SharedString,
    label: String,
    field: Field,
    open: bool,
    t: &Theme,
) -> Stateful<gpui::Div> {
    let hover = t.bg_hover;
    button(id, label)
        .aria_expanded(open)
        .w_full()
        .h(px(SELECT_HEIGHT))
        .gap(px(8.))
        .px(px(10.))
        .rounded(px(radius()))
        .border_1()
        .border_color(if open { t.accent } else { t.border_strong })
        .bg(t.bg_editor)
        .text_size(px(fs_md()))
        .text_color(if field.unset { t.text_muted } else { t.text })
        .hover(move |s| s.bg(hover))
        .children(field.mark)
        .child(div().flex_1().min_w_0().truncate().child(field.shown))
        .child(icon("caret-down", 12.).text_color(t.text_subtle))
}

impl GranularDialog {
    /// One panel of the card: an agent with its model and effort, over those of the sub agents
    /// it starts. `main` is the slot of the agent and `sub` the slot of its sub agents, if it
    /// starts any.
    fn panel(&self, main: Slot, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let dialog = self;
        let value = &dialog.value;
        let dropdown = |slot: Slot, part: Part, label: String, field: Field| {
            let key = SharedString::from(format!("granular-{label}"));
            let menu = dialog.menu.as_ref().filter(|(k, _)| *k == key);
            let field = select(key.clone(), label, field, menu.is_some(), t).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.toggle_menu(key.clone(), (slot, part), window, cx);
                },
            ));
            // The menu hangs from the select it belongs to.
            div()
                .relative()
                .min_w_0()
                .child(field)
                .children(menu.map(|(_, menu)| below(menu.clone())))
        };
        // A select under its title.
        let row = |title: &'static str, field: gpui::Div| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(52.))
                        .flex_none()
                        .text_size(px(fs_md()))
                        .text_color(t.text_muted)
                        .child(title),
                )
                .child(field.flex_1())
                .into_any_element()
        };
        // A slot's model and effort, each a named row. Nothing for a slot that cannot be set.
        let rows = |slot: Slot| -> Vec<AnyElement> {
            let Some(agent) = agent_of(value, slot) else {
                return Vec::new();
            };
            let pick = pick_of(value, slot);
            let model = pick.and_then(|p| p.model.as_deref()).map(|id| {
                dialog
                    .models_of(agent)
                    .and_then(|models| models.iter().find(|m| m.id.as_deref() == Some(id)))
                    .map_or_else(|| id.to_string(), |m| m.label.clone())
            });
            let effort = pick.and_then(|p| p.effort.as_deref()).map(effort_label);
            [
                (Part::Model, "Model", model),
                (Part::Effort, "Effort", effort),
            ]
            .into_iter()
            .map(|(part, title, shown)| {
                let label = format!("{} {}", slot.name(), title.to_lowercase());
                row(title, dropdown(slot, part, label, Field::value(shown)))
            })
            .collect()
        };
        let agent = agent_of(value, main);
        let model = pick_of(value, main).and_then(|p| p.model.as_deref());
        let mark = match agent {
            Some(agent) => agent_logo(agent, model, 16.),
            None => icon("prohibit", 16.).into_any_element(),
        };
        let name = agent.map_or("None", agent_label);
        let picker = if main == Slot::Main && dialog.agent_fixed {
            // A started conversation stays with its agent, so it is shown, not picked.
            div()
                .flex()
                .items_center()
                .h(px(SELECT_HEIGHT))
                .gap(px(8.))
                .px(px(10.))
                .rounded(px(radius()))
                .bg(t.bg_hover)
                .text_size(px(fs_md()))
                .font_weight(FontWeight::MEDIUM)
                .child(mark)
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(icon("lock-simple", 14.).text_color(t.text_subtle))
        } else {
            let field = Field {
                shown: name.into(),
                unset: agent.is_none(),
                mark: Some(mark),
            };
            dropdown(main, Part::Agent, main.name().to_string(), field)
        };
        let reset = icon_button(
            SharedString::from(format!("granular-reset-{}", main.name())),
            format!("Reset {}", main.name().to_lowercase()),
            "arrow-counter-clockwise",
            14.,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.reset(main, cx)));
        div()
            .id(SharedString::from(format!("granular-{}", main.name())))
            .role(Role::Group)
            .aria_label(main.name())
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(20.))
                    .text_size(px(fs_md()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(main.name())
                    .child(reset),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(row("Agent", picker))
                    .children(rows(main)),
            )
            .into_any_element()
    }
}

impl Render for GranularDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog = &*self;
        let t = *theme(cx);
        let viewport = window.viewport_size();
        // Settings → Chat can turn review off for every chat, and then there is nothing to pick.
        let reviewer = (!self.settings.review_off).then(|| dialog.panel(Slot::Review, &t, cx));
        // Stacked, the card can outgrow the window, so its panels scroll under the title.
        let panels = div()
            .id("granular-panels")
            .flex()
            .flex_col()
            .gap(px(12.))
            .max_h(viewport.height - px(180.))
            .overflow_y_scroll()
            .child(dialog.panel(Slot::Main, &t, cx))
            .children(reviewer)
            .child(dialog.panel(Slot::Fallback, &t, cx));
        let project = matches!(dialog.scope, GranularScope::Project(_));
        let save = if dialog.first.is_some() {
            "Send"
        } else if project {
            "Apply to all chats"
        } else {
            "Save"
        };
        // A project's settings, or a new chat's in a project with none, can be kept for the
        // project's new chats alone.
        // Lit while the card shows what the project's new chats start with.
        let is_default = dialog.default.as_ref() == Some(&dialog.value);
        let make_default = dialog.default_for.is_some().then(|| {
            let shown = if is_default {
                "Default"
            } else {
                "Make default"
            };
            btn(
                "granular-default",
                "Default for new chats",
                BtnKind::Plain,
                false,
                &t,
            )
            .aria_toggled(is_default.into())
            .when(is_default, |b| {
                b.border_color(t.accent)
                    .bg(t.bg_accent_soft)
                    .text_color(t.accent_text)
                    .child(icon("check", 14.))
            })
            .child(shown)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_default(cx)))
        });
        let buttons = div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .child(
                btn("granular-cancel", "Cancel", BtnKind::Plain, false, &t)
                    .child("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close(false, cx);
                    })),
            )
            .children(make_default)
            .child(
                btn("granular-save", save, BtnKind::Primary, dialog.saving, &t)
                    .child(save)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save(window, cx);
                    })),
            );
        let card = dialog::card("granular-dialog", dialog.title.clone(), &dialog.focus, &t)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close(false, cx);
                }
            }))
            .flex()
            .flex_col()
            .gap(px(14.))
            .w(px(640.))
            .max_w(viewport.width - px(48.))
            .p(px(16.))
            .child(dialog::title_row(
                dialog.title.clone(),
                icon_button("granular-close", "Close", "x", 16., cx)
                    .on_click(cx.listener(|this, _, _, cx| this.close(false, cx))),
            ))
            .child(panels)
            .children(dialog.error.clone().map(|error| {
                div()
                    .id("granular-error")
                    .role(Role::Alert)
                    .text_size(px(fs_sm()))
                    .text_color(t.danger)
                    .child(error)
            }))
            .child(buttons);
        dialog::backdrop("granular-dialog-backdrop", true)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close(false, cx)),
            )
            .child(card)
    }
}
