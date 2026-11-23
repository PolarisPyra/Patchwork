//! What the card holds and what its buttons do: whose settings they are, the orchestrator
//! and executor it sets, and opening, saving, resetting and closing it.

// Rust guideline compliant 2026-11-23

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, Entity, EventEmitter, FocusHandle, SharedString, Window, prelude::*};
use patchwork_core::Core;
use patchwork_core::chats::{AgentPick, Attachment, Granular, Reserves};
use patchwork_core::provider::{AgentKind, ModelOption};
use patchwork_core::settings::Settings;

use patchwork_ui::controls::menu::Menu;

/// Whose settings the card holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GranularScope {
    Chat(String),
    Project(String),
}

/// One of the two agents the card sets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    /// The chat's own agent, which plans, decides and reviews.
    Main,
    /// The agent its sub agents run on.
    Executor,
}

impl Slot {
    /// The slot's name, which is unique between the two for a screen reader.
    pub(super) fn name(self) -> &'static str {
        match self {
            Slot::Main => "Orchestrator",
            Slot::Executor => "Executor",
        }
    }

    /// What the slot's agent does.
    pub(super) fn description(self) -> &'static str {
        match self {
            Slot::Main => {
                "Plans the work, makes design decisions, and reviews the result. Pick a top-tier \
                 model. You can also pick one per conversation from the Orchestrator row in the \
                 model menu."
            }
            Slot::Executor => {
                "Carries out the plan: searches, edits, builds, and tests. Pick a faster, cheaper \
                 model."
            }
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

/// What the card tells the window.
#[derive(Debug, Clone)]
pub enum GranularEvent {
    /// The card closed, `saved` or not, handing back the message its chat starts with.
    Closed {
        scope: GranularScope,
        first: Option<First>,
        saved: bool,
    },
}

/// Granular settings of a chat or a project, in a card over the window.
pub struct GranularDialog {
    pub(super) core: Arc<Core>,
    pub(super) settings: Settings,
    /// The models each agent offers, as the chat box last listed them.
    pub(super) models: HashMap<AgentKind, Vec<ModelOption>>,
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

impl std::fmt::Debug for GranularDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GranularDialog")
            .field("scope", &self.scope)
            .field("saving", &self.saving)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<GranularEvent> for GranularDialog {}

/// The agent the slot's pick runs on. None for an executor not picked.
pub(super) fn agent_of(value: &Granular, slot: Slot) -> Option<AgentKind> {
    pick_of(value, slot).map(|p| p.agent)
}

pub(super) fn pick_of(value: &Granular, slot: Slot) -> Option<&AgentPick> {
    match slot {
        Slot::Main => Some(&value.main),
        Slot::Executor => value.reserves.executor.as_ref(),
    }
}

/// Changes the slot's model or effort.
pub(super) fn edit(value: &mut Granular, slot: Slot, change: impl FnOnce(&mut AgentPick)) {
    let held = match slot {
        Slot::Main => Some(&mut value.main),
        Slot::Executor => value.reserves.executor.as_mut(),
    };
    if let Some(pick) = held {
        change(pick);
    }
}

/// Puts `pick` in a slot that names an agent, when it names another agent than the slot's.
pub(super) fn set_agent(value: &mut Granular, slot: Slot, pick: Option<AgentPick>) {
    match (slot, pick) {
        (Slot::Main, Some(pick)) if pick.agent != value.main.agent => value.main = pick,
        (Slot::Executor, pick)
            if pick.as_ref().map(|p| p.agent)
                != value.reserves.executor.as_ref().map(|e| e.agent) =>
        {
            value.reserves.executor = pick;
        }
        _ => {}
    }
}

/// The agent new chats start on, with its model from Settings.
fn default_pick(settings: &Settings, agent: Option<AgentKind>) -> AgentPick {
    let agent = agent.unwrap_or_else(|| settings.default_agent());
    AgentPick {
        agent,
        model: settings.tokens.default_models.get(&agent).cloned(),
        effort: None,
    }
}

impl GranularDialog {
    /// Opens the granular settings of a chat or a project, focused. `first` is the message a new
    /// chat starts with, sent once the card is saved. None for a subagent's chat, which runs with
    /// what its parent gave it, or for a chat or project that is gone.
    #[must_use]
    pub fn open(
        core: Arc<Core>,
        settings: Settings,
        models: HashMap<AgentKind, Vec<ModelOption>>,
        scope: GranularScope,
        first: Option<First>,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Entity<Self>> {
        let (title, value, agent_fixed, default_for) = match &scope {
            GranularScope::Chat(id) => {
                let (Ok(chat), Ok(value)) = (core.chat(id), core.chat_granular(id)) else {
                    return None;
                };
                if chat.parent.is_some() {
                    return None;
                }
                let started = !chat.messages.is_empty() && !chat.context_pending;
                // A new chat in a project with nothing set can set it.
                let default_for = chat
                    .project_id
                    .filter(|id| !started && core.project_granular(id).is_none());
                (
                    "Granular chat settings".to_string(),
                    value,
                    started,
                    default_for,
                )
            }
            GranularScope::Project(id) => {
                let project = settings.projects.iter().find(|p| &p.id == id)?;
                let value = core.project_granular(id).unwrap_or_else(|| Granular {
                    main: default_pick(&settings, None),
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
        let default = default_for
            .as_deref()
            .and_then(|id| core.project_granular(id));
        Some(cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Self {
                core,
                settings,
                models,
                scope,
                title: title.into(),
                value,
                agent_fixed,
                focus,
                menu: None,
                first,
                default_for,
                default,
                saving: false,
                error: None,
            }
        }))
    }

    /// The agent new chats start on, with its model from Settings.
    pub(super) fn default_pick(&self, agent: Option<AgentKind>) -> AgentPick {
        default_pick(&self.settings, agent)
    }

    /// The models `agent` offers.
    pub(super) fn models_of(&self, agent: AgentKind) -> Option<&[ModelOption]> {
        self.models.get(&agent).map(Vec::as_slice)
    }

    /// Saves the card's settings off the UI thread, since a project's go to each of its chats,
    /// and closes it once they are saved.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        self.saving = true;
        self.menu = None;
        let (core, scope, value) = (
            Arc::clone(&self.core),
            self.scope.clone(),
            self.value.clone(),
        );
        let task = cx.background_spawn(async move {
            match &scope {
                GranularScope::Chat(id) => core.set_chat_granular(id, value),
                GranularScope::Project(id) => core.set_project_granular(id, value),
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let saved = task.await;
            let _ = this.update(cx, |this, cx| match saved {
                Ok(()) => this.close(true, cx),
                Err(e) => {
                    this.saving = false;
                    this.error = Some(e.to_string().into());
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
    pub(super) fn toggle_default(&mut self, cx: &mut Context<Self>) {
        let Some(id) = &self.default_for else {
            return;
        };
        let changed = if self.default.as_ref() == Some(&self.value) {
            self.core.clear_project_granular_default(id)
        } else {
            self.core
                .set_project_granular_default(id, self.value.clone())
        };
        match changed {
            Ok(()) => {
                self.default = self.core.project_granular(id);
                // As saved, without the picks that could never act.
                if let Some(saved) = &self.default {
                    self.value = saved.clone();
                }
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string().into()),
        }
        cx.notify();
    }

    /// Closes the card, handing the window the first message to send when the settings were
    /// saved, or to put back in the chat box when they were not.
    pub(super) fn close(&mut self, saved: bool, cx: &mut Context<Self>) {
        cx.emit(GranularEvent::Closed {
            scope: self.scope.clone(),
            first: self.first.take(),
            saved,
        });
    }

    /// Puts a slot back: the agent to the one new chats start on, and the rest to none.
    pub(super) fn reset(&mut self, slot: Slot, cx: &mut Context<Self>) {
        let kept = self.agent_fixed.then_some(self.value.main.agent);
        let main = self.default_pick(kept);
        let value = &mut self.value;
        match slot {
            Slot::Main if main.agent == value.main.agent => value.main = main,
            Slot::Main => set_agent(value, slot, Some(main)),
            Slot::Executor => value.reserves.executor = None,
        }
        self.error = None;
        cx.notify();
    }
}
