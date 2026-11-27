//! The composer's state: its draft per chat, and the agent, model and options it sends with.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::collections::HashMap;
use gpui::{Entity, FocusHandle, Pixels, Point, WeakEntity, Window, prelude::*};
use patchwork_backend::agents::Agent;
use patchwork_backend::provider::{AgentKind, ModelOption};
use patchwork_backend::settings::Settings;
use patchwork_backend::chats::{AccessMode, Attachment, NewChat};
use patchwork_ui_editor::{Arrow, Changed, Edge, Submitted, Tabbed, TextArea};
use crate::workspace::Workspace;
use super::menus::{Menu, Pick, Roles};
use super::model_picker::{ModelPicker, ordered};
use super::model_notice;
use super::sigil_menu;

/// What the empty editor says, as the web composer's.
pub(super) const PLACEHOLDER: &str = "Ask anything, @ to mention, / for actions";

/// What a new chat starts with, picked before its first message is sent.
pub(super) struct Draft {
    pub(super) agent: Option<AgentKind>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) access: AccessMode,
    pub(super) compact: Option<u64>,
    pub(super) role: Option<String>,
}

/// The values the chips show: the open chat's, or the draft's before it starts.
pub struct Picks {
    pub agent: AgentKind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub access: AccessMode,
    pub compact: Option<u64>,
    /// The size Settings → Tokens gives the agent, which `compact: None` follows.
    pub fallback_window: u64,
    pub role: Option<String>,
}

/// The project the chats a request starts from the Orchestrator page go in.
pub enum LaunchProject {
    /// The project open in the sidebar, until one is picked.
    Open,
    /// The project picked on the page, or None for none.
    Picked(Option<String>),
}

pub struct Composer {
    pub(super) workspace: WeakEntity<Workspace>,
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
    pub(super) picker: Option<ModelPicker>,
    /// How far along the effort slider the pointer holds its knob, 0 to 1, while it is dragged.
    pub(super) effort_drag: Option<f32>,
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
    /// The agent and model picked on the Orchestrator page for the chats a request starts.
    pub(super) launch: Option<(AgentKind, Option<String>)>,
    /// The project for the chats a request starts from the Orchestrator page.
    pub(super) launch_project: LaunchProject,
    /// The open model picker is the Orchestrator page's, not the chat's.
    pub(super) launching: bool,
    /// Where Up has stepped to among the messages sent in this chat.
    pub(super) recalled: Option<super::recall::Recall>,
}

impl Composer {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
        cx.subscribe(&editor, |this, _, e: &Arrow, cx| this.sigil_arrow(e.0, cx))
            .detach();
        cx.subscribe_in(&editor, window, |this, _, e: &Edge, window, cx| {
            this.recall(e.0, window, cx);
        })
        .detach();
        cx.subscribe_in(&editor, window, |this, _, _: &Tabbed, window, cx| {
            this.sigil_tab(window, cx);
        })
        .detach();
        // The saved favourites, read off the UI thread once the workspace this belongs to is made.
        cx.spawn(async move |this, cx| {
            let core = this.update(cx, |this, cx| {
                this.workspace
                    .upgrade()
                    .map(|w| std::sync::Arc::clone(&w.read(cx).core))
            });
            let Ok(Some(core)) = core else {
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
        for kind in patchwork_desktop_shared::AGENT_KINDS {
            load_models(kind, cx);
        }
        Self {
            workspace,
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
            picker: None,
            effort_drag: None,
            queue_next: None,
            was_working: false,
            health: model_notice::ModelHealth::default(),
            queues: false,
            sigil: sigil_menu::SigilMenu::default(),
            recalled: None,
            favorites: Vec::new(),
            launch: None,
            launch_project: LaunchProject::Open,
            launching: false,
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

    pub(super) fn picks(&self, w: &Workspace) -> Picks {
        let default_agent = w.settings.default_agent();
        let (agent, model, effort, access, compact, role) = match &w.chat {
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
            fallback_window: w.settings.tokens.compact_window(agent),
            role,
        }
    }

    /// A new chat in `project` with the default agent and its default model, and the access,
    /// effort and compact window last picked.
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
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let w = workspace.read(cx);
        let core = Arc::clone(&w.core);
        let project = w.project().map(|p| p.id.clone());
        // The same project's agents stay listed while they refresh, rather than flashing a
        // loading line each time a menu opens.
        if self.roles_project != project {
            self.roles = Roles::Loading;
            self.roles_project.clone_from(&project);
        }
        patchwork_ui_theme::off_thread(
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
pub(super) fn offered(w: &Workspace, agent: AgentKind) -> Vec<AgentKind> {
    let started = w
        .chat
        .as_ref()
        .is_some_and(|t| !t.messages.is_empty() && !t.context_pending);
    if started {
        return vec![agent];
    }
    let mut agents = w.settings.agents.clone();
    if !agents.contains(&agent) {
        agents.push(agent);
    }
    ordered(&agents)
}

/// Lists `kind`'s models off the UI thread and keeps them when they arrive.
fn load_models(kind: AgentKind, cx: &mut Context<Composer>) {
    let task = cx.background_spawn(async move { patchwork_backend::provider::list_models(kind) });
    cx.spawn(async move |this, cx| {
        let models = task.await;
        this.update(cx, |this, cx| {
            this.models.insert(kind, models);
            cx.notify();
        })
    })
    .detach();
}
