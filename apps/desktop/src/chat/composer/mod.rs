//! The box a message is written in (`features/chat/composer/Composer.tsx`): the editor, the files
//! attached to the message, and the toolbar of pickers, formatting, attach, stop and send.

mod menus;
mod model_notice;
mod model_picker;
mod sigil_menu;

use std::collections::HashMap;
use std::f32::consts::TAU;

use gpui::{
    AnyElement, ClickEvent, KeyDownEvent, Context, Div, Entity, FocusHandle, Focusable, FontWeight,
    IntoElement, MouseButton, PathBuilder, PathPromptOptions, Pixels, Point, Render, Role,
    SharedString, Stateful, WeakEntity, Window, canvas, div, point, prelude::*, px,
};
use patchwork_server::provider::{AgentKind, ModelOption};
use patchwork_server::settings::Settings;
use patchwork_server::threads::{AccessMode, Attachment, AttachmentKind, GoalStatus, NewThread, Thread};

use crate::chat::text_area::{Arrow, Changed, Submitted, Tabbed, TextArea};
use crate::chat::user::attachment_chip;
use crate::theme::{FS_MD, RADIUS, RADIUS_LG, RADIUS_SM, Theme, theme};
use crate::ui::{above, agent_logo, button, icon, tooltip};
use crate::workspace::{Workspace, is_busy, is_working};
use menus::{Menu, Pick, Roles, access_of, compact_label, effort_label, efforts};
use model_picker::{ModelPicker, ordered};

/// What a new chat starts with, picked before its first message is sent.
struct Draft {
    agent: Option<AgentKind>,
    model: Option<String>,
    effort: Option<String>,
    access: AccessMode,
    compact: Option<u64>,
    role: Option<String>,
    plan_mode: bool,
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
    workspace: WeakEntity<Workspace>,
    pub editor: Entity<TextArea>,
    attachments: Vec<Attachment>,
    /// Each agent's models, read once off the main thread.
    models: HashMap<AgentKind, Vec<ModelOption>>,
    draft: Draft,
    menu: Option<Menu>,
    /// Where the press that last closed a menu by landing outside it was, so the same press on
    /// the menu's own chip does not open it again; a press on another chip opens that one.
    dismissed: Option<(Menu, Point<Pixels>)>,
    menu_focus: FocusHandle,
    /// The row under the keyboard, or `usize::MAX` until the open menu is drawn.
    highlight: usize,
    /// What each row of the open menu picks, in order, for the keyboard.
    menu_picks: Vec<Pick>,
    roles: Roles,
    picker: Option<ModelPicker>,
    /// Whether the next send while a turn runs queues rather than steers, when picked from the
    /// send menu. Each send, and each turn starting or ending, puts it back to the default.
    queue_next: Option<bool>,
    was_working: bool,
    /// No queued message waits, so another can be queued.
    can_queue: bool,
    /// What the next send while a turn runs does, as last drawn: queue, or steer.
    queues: bool,
    health: model_notice::ModelHealth,
    sigil: sigil_menu::SigilMenu,
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

    fn models_of(&self, kind: AgentKind) -> Option<&[ModelOption]> {
        self.models.get(&kind).map(Vec::as_slice)
    }

    fn picks(&self, w: &Workspace) -> Picks {
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
    fn offered(w: &Workspace, agent: AgentKind) -> Vec<AgentKind> {
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

    /// Sends the draft. While a turn runs it queues or steers, as the send menu picked;
    /// `alternate` (Ctrl+Enter) does the other one, or steers with the first queued message.
    /// `ask_resume` asks first when the session has gone cold, unless that was just answered.
    fn send(
        &mut self,
        alternate: bool,
        ask_resume: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.editor.read(cx).content.trim().to_string();
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        // Nothing runs while the project's folder is missing.
        if workspace.read(cx).folder_missing() {
            return;
        }
        let w = workspace.read(cx);
        let working = w.thread.as_ref().is_some_and(is_working);
        // Same as clicking the queued message's Steer button. The draft is not sent.
        if alternate && working && !w.queued().is_empty() && !w.editing_queued() {
            workspace.update(cx, |w, cx| w.steer_first_queued(cx));
            return;
        }
        if w.editing_queued() {
            workspace.update(cx, |w, cx| w.update_queued(text, cx));
            return;
        }
        // With a plan ready, sending nothing implements it; anything written refines it.
        if text.is_empty() && self.attachments.is_empty() && w.ready_plan().is_some() {
            workspace.update(cx, |w, cx| w.implement_plan(false, cx));
            return;
        }
        if self.attachments.is_empty()
            && let Some(command) = sigil_menu::local_command(&text)
        {
            return self.run_local(command, window, cx);
        }
        // Mid-turn, /goal clear is the Stop button: the agent's session drops the goal once it
        // exits.
        if working
            && w.thread.as_ref().is_some_and(|t| t.goal.is_some())
            && crate::chat::rail::is_goal_clear(&text)
            && self.attachments.is_empty()
        {
            self.editor.update(cx, |e, cx| e.clear(window, cx));
            workspace.update(cx, |w, cx| w.stop(cx));
            return;
        }
        let queue_next = self.queue_next.unwrap_or_else(|| Self::queue_by_default(w));
        let queue = working && (alternate != queue_next);
        if queue && !self.can_queue {
            return;
        }
        // An answer written here goes to the agent's question, not into the chat.
        if workspace.read(cx).answering_in_composer() {
            if !text.is_empty() {
                workspace.update(cx, |w, cx| w.answer_question(text, cx));
                self.editor.update(cx, |e, cx| e.clear(window, cx));
            }
            return;
        }
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        // Resuming a large session whose cache has expired re-reads all of it at full price.
        if ask_resume && workspace.update(cx, |w, cx| w.ask_before_resume(cx)) {
            return;
        }
        let picks = self.picks(workspace.read(cx));
        let new = NewThread {
            id: None,
            project_id: workspace.read(cx).settings.active_project_id.clone(),
            agent: picks.agent,
            model: picks.model,
            access_mode: picks.access,
            effort: picks.effort,
            compact_window: picks.compact,
            plan_mode: self.draft.plan_mode,
        };
        let attachments = std::mem::take(&mut self.attachments);
        workspace.update(cx, |w, cx| {
            w.send_with(text, attachments, Some(new), picks.role, queue, cx)
        });
        self.editor.update(cx, |e, cx| e.clear(window, cx));
        self.queue_next = None;
        cx.notify();
    }

    /// Sends the draft once the resume prompt is answered with Continue.
    pub fn send_unasked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send(false, false, window, cx);
    }

    /// Holds the next message for after the running turn, unless the setting makes Enter steer
    /// a running reply. While a goal runs, follow-ups wait by default.
    fn queue_by_default(w: &Workspace) -> bool {
        let goal_active = w
            .thread
            .as_ref()
            .and_then(|t| t.goal.as_ref())
            .is_some_and(|g| g.status == GoalStatus::Active);
        goal_active || !w.settings.steer_while_working
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.editor.focus_handle(cx), cx);
    }

    /// Puts a restored message back to edit or send again, unless something is written already.
    pub fn fill_if_empty(
        &mut self,
        text: &str,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor.read(cx).content.trim().is_empty() || !self.attachments.is_empty() {
            return;
        }
        // The composer lists images with the other attachments, not inline in the text.
        let text = crate::chat::user::without_image_markdown(text);
        self.editor.update(cx, |e, cx| e.set_text(&text, cx));
        self.attachments = attachments;
        self.focus(window, cx);
        cx.notify();
    }

    fn attach(&mut self, folders: bool, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update(cx, |this, cx| {
                for path in paths {
                    let kind = if folders {
                        AttachmentKind::Folder
                    } else if patchwork_server::threads::image_media_type(&path.to_string_lossy())
                        .is_some()
                    {
                        AttachmentKind::Image
                    } else {
                        AttachmentKind::File
                    };
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let path = path.to_string_lossy().into_owned();
                    if !this.attachments.iter().any(|a| a.path == path) {
                        this.attachments.push(Attachment { kind, path, name });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A chip was clicked: opens its menu, or closes it if it is open. The press that just
    /// closed it from outside does not open it again.
    fn toggle_menu(
        &mut self,
        menu: Menu,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ClickEvent::Mouse(click) = event
            && self.dismissed.take() == Some((menu, click.down.position))
        {
            return;
        }
        if self.menu == Some(menu) {
            return self.close_menu(window, cx);
        }
        self.open_menu(menu, window, cx);
    }

    /// Opens a chip's menu, as clicking it does.
    fn open_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = Some(menu);
        self.picker = None;
        self.highlight = usize::MAX;
        match menu {
            Menu::Model => {
                if let Some(workspace) = self.workspace.upgrade() {
                    let w = workspace.read(cx);
                    let picks = self.picks(w);
                    let agents = Self::offered(w, picks.agent);
                    self.open_model_picker(&agents, (picks.agent, picks.model), window, cx);
                }
            }
            Menu::Agent => {
                self.load_roles(cx);
                window.focus(&self.menu_focus, cx);
            }
            _ => window.focus(&self.menu_focus, cx),
        }
        cx.notify();
    }

    /// Reads the custom agents the chat's project can summon, for the open menu.
    fn load_roles(&mut self, cx: &mut Context<Self>) {
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

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.picker = None;
        let focus = self.editor.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Applies a menu's pick to the open chat, or to the draft of the next one.
    fn apply(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
        if let Pick::SendMode(queue) = pick {
            if !queue || self.can_queue {
                self.queue_next = Some(queue);
                self.close_menu(window, cx);
            }
            return;
        }
        self.close_menu(window, cx);
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let thread = workspace.read(cx).thread.as_ref().map(|t| t.id.clone());
        let Some(id) = thread else {
            match pick {
                Pick::Model(agent, model) => {
                    self.draft.agent = Some(agent);
                    self.draft.model = model;
                }
                Pick::Effort(effort) => self.draft.effort = effort,
                Pick::Compact(window) => self.draft.compact = window,
                Pick::Access(mode) => self.draft.access = mode,
                Pick::Role(role) => self.draft.role = role,
                Pick::CompactNow | Pick::SendMode(_) => {}
            }
            return;
        };
        // New chats start with the access, effort and compact window last picked.
        match &pick {
            Pick::Effort(effort) => self.draft.effort = effort.clone(),
            Pick::Compact(window) => self.draft.compact = *window,
            Pick::Access(mode) => self.draft.access = *mode,
            _ => {}
        }
        workspace.update(cx, |w, cx| {
            let server = w.server.clone();
            let done = match pick {
                Pick::Model(agent, model) => server.set_thread_model(&id, agent, model),
                Pick::Effort(effort) => server.set_thread_effort(&id, effort),
                Pick::Compact(window) => server.set_thread_compact_window(&id, window),
                Pick::Access(mode) => server.set_thread_access_mode(&id, mode),
                Pick::Role(role) => server.set_thread_role(&id, role),
                Pick::CompactNow => {
                    w.send_command("/compact", cx);
                    Ok(())
                }
                Pick::SendMode(_) => Ok(()),
            };
            if let Err(e) = done {
                log::warn!("could not change the chat: {e}");
            }
            w.reload(cx);
        });
    }

    /// A chip, with its menu open above it when it is the open one.
    #[allow(clippy::too_many_arguments)]
    fn chip_with_menu(
        &mut self,
        menu: Menu,
        chip: Stateful<Div>,
        picks: &Picks,
        can_compact: bool,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let open = self.menu == Some(menu);
        let chip = chip.aria_expanded(open).on_click(
            cx.listener(move |this, e, window, cx| this.toggle_menu(menu, e, window, cx)),
        );
        let body = open.then(|| {
            if menu == Menu::Model {
                let agents = self
                    .workspace
                    .upgrade()
                    .map(|w| Self::offered(w.read(cx), picks.agent))
                    .unwrap_or_default();
                return self.model_picker(
                    &agents,
                    (picks.agent, picks.model.as_deref()),
                    t,
                    window,
                    cx,
                );
            }
            let rows = self.rows(menu, picks, can_compact, t);
            self.menu_picks = rows.iter().map(|r| r.pick.clone()).collect();
            if self.highlight >= rows.len() {
                self.highlight = rows.iter().position(|r| r.checked).unwrap_or(0);
            }
            let focus = self.menu_focus.clone();
            self.menu_box(menu, rows, &focus, t, cx)
        });
        div()
            .relative()
            .flex()
            .child(chip)
            .when_some(body, |d, body| d.child(above(body)))
    }
}

/// `CHIP_CLASS`: a picker's button, its value muted until hovered.
fn chip(
    id: &'static str,
    aria: String,
    lead: AnyElement,
    label: String,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, aria)
        .h(px(28.))
        .gap(px(4.))
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .text_size(px(FS_MD))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(lead)
        .child(div().max_w(px(180.)).truncate().child(label))
        .child(icon("caret-down", 14.))
}

/// A 32px square tool button: formatting and attaching.
fn tool_button(
    id: &'static str,
    label: &'static str,
    name: &'static str,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .size(px(32.))
        .flex_none()
        .justify_center()
        .rounded(px(RADIUS))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 16.))
}

/// How full the context is against where it compacts, as a ring that turns the warning colour
/// once compacting is near.
fn fill_ring(fill: f32, t: &Theme) -> AnyElement {
    let color = if fill >= 0.9 { t.warning } else { t.text_muted };
    canvas(
        |_, _, _| {},
        move |bounds, _, window: &mut Window, _| {
            let c = bounds.center();
            let r = 6.;
            let at = |turn: f32| {
                let a = (turn - 0.25) * TAU;
                point(c.x + px(r * a.cos()), c.y + px(r * a.sin()))
            };
            let radii = point(px(r), px(r));
            let mut ring = PathBuilder::stroke(px(2.));
            ring.move_to(at(0.));
            ring.arc_to(radii, px(0.), false, true, at(0.5));
            ring.arc_to(radii, px(0.), false, true, at(0.));
            if let Ok(path) = ring.build() {
                window.paint_path(path, color.opacity(0.3));
            }
            let fill = fill.min(1.);
            if fill > 0. {
                let mut arc = PathBuilder::stroke(px(2.));
                arc.move_to(at(0.));
                arc.arc_to(radii, px(0.), fill > 0.5, true, at(fill.min(0.9999)));
                if let Ok(path) = arc.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(16.))
    .flex_none()
    .into_any_element()
}

/// The chat's context now, from the last reply that reported it.
fn context_of(thread: &Thread) -> Option<u64> {
    thread
        .messages
        .iter()
        .rev()
        .find_map(|m| m.usage.as_ref().map(|u| u.context))
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let (agent, model, working, vision_set) = {
            let w = workspace.read(cx);
            let picks = self.picks(w);
            let working = w.thread.as_ref().is_some_and(is_working);
            (
                picks.agent,
                picks.model,
                working,
                w.settings.vision.is_some(),
            )
        };
        // A reply ending may have changed what the model's server serves.
        if working != self.was_working {
            self.recheck_model();
        }
        self.check_model(agent, model.as_deref(), working, cx);
        let notice = self.model_notice(agent, model.as_deref(), vision_set, &t, cx);
        let w = workspace.read(cx);
        let usage = w
            .thread
            .as_ref()
            .filter(|t| w.settings.tokens.show_usage && !t.messages.is_empty())
            .map(|thread| crate::chat::usage_line(thread, w.settings.tokens.warn_above, &t));
        let picks = self.picks(w);
        let folder_missing = w.folder_missing();
        let provider_commands = w
            .thread
            .as_ref()
            .map(|t| t.slash_commands.clone())
            .unwrap_or_default();
        let thread = w.thread.clone();
        let thread = thread.as_ref();
        let agent = picks.agent;
        let model_label = self
            .models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id == picks.model))
            .map(|m| m.label.clone())
            .or(picks.model.clone())
            .unwrap_or_else(|| "Default".into());
        let levels = efforts(agent, picks.model.as_deref(), self.models_of(agent));
        let effort = picks
            .effort
            .as_ref()
            .filter(|e| levels.contains(e))
            .map(|e| effort_label(e))
            .unwrap_or_else(|| "Default".into());
        let effective = picks.compact.unwrap_or(picks.fallback_window);
        let full = self
            .models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id == picks.model))
            .and_then(|m| m.context_window);
        let limit = if effective > 0 { Some(effective) } else { full };
        let ring = match limit {
            Some(limit) => fill_ring(
                thread.and_then(context_of).unwrap_or(0) as f32 / limit as f32,
                &t,
            ),
            None => icon("arrows-in-simple", 16.).into_any_element(),
        };
        let compact = compact_label(effective);
        let (access, access_icon) = access_of(picks.access);
        let working = thread.is_some_and(is_working);
        let busy = thread.is_some_and(is_busy);
        if working != self.was_working {
            self.was_working = working;
            self.queue_next = None;
            if self.menu == Some(Menu::Send) {
                self.menu = None;
            }
        }
        let editing = w.editing_queued();
        self.can_queue = w.queued().is_empty();
        let queue_next = self.queue_next.unwrap_or_else(|| Self::queue_by_default(w));
        let can_compact = thread.is_some_and(|t| !t.messages.is_empty()) && !working;
        let editor = self.editor.read(cx);
        let empty = editor.content.trim().is_empty() && self.attachments.is_empty();
        let focused = editor.focus_handle(cx).is_focused(window);
        let editor_focus = editor.focus_handle(cx);

        let format_editor = self.editor.clone();
        let format = |id: &'static str,
                      label: &'static str,
                      name: &'static str,
                      before: &'static str,
                      after: &'static str| {
            let editor = format_editor.clone();
            tool_button(id, label, name, &t).on_click(move |_, window, cx| {
                editor.update(cx, |e, cx| {
                    if after.is_empty() {
                        e.prefix_line(before, window, cx)
                    } else {
                        e.surround(before, after, window, cx)
                    }
                });
            })
        };
        let attachments: Vec<_> = self
            .attachments
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let (hover, fg) = (t.bg_hover, t.text);
                attachment_chip(format!("attachment-{i}"), a, &t).child(
                    button(
                        SharedString::from(format!("remove-{i}")),
                        format!("Remove {}", a.name),
                    )
                    .size(px(16.))
                    .justify_center()
                    .rounded_full()
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
                    .child(icon("x", 10.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.attachments.remove(i);
                        cx.notify();
                    })),
                )
            })
            .collect();
        let stop = busy.then(|| {
            let hover = t.accent_hover;
            button(
                "stop",
                if working {
                    "Stop"
                } else {
                    "Stop background commands"
                },
            )
            .size(px(32.))
            .flex_none()
            .justify_center()
            .rounded(px(RADIUS))
            .bg(t.bg_selected_muted)
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .child(icon("square-fill", 12.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.workspace.update(cx, |w, cx| w.stop(cx)).ok();
            }))
        });
        let send_label = if editing {
            "Update queued message"
        } else if !working {
            "Send"
        } else if queue_next {
            "Queue message"
        } else {
            "Steer message"
        };
        let send_tip = if editing {
            "Update queued message"
        } else if !working {
            "Send"
        } else if queue_next {
            "Queue for after the turn"
        } else {
            "Steer the running turn"
        };
        let split = working && !editing;
        let send = (!working || !empty || editing).then(|| {
            let base = button("send", send_label)
                .tooltip(tooltip(send_tip))
                .size(px(32.))
                .flex_none()
                .justify_center()
                .map(|b| {
                    if split {
                        b.rounded_l(px(RADIUS))
                    } else {
                        b.rounded(px(RADIUS))
                    }
                })
                .child(icon(if editing { "check" } else { "arrow-up" }, 16.));
            let blocked = folder_missing
                || (empty && !editing)
                || (working && queue_next && !self.can_queue && !editing);
            if blocked {
                base.bg(t.bg_selected_muted)
                    .text_color(t.text_subtle)
                    .cursor_default()
            } else {
                let hover = t.accent_hover;
                base.bg(t.accent)
                    .text_color(t.text_on_accent)
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(|this, _, window, cx| this.send(false, true, window, cx)))
            }
        });
        let send_mode = (split && !empty).then(|| {
            let hover = t.accent_hover;
            let caret = button("send-mode", "Choose queue or steer")
                .tooltip(tooltip("Choose queue or steer"))
                .h(px(32.))
                .w(px(24.))
                .flex_none()
                .justify_center()
                .rounded_r(px(RADIUS))
                .border_l_1()
                .border_color(t.text_on_accent.opacity(0.2))
                .bg(t.accent)
                .text_color(t.text_on_accent)
                .hover(move |s| s.bg(hover))
                .child(icon("caret-down", 12.));
            self.chip_with_menu(Menu::Send, caret, &picks, can_compact, &t, window, cx)
        });
        let model_chip = self.chip_with_menu(
            Menu::Model,
            chip(
                "model",
                format!("Choose model: {model_label}"),
                agent_logo(agent, picks.model.as_deref(), 16.),
                model_label,
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        // A model that takes no level has nothing to pick, so its chip would only take up room.
        let effort_chip = (!levels.is_empty()).then(|| {
            self.chip_with_menu(
                Menu::Effort,
                chip(
                    "effort",
                    format!("Effort: {effort}"),
                    icon("lightbulb", 16.).into_any_element(),
                    effort,
                    &t,
                ),
                &picks,
                can_compact,
                &t,
                window,
                cx,
            )
        });
        let compact_chip = self.chip_with_menu(
            Menu::Compact,
            chip(
                "compact",
                format!("Compact at: {compact}"),
                ring,
                compact,
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let access_chip = self.chip_with_menu(
            Menu::Access,
            chip(
                "access",
                format!("Access: {access}"),
                icon(access_icon, 16.).into_any_element(),
                access.into(),
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let role = picks.role.clone();
        let agent_chip = self.chip_with_menu(
            Menu::Agent,
            chip(
                "agent-role",
                match &role {
                    Some(role) => format!("Agent: {role}"),
                    None => "Agent: none".into(),
                },
                icon("robot", 16.).into_any_element(),
                role.unwrap_or_else(|| "Agent".into()),
                &t,
            ),
            &picks,
            can_compact,
            &t,
            window,
            cx,
        );
        let sigil_menu = self.sigil_menu(picks.agent, &provider_commands, &t, cx);
        let frame = div()
            .id("composer")
            .role(Role::Group)
            .aria_label("Composer")
            .relative()
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(RADIUS_LG))
            .pt(px(14.))
            .pr(px(12.))
            .pb(px(10.))
            .pl(px(12.))
            .border_1()
            .border_color(if focused { t.accent } else { t.border_strong })
            .bg(t.bg_editor)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" && this.sigil_escape(cx) {
                    return cx.stop_propagation();
                }
                let m = &e.keystroke.modifiers;
                if e.keystroke.key == "up" && m.alt && !m.control && !m.shift && !m.platform {
                    this.workspace
                        .update(cx, |w, cx| w.edit_latest_queued(cx))
                        .ok();
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if !editor_focus.is_focused(window) {
                    window.focus(&editor_focus, cx);
                }
            })
            .children(sigil_menu)
            .when(!attachments.is_empty(), |c| {
                c.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .px(px(6.))
                        .children(attachments),
                )
            })
            .child(
                div()
                    .id("composer-editor")
                    .min_h(px(64.))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .px(px(6.))
                    .text_size(px(FS_MD))
                    .line_height(px(20.))
                    .child(self.editor.clone()),
            )
            .child(
                div()
                    .id("composer-controls")
                    .role(Role::Toolbar)
                    .aria_label("Composer controls")
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .child(model_chip)
                    .children(effort_chip)
                    .child(compact_chip)
                    .child(access_chip)
                    .child(agent_chip)
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("formatting")
                            .role(Role::Toolbar)
                            .aria_label("Formatting")
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(format("bold", "Bold", "text-b", "**", "**"))
                            .child(format("italic", "Italic", "text-italic", "*", "*"))
                            .child(format(
                                "strike",
                                "Strikethrough",
                                "text-strikethrough",
                                "~~",
                                "~~",
                            ))
                            .child(format("inline-code", "Inline code", "code", "`", "`"))
                            .child(format(
                                "code-block",
                                "Code block",
                                "code-block",
                                "```\n",
                                "\n```",
                            ))
                            .child(format("bullets", "Bulleted list", "list-bullets", "- ", ""))
                            .child(format(
                                "numbers",
                                "Numbered list",
                                "list-numbers",
                                "1. ",
                                "",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .child(
                                tool_button("attach-files", "Attach files", "paperclip", &t)
                                    .on_click(cx.listener(|this, _, _, cx| this.attach(false, cx))),
                            )
                            .child(
                                tool_button("attach-folder", "Attach folder", "folder", &t)
                                    .on_click(cx.listener(|this, _, _, cx| this.attach(true, cx))),
                            ),
                    )
                    .children(stop)
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .children(send)
                            .children(send_mode),
                    ),
            );
        div()
            .flex()
            .flex_col()
            .children(notice)
            .children(usage)
            .child(frame)
            .into_any_element()
    }
}
