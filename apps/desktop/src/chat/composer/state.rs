//! The composer's state: its draft per chat, and the agent, model and options it sends with.

use std::collections::HashMap;
use gpui::{Context, Entity, FocusHandle, Pixels, Point, WeakEntity, Window, prelude::*};
use patchwork_server::provider::{AgentKind, ModelOption};
use patchwork_server::settings::Settings;
use patchwork_server::threads::{AccessMode, Attachment, NewThread};
use crate::chat::text_area::{Arrow, Changed, Submitted, Tabbed, TextArea};
use crate::ui::theme::theme;
use crate::workspace::Workspace;
use super::menus::{Menu, Pick, Roles, efforts};
use super::model_picker::{ModelPicker, ordered};
use super::{model_notice};
use super::{sigil_menu};

/// What a new chat starts with, picked before its first message is sent.
pub(super) struct Draft {
    pub(super) agent: Option<AgentKind>,
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) access: AccessMode,
    pub(super) compact: Option<u64>,
    pub(super) role: Option<String>,
    pub(super) plan_mode: bool,
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
    /// The row under the keyboard, or `usize::MAX` until the open menu is drawn.
    pub(super) highlight: usize,
    /// What each row of the open menu picks, in order, for the keyboard.
    pub(super) menu_picks: Vec<Pick>,
    pub(super) roles: Roles,
    pub(super) picker: Option<ModelPicker>,
    /// Whether the next send while a turn runs queues rather than steers, when picked from the
    /// send menu. Each send, and each turn starting or ending, puts it back to the default.
    pub(super) queue_next: Option<bool>,
    pub(super) was_working: bool,
    /// No queued message waits, so another can be queued.
    pub(super) can_queue: bool,
    /// What the next send while a turn runs does, as last drawn: queue, or steer.
    pub(super) queues: bool,
    pub(super) health: model_notice::ModelHealth,
    pub(super) sigil: sigil_menu::SigilMenu,
}

impl Composer {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let t = *theme(cx);
        let editor = cx.new(|cx| {
            let mut area = TextArea::new("Reply or describe the next change…", "Message", cx);
            area.text_color = t.text;
            area.placeholder_color = t.text_subtle;
            area.selection_color = t.accent.opacity(0.3);
            area
        });
        cx.subscribe_in(&editor, window, |this, _, _: &Submitted, window, cx| {
            if this.sigil_enter(window, cx) {
                return;
            }
            let m = window.modifiers();
            let alternate = m.control || m.platform;
            this.send(alternate, true, window, cx)
        })
        .detach();
        cx.subscribe(&editor, |this, _, _: &Changed, cx| {
            this.draft_changed();
            cx.notify();
        })
        .detach();
        cx.subscribe(&editor, |this, _, e: &Arrow, cx| this.sigil_arrow(e.0, cx))
            .detach();
        cx.subscribe_in(&editor, window, |this, _, _: &Tabbed, window, cx| {
            this.sigil_tab(window, cx)
        })
        .detach();
        // The caret moving changes the word a menu is for.
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        // Each agent's list arrives on its own, so a slow CLI holds up only its own label.
        for kind in crate::ui::AGENT_KINDS {
            let task =
                cx.background_spawn(async move { patchwork_server::provider::list_models(kind) });
            cx.spawn(async move |this, cx| {
                let models = task.await;
                this.update(cx, |this, cx| {
                    this.models.insert(kind, models);
                    cx.notify();
                })
            })
            .detach();
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
                plan_mode: false,
            },
            menu: None,
            dismissed: None,
            menu_focus: cx.focus_handle(),
            highlight: usize::MAX,
            menu_picks: Vec::new(),
            roles: Roles::Loading,
            picker: None,
            queue_next: None,
            was_working: false,
            health: Default::default(),
            can_queue: true,
            queues: false,
            sigil: Default::default(),
        }
    }

    pub(super) fn models_of(&self, kind: AgentKind) -> Option<&[ModelOption]> {
        self.models.get(&kind).map(Vec::as_slice)
    }

    pub(super) fn picks(&self, w: &Workspace) -> Picks {
        let default_agent = w
            .settings
            .agents
            .first()
            .copied()
            .unwrap_or(AgentKind::Claude);
        let (agent, model, effort, access, compact, role) = match &w.thread {
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

    /// A new thread in `project` with the default agent and its default model, and the access,
    /// effort and compact window last picked.
    pub fn new_thread(&self, project_id: Option<String>, settings: &Settings) -> NewThread {
        let agent = settings
            .agents
            .first()
            .copied()
            .unwrap_or(AgentKind::Claude);
        let effort = self
            .draft
            .effort
            .clone()
            .filter(|e| efforts(agent, None, self.models_of(agent)).contains(e));
        NewThread {
            id: None,
            project_id,
            agent,
            model: settings.tokens.default_models.get(&agent).cloned(),
            access_mode: self.draft.access,
            effort,
            compact_window: self.draft.compact,
            plan_mode: false,
        }
    }

    /// The agents the model picker offers: any connected one before the chat's first message,
    /// and only its own after.
    pub(super) fn offered(w: &Workspace, agent: AgentKind) -> Vec<AgentKind> {
        let started = w
            .thread
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

    /// Reads the custom agents the chat's project can summon, for the open menu.
    pub(super) fn load_roles(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let w = workspace.read(cx);
        let server = w.server.clone();
        let project = w.project().map(|p| p.id.clone());
        self.roles = Roles::Loading;
        let task = cx.background_spawn(async move { server.project_agents(project.as_deref()) });
        cx.spawn(async move |this, cx| {
            let agents = task.await;
            this.update(cx, |this, cx| {
                this.roles = Roles::Loaded(agents);
                cx.notify();
            })
        })
        .detach();
    }
}
