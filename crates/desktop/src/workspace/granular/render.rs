//! Drawing the card: a panel for the chat's agent and one for its fallback, each over the sub
//! agents it starts, and the buttons under them.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FontWeight, KeyDownEvent, MouseButton, Role, SharedString, Stateful, Toggled,
    Window, deferred, div, prelude::*, px,
};

use super::menus::below;
use super::state::{GranularDialog, GranularScope, Part, Slot, agent_of, pick_of, starter_of};
use crate::chat::composer::effort_label;
use crate::ui::dialog;
use crate::ui::theme::{FS_MD, FS_SM, RADIUS, Theme, theme};
use crate::ui::{BtnKind, agent_label, agent_logo, btn, button, icon, icon_button};
use crate::workspace::Workspace;
use patchwork_server::threads::delegates_sub_agents;

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
        .rounded(px(RADIUS))
        .border_1()
        .border_color(if open { t.accent } else { t.border_strong })
        .bg(t.bg_editor)
        .text_size(px(FS_MD))
        .text_color(if field.unset { t.text_muted } else { t.text })
        .hover(move |s| s.bg(hover))
        .children(field.mark)
        .child(div().flex_1().min_w_0().truncate().child(field.shown))
        .child(icon("caret-down", 12.).text_color(t.text_subtle))
}

impl Workspace {
    /// One side of the card: an agent with its model and effort, over those of the sub agents
    /// it starts. `main` is the slot of the agent and `sub` the slot of its sub agents.
    fn granular_panel(
        &self,
        dialog: &GranularDialog,
        (main, sub): (Slot, Slot),
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = &dialog.value;
        let composer = self.composer.read(cx);
        let dropdown = |slot: Slot, part: Part, label: String, field: Field| {
            let key = SharedString::from(format!("granular-{label}"));
            let menu = dialog.menu.as_ref().filter(|(k, _)| *k == key);
            let field = select(key.clone(), label, field, menu.is_some(), t).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.toggle_granular_menu(key.clone(), (slot, part), window, cx);
                },
            ));
            // The menu hangs from the select it belongs to.
            div()
                .relative()
                .min_w_0()
                .child(field)
                .children(menu.map(|(_, menu)| below(menu.clone())))
        };
        // A select of the sub agents under its title.
        let row = |title: &'static str, field: gpui::Div| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .w(px(52.))
                        .flex_none()
                        .text_size(px(FS_MD))
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
                composer
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
                .rounded(px(RADIUS))
                .bg(t.bg_hover)
                .text_size(px(FS_MD))
                .font_weight(FontWeight::MEDIUM)
                .child(mark)
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(icon("lock-simple", 14.).text_color(t.text_subtle))
                .into_any_element()
        } else {
            let field = Field {
                shown: name.into(),
                unset: agent.is_none(),
                mark: Some(mark),
            };
            dropdown(main, Part::Agent, main.name().to_string(), field)
                .font_weight(FontWeight::MEDIUM)
                .into_any_element()
        };
        let reset = icon_button(
            SharedString::from(format!("granular-reset-{}", main.name())),
            format!("Reset {}", main.name().to_lowercase()),
            "arrow-counter-clockwise",
            14.,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.reset_granular(main, cx)));
        // The agent the sub agents run on, where the one that starts them can hand work to another.
        let sub_agent = starter_of(value, sub)
            .filter(|starter| delegates_sub_agents(*starter))
            .map(|starter| {
                let pick = pick_of(value, sub);
                let on = pick.map_or(starter, |p| p.agent);
                let model = pick.and_then(|p| p.model.as_deref());
                let field = Field {
                    shown: agent_label(on).into(),
                    unset: on == starter,
                    mark: Some(agent_logo(on, model, 16.)),
                };
                let label = format!("{} agent", sub.name());
                row("Agent", dropdown(sub, Part::Agent, label, field))
            });
        let sub_rows: Vec<AnyElement> = sub_agent.into_iter().chain(rows(sub)).collect();
        let sub_agents = (!sub_rows.is_empty()).then(|| {
            div()
                .id(SharedString::from(format!("granular-{}", sub.name())))
                .role(Role::Group)
                .aria_label(sub.name())
                .flex()
                .flex_col()
                .gap(px(6.))
                .pt(px(10.))
                .border_t_1()
                .border_color(t.border)
                .child(
                    div()
                        .text_size(px(FS_SM))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.text_muted)
                        .child("Sub agents"),
                )
                .children(sub_rows)
        });
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
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(20.))
                    .text_size(px(FS_MD))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(main.name())
                    .child(reset),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(picker)
                    .children(rows(main)),
            )
            .children(sub_agents)
            .into_any_element()
    }

    pub(in crate::workspace) fn granular_dialog(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.granular_dialog.as_ref()?;
        let t = *theme(cx);
        let viewport = window.viewport_size();
        let panels = div()
            .flex()
            .items_start()
            .gap(px(12.))
            .child(self.granular_panel(dialog, (Slot::Main, Slot::Sub), &t, cx))
            .child(self.granular_panel(dialog, (Slot::Fallback, Slot::SubFallback), &t, cx));
        let project = matches!(dialog.scope, GranularScope::Project(_));
        let save = if dialog.first.is_some() {
            "Send"
        } else if project {
            "Apply to all chats"
        } else {
            "Save"
        };
        // A project's settings can be kept for its new chats alone.
        // Lit while the card shows what the project's new chats start with.
        let is_default = dialog.default.as_ref() == Some(&dialog.value);
        let make_default = project.then(|| {
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
            .aria_toggled(if is_default {
                Toggled::True
            } else {
                Toggled::False
            })
            .when(is_default, |b| {
                b.border_color(t.accent)
                    .bg(t.bg_accent_soft)
                    .text_color(t.accent_text)
                    .child(icon("check", 14.))
            })
            .child(shown)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_granular_default(cx)))
        });
        let buttons = div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .child(
                btn("granular-cancel", "Cancel", BtnKind::Plain, false, &t)
                    .child("Cancel")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_granular_dialog(false, window, cx);
                    })),
            )
            .children(make_default)
            .child(
                btn("granular-save", save, BtnKind::Primary, dialog.saving, &t)
                    .child(save)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_granular(window, cx);
                    })),
            );
        let card =
            dialog::card("granular-dialog", dialog.title.clone(), &dialog.focus, &t)
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.close_granular_dialog(false, window, cx);
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
                    icon_button("granular-close", "Close", "x", 16., cx).on_click(cx.listener(
                        |this, _, window, cx| this.close_granular_dialog(false, window, cx),
                    )),
                ))
                .child(panels)
                .children(dialog.error.clone().map(|error| {
                    div()
                        .id("granular-error")
                        .role(Role::Alert)
                        .text_size(px(FS_SM))
                        .text_color(t.danger)
                        .child(error)
                }))
                .child(buttons);
        Some(
            deferred(
                dialog::backdrop("granular-dialog-backdrop", true)
                    .items_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            this.close_granular_dialog(false, window, cx);
                        }),
                    )
                    .child(card),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}
