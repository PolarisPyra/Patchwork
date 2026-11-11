//! Granular settings, in a card over the window: the model and effort of a chat's agent, of the
//! fallback that takes over when that agent is out of usage, and of the sub agents each starts.
//! A chat's are its own; a project's go to every chat in it and to each new one. Escape or a
//! press outside closes the card without saving.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AnyElement, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, MouseButton, Role,
    Toggled, SharedString, Stateful, WeakEntity, Window, anchored, deferred, div, point,
    prelude::*, px,
};
use patchwork_server::provider::{AgentKind, ModelOption};
use patchwork_server::threads::{
    AgentPick, Attachment, Granular, NewThread, Reserves, runs_sub_agents_at,
};

use super::Workspace;
use crate::chat::composer::{effort_label, efforts};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::{FS_LG, FS_MD, FS_SM, RADIUS, RADIUS_LG, Theme, theme};
use crate::ui::{
    AGENT_KINDS, BtnKind, agent_label, agent_logo, btn, button, icon, icon_button, popup_shadow,
};

/// Whose settings the card holds.
#[derive(Clone, PartialEq, Eq)]
pub enum GranularScope {
    Chat(String),
    Project(String),
}

/// One of the four agents the card sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Main,
    Fallback,
    Sub,
    SubFallback,
}

impl Slot {
    /// The slot's name, which is unique among the four for a screen reader.
    fn name(self) -> &'static str {
        match self {
            Slot::Main => "Agent",
            Slot::Fallback => "Fallback",
            Slot::Sub => "Sub agents",
            Slot::SubFallback => "Fallback sub agents",
        }
    }
}

/// What a chip of a slot picks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Agent,
    Model,
    Effort,
}

/// The message a chat starts with, held until its card is answered: the text, its attachments,
/// and whether it waits in the chat's queue.
pub type First = (String, Vec<Attachment>, bool);

pub struct GranularDialog {
    scope: GranularScope,
    title: SharedString,
    value: Granular,
    /// The chat's conversation has started, which belongs to one agent.
    agent_fixed: bool,
    focus: FocusHandle,
    /// The open dropdown: its chip's id, the menu, and where it was opened.
    menu: Option<(SharedString, Entity<Menu>)>,
    first: Option<First>,
    /// What a project's new chats start with, as last saved. None for a chat, and for a
    /// project with none.
    default: Option<Granular>,
    saving: bool,
    error: Option<SharedString>,
}

/// The agent the slot's pick runs on. None for a fallback not picked, and for the sub agents of
/// an agent whose CLI cannot be told what to run them at.
fn agent_of(value: &Granular, slot: Slot) -> Option<AgentKind> {
    let fallback = value.reserves.fallback.as_ref().map(|f| f.agent);
    match slot {
        Slot::Main => Some(value.main.agent),
        Slot::Fallback => fallback,
        Slot::Sub => Some(value.main.agent).filter(|a| runs_sub_agents_at(*a)),
        Slot::SubFallback => fallback.filter(|a| runs_sub_agents_at(*a)),
    }
}

fn pick_of(value: &Granular, slot: Slot) -> Option<&AgentPick> {
    match slot {
        Slot::Main => Some(&value.main),
        Slot::Fallback => value.reserves.fallback.as_ref(),
        Slot::Sub => value.reserves.sub.as_ref(),
        Slot::SubFallback => value.reserves.sub_fallback.as_ref(),
    }
}

/// Changes the slot's model or effort. Sub agents left at the default for both are not kept.
fn edit(value: &mut Granular, slot: Slot, change: impl FnOnce(&mut AgentPick)) {
    let Some(agent) = agent_of(value, slot) else {
        return;
    };
    let held = match slot {
        Slot::Main => return change(&mut value.main),
        Slot::Fallback => {
            if let Some(fallback) = &mut value.reserves.fallback {
                change(fallback);
            }
            return;
        }
        Slot::Sub => &mut value.reserves.sub,
        Slot::SubFallback => &mut value.reserves.sub_fallback,
    };
    let unset = AgentPick {
        agent,
        model: None,
        effort: None,
    };
    let mut pick = held
        .take()
        .filter(|p| p.agent == agent)
        .unwrap_or_else(|| unset.clone());
    change(&mut pick);
    *held = (pick != unset).then_some(pick);
}

/// Puts `pick` in a slot that names an agent, and drops the sub agents of the agent it replaces.
fn set_agent(value: &mut Granular, slot: Slot, pick: Option<AgentPick>) {
    match (slot, pick) {
        (Slot::Main, Some(pick)) if pick.agent != value.main.agent => {
            value.main = pick;
            value.reserves.sub = None;
        }
        (Slot::Fallback, pick)
            if pick.as_ref().map(|p| p.agent)
                != value.reserves.fallback.as_ref().map(|f| f.agent) =>
        {
            value.reserves.fallback = pick;
            value.reserves.sub_fallback = None;
        }
        _ => {}
    }
}

/// A menu row that makes `change` to the open card's settings.
fn choice(
    workspace: &WeakEntity<Workspace>,
    label: impl Into<SharedString>,
    checked: bool,
    change: impl Fn(&mut Granular) + 'static,
) -> MenuItem {
    let workspace = workspace.clone();
    MenuItem::new(label, move |_, cx| {
        let _ = workspace.update(cx, |w, cx| {
            if let Some(dialog) = &mut w.granular_dialog {
                change(&mut dialog.value);
                dialog.error = None;
                cx.notify();
            }
        });
    })
    .radio(checked)
}

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

/// Lays `menu` over the window with its top-left corner 4px under the bottom-left of the element
/// this is a child of, which must be `relative`: a menu that drops from its select.
fn below(menu: Entity<Menu>) -> gpui::Div {
    div().absolute().bottom_0().left_0().child(
        deferred(
            anchored()
                .offset(point(px(0.), px(4.)))
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(3),
    )
}

impl Workspace {
    /// The agent new chats start on, with its model from Settings.
    fn default_pick(&self, agent: Option<AgentKind>) -> AgentPick {
        let first = self.settings.agents.first().copied();
        let agent = agent.or(first).unwrap_or(AgentKind::Claude);
        AgentPick {
            agent,
            model: self.settings.tokens.default_models.get(&agent).cloned(),
            effort: None,
        }
    }

    /// Opens the granular settings of a chat or a project. `first` is the message a new chat
    /// starts with, sent once the card is saved.
    pub fn open_granular_dialog(
        &mut self,
        scope: GranularScope,
        first: Option<First>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let default = match &scope {
            GranularScope::Chat(_) => None,
            GranularScope::Project(id) => self.server.project_granular(id),
        };
        let (title, value, agent_fixed) = match &scope {
            GranularScope::Chat(id) => {
                let (Ok(thread), Ok(value)) =
                    (self.server.thread(id), self.server.thread_granular(id))
                else {
                    return;
                };
                // A subagent's chat runs with what its parent gave it.
                if thread.parent.is_some() {
                    return;
                }
                let started = !thread.messages.is_empty() && !thread.context_pending;
                ("Granular chat settings".to_string(), value, started)
            }
            GranularScope::Project(id) => {
                let Some(project) = self.settings.projects.iter().find(|p| &p.id == id) else {
                    return;
                };
                let value = self
                    .server
                    .project_granular(id)
                    .unwrap_or_else(|| Granular {
                        main: self.default_pick(None),
                        reserves: Reserves::default(),
                    });
                (format!("Default {} settings", project.name), value, false)
            }
        };
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.granular_dialog = Some(GranularDialog {
            scope,
            title: title.into(),
            value,
            agent_fixed,
            focus,
            menu: None,
            first,
            default,
            saving: false,
            error: None,
        });
        cx.notify();
    }

    /// Starts the chat `new` and opens its granular settings, holding `first` until they are
    /// saved.
    pub fn start_granular_chat(
        &mut self,
        new: NewThread,
        role: Option<String>,
        first: First,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let thread = match self.server.create_thread(new) {
            Ok(thread) => thread,
            Err(e) => {
                log::warn!(error:% = e; "could not start a chat: {{error}}");
                self.composer
                    .update(cx, |c, cx| c.put_back(&first.0, first.1, cx));
                return;
            }
        };
        let id = thread.id.clone();
        if role.is_some()
            && let Err(e) = self.server.set_thread_role(&id, role)
        {
            log::warn!(error:% = e; "could not set the chat's agent: {{error}}");
        }
        self.thread = Some(thread);
        self.reload(cx);
        self.open_granular_dialog(GranularScope::Chat(id), Some(first), window, cx);
    }

    /// Saves the card's settings off the UI thread, since a project's go to each of its chats,
    /// and closes it once they are saved.
    fn save_granular(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.granular_dialog.as_mut().filter(|d| !d.saving) else {
            return;
        };
        dialog.saving = true;
        dialog.menu = None;
        let (server, scope, value) = (
            Arc::clone(&self.server),
            dialog.scope.clone(),
            dialog.value.clone(),
        );
        let task = cx.background_spawn(async move {
            match &scope {
                GranularScope::Chat(id) => server.set_thread_granular(id, value),
                GranularScope::Project(id) => server.set_project_granular(id, value),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let saved = task.await;
            let _ = this.update_in(cx, |this, window, cx| match saved {
                Ok(()) => this.close_granular_dialog(true, window, cx),
                Err(e) => {
                    if let Some(dialog) = &mut this.granular_dialog {
                        dialog.saving = false;
                        dialog.error = Some(e.to_string().into());
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Makes the card's settings what the project's new chats start with, or stops them being
    /// that when they already are. The chats the project has are left as they are, and the
    /// card stays open to show which it is.
    fn toggle_granular_default(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.granular_dialog else {
            return;
        };
        let GranularScope::Project(id) = &dialog.scope else {
            return;
        };
        let changed = if dialog.default.as_ref() == Some(&dialog.value) {
            self.server.clear_project_granular_default(id)
        } else {
            self.server
                .set_project_granular_default(id, dialog.value.clone())
        };
        match changed {
            Ok(()) => {
                dialog.default = self.server.project_granular(id);
                // As saved, without the picks that could never act.
                if let Some(saved) = &dialog.default {
                    dialog.value = saved.clone();
                }
                dialog.error = None;
            }
            Err(e) => dialog.error = Some(e.to_string().into()),
        }
        cx.notify();
    }

    /// Closes the card. A held first message is sent when the settings were saved, and goes
    /// back to the chat box when they were not.
    fn close_granular_dialog(&mut self, saved: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.granular_dialog.take() else {
            return;
        };
        self.reload(cx);
        let open = self.thread.as_ref().map(|t| t.id.as_str());
        if matches!(&dialog.scope, GranularScope::Chat(id) if open == Some(id.as_str())) {
            let editor = self.composer.read(cx).editor.clone();
            window.focus(&editor.read(cx).focus_handle(cx), cx);
        } else {
            window.focus(&self.focus, cx);
        }
        if let Some((text, attachments, queue)) = dialog.first {
            if saved {
                self.send_with(text, attachments, None, None, queue, cx);
            } else {
                self.composer
                    .update(cx, |c, cx| c.put_back(&text, attachments, cx));
            }
        }
        cx.notify();
    }

    /// Puts a slot back: the agent to the one new chats start on, and the rest to none.
    fn reset_granular(&mut self, slot: Slot, cx: &mut Context<Self>) {
        let Some(dialog) = &self.granular_dialog else {
            return;
        };
        let kept = dialog.agent_fixed.then_some(dialog.value.main.agent);
        let main = self.default_pick(kept);
        let Some(dialog) = &mut self.granular_dialog else {
            return;
        };
        let value = &mut dialog.value;
        match slot {
            Slot::Main if main.agent == value.main.agent => value.main = main,
            Slot::Main => set_agent(value, slot, Some(main)),
            Slot::Fallback => set_agent(value, slot, None),
            Slot::Sub => value.reserves.sub = None,
            Slot::SubFallback => value.reserves.sub_fallback = None,
        }
        dialog.error = None;
        cx.notify();
    }

    /// The rows of the dropdown for one part of a slot.
    fn granular_items(&self, slot: Slot, part: Part, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let Some(dialog) = &self.granular_dialog else {
            return Vec::new();
        };
        let value = &dialog.value;
        let this = cx.entity().downgrade();
        let pick = pick_of(value, slot);
        if part == Part::Agent {
            let current = pick.map(|p| p.agent);
            let none = (slot == Slot::Fallback).then(|| {
                choice(&this, "None", current.is_none(), move |v| {
                    set_agent(v, slot, None);
                })
            });
            let agents = AGENT_KINDS
                .into_iter()
                .filter(|kind| self.settings.agents.contains(kind))
                .map(|kind| {
                    let to = self.default_pick(Some(kind));
                    choice(&this, agent_label(kind), current == Some(kind), move |v| {
                        set_agent(v, slot, Some(to.clone()));
                    })
                });
            return none.into_iter().chain(agents).collect();
        }
        let Some(agent) = agent_of(value, slot) else {
            return Vec::new();
        };
        let composer = self.composer.read(cx);
        let models = composer.models_of(agent);
        let model = pick.and_then(|p| p.model.clone());
        if part == Part::Effort {
            let current = pick.and_then(|p| p.effort.clone());
            let default = choice(&this, "Default", current.is_none(), move |v| {
                edit(v, slot, |p| p.effort = None);
            });
            let levels = efforts(agent, model.as_deref(), models)
                .into_iter()
                .map(|level| {
                    let checked = current.as_deref() == Some(level.as_str());
                    choice(&this, effort_label(&level), checked, move |v| {
                        edit(v, slot, |p| p.effort = Some(level.clone()));
                    })
                });
            return std::iter::once(default).chain(levels).collect();
        }
        // A model keeps the effort only when it takes that level.
        let to_model = |id: Option<String>| {
            let takes = efforts(agent, id.as_deref(), models);
            move |v: &mut Granular| {
                edit(v, slot, |p| {
                    p.model.clone_from(&id);
                    p.effort = p.effort.take().filter(|e| takes.contains(e));
                });
            }
        };
        let default = choice(&this, "Default", model.is_none(), to_model(None));
        let listed = models
            .unwrap_or_default()
            .iter()
            .filter(|m| !m.is_legacy || m.id == model)
            .filter_map(|m: &ModelOption| {
                let id = m.id.clone()?;
                let checked = model.as_deref() == Some(id.as_str());
                Some(choice(&this, m.label.clone(), checked, to_model(Some(id))))
            });
        std::iter::once(default).chain(listed).collect()
    }

    /// Opens the dropdown of the chip `key`, or closes it when it is the one open.
    fn toggle_granular_menu(
        &mut self,
        key: SharedString,
        (slot, part): (Slot, Part),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.granular_dialog.as_ref().and_then(|d| d.menu.as_ref());
        if open.is_some_and(|(k, _)| *k == key) {
            if let Some(dialog) = &mut self.granular_dialog {
                dialog.menu = None;
            }
            return cx.notify();
        }
        let items = self.granular_items(slot, part, cx);
        let label = match part {
            Part::Agent => slot.name().to_string(),
            Part::Model => format!("{} model", slot.name()),
            Part::Effort => format!("{} effort", slot.name()),
        };
        let menu = Menu::open(label, items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, _: &Dismissed, window, cx| {
            if let Some(dialog) = &mut this.granular_dialog {
                dialog.menu = None;
                window.focus(&dialog.focus, cx);
            }
            cx.notify();
        })
        .detach();
        if let Some(dialog) = &mut self.granular_dialog {
            dialog.menu = Some((key, menu));
        }
        cx.notify();
    }

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
                    .child(dropdown(slot, part, label, Field::value(shown)).flex_1())
                    .into_any_element()
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
        let sub_rows = rows(sub);
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

    pub(super) fn granular_dialog(
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
        let card = div()
            .id("granular-dialog")
            .role(Role::Dialog)
            .aria_label(dialog.title.clone())
            .track_focus(&dialog.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_granular_dialog(false, window, cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .gap(px(14.))
            .w(px(640.))
            .max_w(viewport.width - px(48.))
            .p(px(16.))
            .rounded(px(RADIUS_LG))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .shadow(popup_shadow(&t))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(FS_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(dialog.title.clone()),
                    )
                    .child(
                        icon_button("granular-close", "Close", "x", 16., cx).on_click(cx.listener(
                            |this, _, window, cx| this.close_granular_dialog(false, window, cx),
                        )),
                    ),
            )
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
                div()
                    .id("granular-dialog-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .occlude()
                    .bg(gpui::hsla(0., 0., 0., 0.4))
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
