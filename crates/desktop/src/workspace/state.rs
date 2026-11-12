//! The workspace's state: the open chat, the views around it, and the window's regions.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    AnyElement, Entity, FocusHandle, Focusable, ListAlignment, ListState, ScrollHandle,
    Subscription, WeakEntity, Window, div, prelude::*,
};
use patchwork_server::Server;
use patchwork_server::permissions::PermissionKind;
use patchwork_server::projects::{Folder, Project};
use patchwork_server::settings::Settings;
use patchwork_server::threads::{Message, MessageStatus, Part, Thread};
use patchwork_server::workspace::{FileChange, ProjectChanged};
use crate::canvas::Canvas;
use crate::chat::approvals::AllowChoice;
use crate::chat::composer::Composer;
use crate::chat::files::ReplyFiles;
use crate::chat::goal::GoalClear;
use crate::chat::lineage::Lineage;
use crate::chat::plan::ShownPlan;
use crate::chat::prompts::Answering;
use crate::chat::queue::{ChatDrafts, QueuedEdit};
use crate::workspace::add_project::AddProject;
use crate::ui::menu::Menu;
use crate::workspace::palette::Palette;
use crate::sidebar::changes::ChangesView;
use crate::sidebar::menu::{ContextMenu, Renaming, SortUndo};
use super::background_dialog;
use super::granular;
use super::panel_area;

/// The sidebar's views, one per activity bar icon.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Tasks,
    Changes,
    Archived,
}

/// The sidebar's width: its default, and how narrow and wide a drag may make it.
pub const SIDEBAR_WIDTH: f32 = 300.;

pub const SIDEBAR_MIN: f32 = 200.;

pub const SIDEBAR_MAX: f32 = 480.;

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct Workspace {
    pub server: Arc<Server>,
    pub settings: Settings,
    /// Every thread, each with only the messages the task list reads.
    pub threads: Vec<Thread>,
    pub folders: Vec<Folder>,
    /// Projects and folders (`folder:<id>`) whose rows are closed.
    pub collapsed: HashSet<String>,
    /// The archive view's closed groups, its own so closing one leaves the task list as it was.
    pub archived_collapsed: HashSet<String>,
    /// The open chat, in full.
    pub thread: Option<Thread>,
    pub view: Option<View>,
    pub branch: Option<String>,
    /// The project's uncommitted files, as git reports them.
    pub changes: Vec<FileChange>,
    /// The reply whose "Show changes" was clicked: its files lead the Changes view.
    pub change_focus: Option<(String, String)>,
    /// The approval whose "More ways to allow" menu is open, and how each kind of request was last
    /// allowed, which its Allow button offers first.
    pub allow_menu: Option<String>,
    /// The resolved approval whose rule menu is open.
    pub rule_menu: Option<String>,
    pub allow_choices: HashMap<PermissionKind, AllowChoice>,
    /// The answers given to the agent's questions so far, the panel's highlighted row, the replies
    /// whose options were answered or put away, and the one whose options show now.
    pub answering: Option<Answering>,
    pub choice_active: Option<(String, usize)>,
    pub dismissed_choices: HashSet<String>,
    pub shown_choice: Option<String>,
    pub plan_open: bool,
    pub plan_last_running: Option<String>,
    pub shown_plan: Option<ShownPlan>,
    pub plan_fold_queued: Option<String>,
    pub implementing: HashSet<String>,
    pub goal_clear: Option<GoalClear>,
    pub goal_closed: Option<u64>,
    /// A redraw is due in a second, for the elapsed times on screen.
    pub(super) ticking: bool,
    pub queue_open: bool,
    pub queued_edit: Option<QueuedEdit>,
    pub chat_drafts: ChatDrafts,
    pub steer_pressed: bool,
    pub lineage: Lineage,
    pub(super) _prompt_keys: gpui::Subscription,
    pub changes_view: Entity<ChangesView>,
    /// Each reply's files as they stand now, by message id, and a count of the git updates that
    /// makes them load again.
    pub reply_files: HashMap<String, ReplyFiles>,
    pub changes_generation: u64,
    /// Where git's state arrives from the project watcher's thread.
    pub(super) project_events: mpsc::UnboundedSender<ProjectChanged>,
    /// The last error an action hit, shown above the status bar until dismissed.
    pub error: Option<String>,
    /// The file panel right of the chat, its width once dragged, and where a drag of it started.
    pub canvas: Option<Entity<Canvas>>,
    pub(super) canvas_width: Option<f32>,
    pub(super) canvas_resizing: Option<(f32, f32)>,
    /// The mobile app in the open project, the project it was last looked for in, and the
    /// Android panel, which takes the file panel's place while it is open.
    pub mobile: Option<patchwork_server::android::MobileProject>,
    pub(super) mobile_for: Option<String>,
    pub(crate) android: Option<Entity<crate::android::AndroidPanel>>,
    /// The terminal and Output panel under the chat, whether it shows, its height, and the drag
    /// of its handle: where it started and the height then.
    pub(super) panel: Entity<crate::panel::Panel>,
    pub(super) panel_open: bool,
    pub(super) panel_height: f32,
    pub(super) panel_resizing: Option<(f32, f32)>,
    pub composer: Entity<Composer>,
    /// The sidebar and the chat, each drawn as its own view so a scroll in one leaves the other's frame.
    pub(super) sidebar_region: Entity<Region>,
    pub(super) chat_region: Entity<Region>,
    /// The status bar, so its spinner redraws only the bar as it turns, not the whole workspace.
    pub(super) status_region: Entity<Region>,
    pub sidebar_width: f32,
    /// Where a sidebar drag started: the pointer's x and the width then.
    pub(super) resizing: Option<(f32, f32)>,
    /// Replies whose folded steps are open, and steps whose details are.
    /// The project or folder row under the pointer, which shows its buttons.
    pub hovered_row: Option<gpui::SharedString>,
    /// Rows of the task tree picked together with Ctrl or Shift.
    pub picking: crate::sidebar::pick::Picking,
    pub open_folds: HashSet<String>,
    pub open_steps: HashSet<String>,
    /// Runs of subagents listed open, by their first call.
    pub open_agents: HashSet<String>,
    pub find: crate::chat::find::FindState,
    pub rail: crate::chat::rail::RailState,
    pub proposed: crate::chat::proposed::ProposedPlans,
    pub user: crate::chat::user::UserState,
    pub backgrounds: crate::ui::backdrop::Backgrounds,
    pub checks: crate::chat::checks::ChecksState,
    pub secrets: crate::chat::secret::SecretState,
    /// The resume prompt, while it waits for an answer.
    pub resume_ask: Option<crate::chat::resume::ResumeAsk>,
    /// The project folder last looked for, and whether it was found once the look is done.
    pub folder_check: Option<(String, Option<bool>)>,
    /// Replies seen streaming, whose steps start open once they settle.
    pub streamed: HashSet<String>,
    /// The transcript, drawn a screenful at a time, and the chat and items it last held.
    pub transcript_list: ListState,
    pub transcript_rows: (Option<String>, Vec<crate::chat::Row>),
    /// When the open chat was chosen, which its transcript waits on for at most `OPEN_HOLD`.
    pub opened_at: std::time::Instant,
    pub tree_scroll: ScrollHandle,
    pub focus: FocusHandle,
    pub usage: crate::sidebar::usage::UsageState,
    /// A reload of the threads is already on its way.
    pub(super) reload_queued: bool,
    /// Search, while open, and what had focus before it opened.
    pub(super) palette: Option<Entity<Palette>>,
    pub(super) palette_return: Option<FocusHandle>,
    /// Settings, while open.
    pub(super) settings_dialog: Option<Entity<crate::settings::SettingsDialog>>,
    /// The version whose update card the user closed.
    pub(super) update_dismissed: Option<String>,
    /// How many replies the last session cut off wait to be resumed, with Settings set to ask.
    pub(super) interrupted: usize,
    /// The Add a project dialog, while open.
    pub(super) add_project: Option<Entity<AddProject>>,
    pub(super) background_dialog: Option<background_dialog::BackgroundDialog>,
    pub(super) granular_dialog: Option<granular::GranularDialog>,
    /// First-run setup, shown instead of everything else until the settings say it is done.
    pub(super) onboarding: Option<Entity<crate::onboarding::Onboarding>>,
    /// The toolbar's project menu, while open, and where the press that last closed it was.
    pub(super) project_menu: Option<Entity<Menu>>,
    pub(super) project_menu_dismissed: Option<gpui::Point<gpui::Pixels>>,
    /// The task tree's open menu, the row being renamed, and where sorted tasks were.
    pub context_menu: Option<ContextMenu>,
    pub renaming: Option<Renaming>,
    pub sort_undo: HashMap<String, SortUndo>,
    /// The window's title as last set: "Patchwork", then the open chat's title after a dash.
    pub(super) title: String,
}

impl Workspace {
    pub fn new(server: Arc<Server>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = server.store.settings();
        let collapsed = server.collapsed_sections().into_iter().collect();
        let update_dismissed = server.dismissed_update();
        let threads = server.list_threads();
        let folders = server.list_folders();
        let workspace = cx.entity().downgrade();
        let composer = cx.new(|cx| Composer::new(workspace.clone(), window, cx));
        let changes_view = cx.new(|cx| ChangesView::new(workspace, window, cx));
        // The panel above the composer answers its keys before the composer's bindings do.
        let weak = cx.entity().downgrade();
        let prompt_keys = cx.intercept_keystrokes(move |ev, window, cx| {
            let _ = weak.update(cx, |w, cx| w.on_prompt_key(&ev.keystroke, window, cx));
        });
        let (project_events, mut received) = mpsc::unbounded::<ProjectChanged>();
        cx.spawn(async move |this, cx| {
            while let Some(changed) = received.next().await {
                let applied = this.update(cx, |this, cx| this.on_project_changed(changed, cx));
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();
        let entity = cx.entity();
        let sidebar_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, window, cx| match w.view {
                    Some(view) => crate::sidebar::render(w, view, window, cx).into_any_element(),
                    None => gpui::Empty.into_any_element(),
                },
                cx,
            )
        });
        let chat_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, window, cx| crate::chat::render(w, window, cx).into_any_element(),
                cx,
            )
        });
        let status_region =
            cx.new(|cx| Region::new(&entity, |w, _, cx| w.status_bar(cx).into_any_element(), cx));
        let (panel, panel_height) =
            panel_area::new_panel(Arc::clone(&server), &settings, window, cx);
        let mut this = Self {
            server,
            settings,
            threads,
            folders,
            collapsed,
            archived_collapsed: HashSet::new(),
            thread: None,
            view: Some(View::Tasks),
            branch: None,
            changes: Vec::new(),
            change_focus: None,
            allow_menu: None,
            rule_menu: None,
            allow_choices: HashMap::new(),
            answering: None,
            choice_active: None,
            dismissed_choices: HashSet::new(),
            shown_choice: None,
            plan_open: false,
            plan_last_running: None,
            shown_plan: None,
            plan_fold_queued: None,
            implementing: HashSet::new(),
            goal_clear: None,
            goal_closed: None,
            ticking: false,
            queue_open: true,
            queued_edit: None,
            chat_drafts: ChatDrafts::default(),
            steer_pressed: false,
            lineage: Lineage::default(),
            _prompt_keys: prompt_keys,
            changes_view,
            reply_files: HashMap::new(),
            changes_generation: 0,
            project_events,
            error: None,
            canvas: None,
            canvas_width: None,
            canvas_resizing: None,
            mobile: None,
            mobile_for: None,
            android: None,
            panel,
            panel_open: false,
            panel_height,
            panel_resizing: None,
            composer,
            sidebar_region,
            chat_region,
            status_region,
            sidebar_width: SIDEBAR_WIDTH,
            resizing: None,
            hovered_row: None,
            picking: crate::sidebar::pick::Picking::default(),
            open_folds: HashSet::new(),
            open_steps: HashSet::new(),
            open_agents: HashSet::new(),
            resume_ask: None,
            checks: crate::chat::checks::ChecksState::default(),
            secrets: crate::chat::secret::SecretState::default(),
            folder_check: None,
            find: crate::chat::find::FindState::default(),
            rail: crate::chat::rail::RailState::new(cx),
            proposed: crate::chat::proposed::ProposedPlans::default(),
            user: crate::chat::user::UserState::default(),
            backgrounds: crate::ui::backdrop::Backgrounds::default(),
            streamed: HashSet::new(),
            transcript_list: ListState::new(0, ListAlignment::Top, gpui::px(800.)),
            transcript_rows: (None, Vec::new()),
            opened_at: std::time::Instant::now(),
            tree_scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            usage: crate::sidebar::usage::UsageState::new(cx),
            reload_queued: false,
            palette: None,
            palette_return: None,
            settings_dialog: None,
            update_dismissed,
            interrupted: 0,
            add_project: None,
            background_dialog: None,
            granular_dialog: None,
            onboarding: None,
            title: "Patchwork".into(),
            project_menu: None,
            project_menu_dismissed: None,
            context_menu: None,
            renaming: None,
            sort_undo: HashMap::new(),
        };
        let last = this
            .server
            .last_thread_id()
            .filter(|id| this.threads.iter().any(|t| &t.id == id));
        if let Some(id) = last {
            this.open_thread(&id, cx);
        }
        this.watch_rail_focus(window, cx);
        // A focused control that goes away, such as a shell's kill button, hands focus to the
        // nearest place around it still shown, so the app's keys keep working.
        cx.on_focus_lost(window, |this, window, cx| {
            let target = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| this.focus.clone());
            window.focus(&target, cx);
        })
        .detach();
        this.load_git(cx);
        // The composer starts focused, so typing and the app's keys work before any click.
        let composer = this.composer.read(cx).editor.focus_handle(cx);
        window.focus(&composer, cx);
        this
    }

    /// The project the window is about: the open chat's, else the active one.
    pub fn project(&self) -> Option<&Project> {
        let id = match &self.thread {
            Some(thread) => thread.project_id.as_ref(),
            None => self.settings.active_project_id.as_ref(),
        }?;
        self.settings.projects.iter().find(|p| &p.id == id)
    }

    /// The open chat's message with this id.
    pub fn message(&self, id: &str) -> Option<&Message> {
        self.thread.as_ref()?.messages.iter().find(|m| m.id == id)
    }

    /// Draws the window again in a second, for the elapsed times it shows.
    pub fn tick_each_second(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            // Once a second, the finest step the elapsed times on screen show.
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                this.ticking = false;
                cx.notify();
            });
        })
        .detach();
    }
}

pub(super) fn message_mut<'a>(thread: &'a mut Thread, id: &str) -> Option<&'a mut Message> {
    thread.messages.iter_mut().find(|m| m.id == id)
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub(super) fn end_thinking(parts: &mut [Part]) {
    if let Some(Part::Thinking { ended_at, .. }) = parts.last_mut()
        && ended_at.is_none()
    {
        *ended_at = Some(crate::ui::now_ms());
    }
}

/// A reply is streaming.
pub fn is_working(thread: &Thread) -> bool {
    thread
        .messages
        .iter()
        .any(|m| m.status == MessageStatus::Streaming)
}

/// The agent is at work: a reply is streaming, or tasks a finished reply left running still are.
pub fn is_busy(thread: &Thread) -> bool {
    is_working(thread)
        || thread
            .messages
            .iter()
            .any(|m| m.background.iter().any(|task| task.status.is_live()))
}

type Draw = fn(&mut Workspace, &mut Window, &mut Context<Workspace>) -> AnyElement;

/// One part of the workspace drawn as its own view: the sidebar, the chat, the status bar.
///
/// GPUI redraws a view that is notified and reuses the last frame of every other cached one.
/// A scroll notifies the view it happens in, so with the chat in its own region a scroll of the
/// transcript redraws the chat alone, and the sidebar keeps its frame. Every change to the
/// workspace notifies each region, since each draws from the workspace's state.
pub struct Region {
    workspace: WeakEntity<Workspace>,
    draw: Draw,
    _changes: Subscription,
}

impl Region {
    pub fn new(workspace: &Entity<Workspace>, draw: Draw, cx: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.downgrade(),
            draw,
            _changes: cx.observe(workspace, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for Region {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draw = self.draw;
        let body = self.workspace.update(cx, |w, cx| draw(w, window, cx)).ok();
        div().size_full().flex().children(body)
    }
}
