//! The composer's state: its draft per chat, and the agent, model and options it sends with.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use gpui::{App, Entity, EventEmitter, FocusHandle, Pixels, Point, Window, prelude::*};
use patchwork_client::local::core::agents::Agent;
use patchwork_model::provider::{AgentKind, ModelOption};
use patchwork_client::local::core::settings::Settings;
use patchwork_client::local::core::chats::{AccessMode, Attachment, NewChat};
use patchwork_design_editor::{Arrow, Changed, Edge, Submitted, Tabbed, TextArea};
use crate::host::{ChatEvent, ChatHost};
use super::menus::{Menu, Pick, Roles};
use super::model_picker::ordered;
use super::model_notice;
use super::sigil_menu;

/// What the empty editor says, as the web composer's.
pub(super) const PLACEHOLDER: &str = "Ask anything, @ to mention, / for actions";

/// What a new chat starts with, picked before its first message is sent.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Draft {
    pub(crate) agent: Option<AgentKind>,
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) access: AccessMode,
    pub(crate) compact: Option<u64>,
    pub(crate) role: Option<String>,
}

/// The values the chips show: the open chat's, or the draft's before it starts.
#[derive(Clone, Debug)]
pub struct Picks {
    /// The agent the chat runs.
    pub agent: AgentKind,
    /// The model, or none for the agent's default.
    pub model: Option<String>,
    /// The reasoning effort, or none for the model's default.
    pub effort: Option<String>,
    /// What the agent may do without asking.
    pub access: AccessMode,
    /// Where the context compacts, or none for `fallback_window`.
    pub compact: Option<u64>,
    /// The size Settings → Tokens gives the agent, which `compact: None` follows.
    pub fallback_window: u64,
    /// The custom agent the chat runs as.
    pub role: Option<String>,
}

/// The box a message is written in, with the agent, model and options it sends with.
pub struct Composer {
    pub(super) host: Rc<dyn ChatHost>,
    /// The editor the message is written in.
    pub editor: Entity<TextArea>,
    pub(super) attachments: Vec<Attachment>,
    /// Each agent's models, read once off the main thread.
    pub(super) models: HashMap<AgentKind, Vec<ModelOption>>,
    pub(super) draft: Draft,
    pub(super) menu: Option<Menu>,
    /// Where the press that last closed a menu by landing outside it was, so the same press on
    /// the menu's own chip does not open it again; a press on another chip opens that one.
    pub(super) dismissed: Option<(Menu, Point<Pixels>)>,
    pub(super) menu_focus: FocusHandle,
    /// What had the keys when a chip's menu opened, which gets them back when it closes.
    pub(super) menu_came_from: Option<FocusHandle>,
    /// The row under the keyboard, or `usize::MAX` until the open menu is drawn.
    pub(super) highlight: usize,
    /// What each row of the open menu picks, in order, for the keyboard.
    pub(super) menu_picks: Vec<Pick>,
    pub(super) roles: Roles,
    /// The project `roles` were listed for, so a refresh for the same one keeps showing them.
    pub(super) roles_project: Option<String>,
    /// The model, agent or permissions menu while open, and the shape of the rows it was built
    /// with, so it is built again when they change, as when the models or agents are listed.
    pub(super) context_menu: Option<(Entity<patchwork_design_components::menu::Menu>, String)>,
    /// Whether the next send while a turn runs queues rather than steers, when picked from the
    /// send menu. Each send, and each turn starting or ending, puts it back to the default.
    pub(super) queue_next: Option<bool>,
    pub(super) was_working: bool,
    /// What the next send while a turn runs does, as last drawn: queue, or steer.
    pub(super) queues: bool,
    pub(super) health: model_notice::ModelHealth,
    pub(super) sigil: sigil_menu::SigilMenu,
    /// The favourite models, as `agent:model` keys, read once in the background and saved there.
    pub(super) favorites: Vec<String>,
    /// Where Up has stepped to among the messages sent in this chat.
    pub(super) recalled: Option<super::recall::Recall>,
}

impl std::fmt::Debug for Composer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Composer")
            .field("attachments", &self.attachments.len())
            .field("draft", &self.draft)
            .field("menu", &self.menu)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<ChatEvent> for Composer {}

impl Composer {
    /// A composer for the window `host` gives it the chat of.
    pub fn new(host: Rc<dyn ChatHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            let mut area = TextArea::new(PLACEHOLDER, "Message", cx);
            area.markdown = true;
            area
        });
        cx.subscribe_in(&editor, window, |this, _, _: &Submitted, window, cx| {
            let m = window.modifiers();
            let alternate = m.control || m.platform;
            // Ctrl+Enter always sends; plain Enter picks from an open sigil menu first.
            if !alternate && this.sigil_enter(window, cx) {
                return;
            }
            this.send(alternate, window, cx);
        })
        .detach();
        cx.subscribe(&editor, |this, _, _: &Changed, cx| {
            this.draft_changed();
            cx.notify();
        })
        .detach();
        cx.subscribe_in(&editor, window, |this, _, e: &Arrow, window, cx| {
            this.sigil_arrow(e.0, window, cx);
        })
        .detach();
        cx.subscribe_in(&editor, window, |this, _, e: &Edge, window, cx| {
            this.recall(e.0, window, cx);
        })
        .detach();
        cx.subscribe_in(&editor, window, |this, _, _: &Tabbed, window, cx| {
            this.sigil_tab(window, cx);
        })
        .detach();
        // The saved favourites, read off the UI thread.
        cx.spawn(async move |this, cx| {
            let Ok(core) = this.update(cx, |_, cx| Arc::clone(AppState::get(cx).core())) else {
                return;
            };
            let favorites = cx
                .background_spawn(async move { super::model_picker::read_favorites(&core) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.favorites = favorites;
                cx.notify();
            });
        })
        .detach();
        // The caret moving changes the word a menu is for.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        // Each agent's list arrives on its own, so a slow CLI holds up only its own label.
        for kind in patchwork_views_logos::AGENT_KINDS {
            load_models(kind, cx);
        }
        Self {
            host,
            editor,
            attachments: Vec::new(),
            models: HashMap::new(),
            draft: Draft {
                agent: None,
                model: None,
                effort: None,
                access: AccessMode::FullAccess,
                compact: None,
                role: None,
            },
            menu: None,
            dismissed: None,
            menu_focus: cx.focus_handle(),
            menu_came_from: None,
            highlight: usize::MAX,
            menu_picks: Vec::new(),
            roles: Roles::Loading,
            roles_project: None,
            context_menu: None,
            queue_next: None,
            was_working: false,
            health: model_notice::ModelHealth::default(),
            queues: false,
            sigil: sigil_menu::SigilMenu::default(),
            recalled: None,
            favorites: Vec::new(),
        }
    }

    pub fn models_of(&self, kind: AgentKind) -> Option<&[ModelOption]> {
        self.models.get(&kind).map(Vec::as_slice)
    }

    /// Asks again for the models of each of `agents` that offered only its CLI default.
    ///
    /// A CLI installed after the composer was made, as during onboarding, listed nothing but
    /// "Default" when the composer first asked.
    pub(super) fn reload_bare_models(&self, agents: &[AgentKind], cx: &mut Context<Self>) {
        for &kind in agents {
            if self.models_of(kind).is_none_or(|models| models.len() <= 1) {
                load_models(kind, cx);
            }
        }
    }

    pub(super) fn picks(&self, cx: &App) -> Picks {
        let default_agent = AppState::get(cx).settings.default_agent();
        let (agent, model, effort, access, compact, role) = match self.host.chat(cx) {
            Some(t) => (
                t.agent,
                t.model.clone(),
                t.effort.clone(),
                t.access_mode,
                t.compact_window,
                t.role.clone(),
            ),
            None => (
                self.draft.agent.unwrap_or(default_agent),
                self.draft.model.clone(),
                self.draft.effort.clone(),
                self.draft.access,
                self.draft.compact,
                self.draft.role.clone(),
            ),
        };
        Picks {
            agent,
            model,
            effort,
            access,
            compact,
            fallback_window: AppState::get(cx).settings.tokens.compact_window(agent),
            role,
        }
    }

    /// A new chat in `project` with the default agent and its default model, and the access,
    /// effort and compact window last picked.
    #[must_use]
    pub fn new_chat(&self, project_id: Option<String>, settings: &Settings) -> NewChat {
        let agent = settings.default_agent();
        let effort = self
            .draft
            .effort
            .clone()
            .filter(|e| agent.model_efforts(None, self.models_of(agent)).contains(e));
        NewChat {
            id: None,
            project_id,
            agent,
            model: settings.tokens.default_models.get(&agent).cloned(),
            access_mode: self.draft.access,
            effort,
            compact_window: self.draft.compact,
        }
    }

    /// Has the next new chat start with `agent`'s agent, model, effort and access, for its page.
    pub fn pick_as(&mut self, agent: &Agent, cx: &mut Context<Self>) {
        self.draft.agent = Some(agent.agent);
        self.draft.model.clone_from(&agent.model);
        self.draft.effort.clone_from(&agent.effort);
        self.draft.access = agent.access_mode;
        self.draft.role = None;
        cx.notify();
    }

    /// Reads the custom agents the chat's project can summon, for the open menu.
    pub(super) fn load_roles(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(AppState::get(cx).core());
        let project = self.host.project(cx).map(|p| p.id.clone());
        // The same project's agents stay listed while they refresh, rather than flashing a
        // loading line each time a menu opens.
        if self.roles_project != project {
            self.roles = Roles::Loading;
            self.roles_project.clone_from(&project);
        }
        patchwork_design_tasks::off_thread(
            cx,
            move || core.project_agents(project.as_deref()),
            move |this, agents, cx| {
                this.roles = Roles::Loaded(agents);
                cx.notify();
            },
        );
    }
}

/// The agents the model picker offers: any connected one before the chat's first message,
/// and only its own after.
pub(super) fn offered(host: &dyn ChatHost, agent: AgentKind, cx: &App) -> Vec<AgentKind> {
    let started = host
        .chat(cx)
        .is_some_and(|t| !t.messages.is_empty() && !t.context_pending);
    if started {
        return vec![agent];
    }
    let mut agents = AppState::get(cx).settings.agents.clone();
    if !agents.contains(&agent) {
        agents.push(agent);
    }
    ordered(&agents)
}

/// Lists `kind`'s models off the UI thread and keeps them when they arrive.
fn load_models(kind: AgentKind, cx: &mut Context<Composer>) {
    let task =
        cx.background_spawn(
            async move { patchwork_client::local::core::provider::list_models(kind) },
        );
    cx.spawn(async move |this, cx| {
        let models = task.await;
        this.update(cx, |this, cx| {
            this.models.insert(kind, models);
            cx.notify();
        })
    })
    .detach();
}
