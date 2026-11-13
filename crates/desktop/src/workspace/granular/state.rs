//! What the card holds and what its buttons do: whose settings they are, the four agents it
//! sets, and opening, saving, resetting and closing it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{Entity, FocusHandle, Focusable, SharedString, Window, prelude::*};
use patchwork_server::provider::AgentKind;
use patchwork_server::threads::{
    AgentPick, Attachment, Granular, NewThread, Reserves, delegates_sub_agents, runs_sub_agents_at,
};

use crate::ui::menu::Menu;
use crate::workspace::Workspace;

/// Whose settings the card holds.
#[derive(Clone, PartialEq, Eq)]
pub enum GranularScope {
    Chat(String),
    Project(String),
}

/// One of the four agents the card sets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    Main,
    Fallback,
    Sub,
    SubFallback,
}

impl Slot {
    /// The slot's name, which is unique among the four for a screen reader.
    pub(super) fn name(self) -> &'static str {
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
pub(super) enum Part {
    Agent,
    Model,
    Effort,
}

/// The message a chat starts with, held until its card is answered: the text, its attachments,
/// and whether it waits in the chat's queue.
pub type First = (String, Vec<Attachment>, bool);

pub struct GranularDialog {
    pub(super) scope: GranularScope,
    pub(super) title: SharedString,
    pub(super) value: Granular,
    /// The chat's conversation has started, which belongs to one agent.
    pub(super) agent_fixed: bool,
    pub(super) focus: FocusHandle,
    /// The open dropdown: its chip's id, the menu, and where it was opened.
    pub(super) menu: Option<(SharedString, Entity<Menu>)>,
    pub(super) first: Option<First>,
    /// The project whose new chats the card can set what they start with: the card's own, or
    /// the project of a new chat when that project has nothing set yet.
    pub(super) default_for: Option<String>,
    /// What that project's new chats start with, as last saved. None when it has none.
    pub(super) default: Option<Granular>,
    pub(super) saving: bool,
    pub(super) error: Option<SharedString>,
}

/// The agent that starts a sub agent slot's sub agents: the chat's, or its fallback. None for the
/// other slots, and for the sub agents of a fallback not picked.
pub(super) fn starter_of(value: &Granular, slot: Slot) -> Option<AgentKind> {
    match slot {
        Slot::Main | Slot::Fallback => None,
        Slot::Sub => Some(value.main.agent),
        Slot::SubFallback => value.reserves.fallback.as_ref().map(|f| f.agent),
    }
}

/// The agent the slot's pick runs on. None for a fallback not picked, and for sub agents whose
/// model and effort cannot be set: those of an agent whose CLI cannot be told what to run its
/// own at.
pub(super) fn agent_of(value: &Granular, slot: Slot) -> Option<AgentKind> {
    match slot {
        Slot::Main => Some(value.main.agent),
        Slot::Fallback => value.reserves.fallback.as_ref().map(|f| f.agent),
        Slot::Sub | Slot::SubFallback => {
            let starter = starter_of(value, slot)?;
            let picked = pick_of(value, slot).map_or(starter, |p| p.agent);
            let set = if picked == starter {
                runs_sub_agents_at(starter)
            } else {
                delegates_sub_agents(starter)
            };
            set.then_some(picked)
        }
    }
}

pub(super) fn pick_of(value: &Granular, slot: Slot) -> Option<&AgentPick> {
    match slot {
        Slot::Main => Some(&value.main),
        Slot::Fallback => value.reserves.fallback.as_ref(),
        Slot::Sub => value.reserves.sub.as_ref(),
        Slot::SubFallback => value.reserves.sub_fallback.as_ref(),
    }
}

/// Changes the slot's model or effort. Sub agents on the agent that starts them, left at the
/// default for both, are not kept.
pub(super) fn edit(value: &mut Granular, slot: Slot, change: impl FnOnce(&mut AgentPick)) {
    let Some(agent) = agent_of(value, slot) else {
        return;
    };
    let own = starter_of(value, slot) == Some(agent);
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
    *held = (!own || pick != unset).then_some(pick);
}

/// Puts `pick` in a slot that names an agent, and drops the sub agents of the agent it replaces.
/// Sub agents put on the agent that starts them are that agent's own again.
pub(super) fn set_agent(value: &mut Granular, slot: Slot, pick: Option<AgentPick>) {
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
        (Slot::Sub | Slot::SubFallback, Some(pick)) => {
            let Some(starter) = starter_of(value, slot) else {
                return;
            };
            let held = if slot == Slot::Sub {
                &mut value.reserves.sub
            } else {
                &mut value.reserves.sub_fallback
            };
            if pick.agent != held.as_ref().map_or(starter, |p| p.agent) {
                *held = (pick.agent != starter).then_some(pick);
            }
        }
        _ => {}
    }
}

impl Workspace {
    /// The agent new chats start on, with its model from Settings.
    pub(super) fn default_pick(&self, agent: Option<AgentKind>) -> AgentPick {
        let agent = agent.unwrap_or_else(|| self.settings.default_agent());
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
        let (title, value, agent_fixed, default_for) = match &scope {
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
                // A new chat in a project with nothing set can set it.
                let default_for = thread
                    .project_id
                    .filter(|id| !started && self.server.project_granular(id).is_none());
                (
                    "Granular chat settings".to_string(),
                    value,
                    started,
                    default_for,
                )
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
                (
                    format!("Default {} settings", project.name),
                    value,
                    false,
                    Some(id.clone()),
                )
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
            default: default_for
                .as_deref()
                .and_then(|id| self.server.project_granular(id)),
            default_for,
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
    pub(super) fn save_granular(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn toggle_granular_default(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = &mut self.granular_dialog else {
            return;
        };
        let Some(id) = &dialog.default_for else {
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
    pub(super) fn close_granular_dialog(
        &mut self,
        saved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
    pub(super) fn reset_granular(&mut self, slot: Slot, cx: &mut Context<Self>) {
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
}
