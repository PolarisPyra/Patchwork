//! The workspace's state: the open chat, the views around it, and the window's regions.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    Task, AnyElement, Entity, FocusHandle, Focusable, ListAlignment, ListState, ScrollHandle,
    Subscription, WeakEntity, Window, div, prelude::*,
};
use patchwork_core::Core;
use patchwork_core::gateway::ChatApps;
use patchwork_core::permissions::PermissionKind;
use patchwork_core::projects::{Folder, Project};
use patchwork_core::settings::Settings;
use patchwork_core::threads::{Message, MessageStatus, Part, Thread, WorktreeMark};
use patchwork_core::vault::{Method, Status};
use patchwork_core::workspace::{FileChange, ProjectChanged};
use crate::canvas::Canvas;
use crate::chat::approvals::AllowChoice;
use crate::chat::composer::Composer;
use crate::chat::files::ReplyFiles;
use crate::chat::goal::GoalClear;
use crate::chat::worktree::WorktreeStrip;
use crate::chat::lineage::Lineage;
use crate::chat::plan::ShownPlan;
use crate::chat::prompts::Answering;
use crate::chat::queue::{ChatDrafts, QueuedEdit};
use crate::orchestrator::{Orchestrator, OrchestratorBackdrop};
use crate::workspace::add_project::AddProject;
use patchwork_ui::menu::Menu;
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

/// Somewhere the back and forward buttons return to.
#[derive(Clone, PartialEq, Eq)]
pub(super) enum Place {
    Orchestrator,
    /// A chat, by its id.
    Chat(String),
    /// A chat opened from an orchestrator card: read-only, with the sidebar closed.
    Peek(String),
    /// A bot's page, by the bot's id.
    Bot(String),
    /// The Quilts page, with the quilt it had open, by its id, or its gallery.
    Quilts(Option<String>),
}

/// What fills the window beside the activity bar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The sidebar, the open chat, and the panels around it.
    Chats,
    Orchestrator,
    /// What agents made to look at, drawn live.
    Quilts,
}

/// The sidebar's width: its default, and how narrow and wide a drag may make it.
pub const SIDEBAR_WIDTH: f32 = 300.;

pub const SIDEBAR_MIN: f32 = 200.;

pub const SIDEBAR_MAX: f32 = 480.;

/// What a running reply's head line adds about it until the reply's next output.
pub enum ReplyNotice {
    /// The agent is retrying a model request: why, the attempt, and how many it makes (0 when
    /// unknown).
    Retrying(String, Option<u32>, Option<u32>),
    /// The agent has said nothing since this time (ms since epoch).
    Quiet(u64),
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct Workspace {
    pub core: Arc<Core>,
    pub settings: Settings,
    /// Every thread, each with only the messages the task list reads.
    pub threads: Vec<Arc<Thread>>,
    pub folders: Vec<Folder>,
    /// Every bot, oldest first.
    pub bots: Vec<patchwork_core::bots::Bot>,
    /// The bot whose page shows in place of the chat, and its page once drawn.
    pub bot_shown: Option<String>,
    pub bot_page: Option<Entity<crate::bots::BotPage>>,
    /// The bot's details are shown beside a bot's chat.
    pub bot_details: bool,
    /// The chat apps each thread came from or has on it now, by thread id.
    pub chat_apps: HashMap<String, ChatApps>,
    /// Where each chat app's connection stands; one that is not listed is off.
    pub chat_app_status: Vec<(
        patchwork_core::gateway::Platform,
        patchwork_core::gateway::Status,
    )>,
    /// Projects and folders (`folder:<id>`) whose rows are closed.
    pub collapsed: HashSet<String>,
    /// The archive view's closed groups, its own so closing one leaves the task list as it was.
    pub archived_collapsed: HashSet<String>,
    /// The open chat, in full.
    pub thread: Option<Thread>,
    pub view: Option<View>,
    pub page: Page,
    /// Whether the orchestrator page has the task list beside it. Its own, so opening or closing
    /// it leaves the chats' sidebar as it was.
    pub(super) orchestrator_tasks: bool,
    pub(super) orchestrator: Entity<Orchestrator>,
    /// The orchestrator page's backdrop, drawn under it as a view of its own.
    pub(super) orchestrator_backdrop: Entity<OrchestratorBackdrop>,
    /// The Quilts page while it shows. Closed with it, so its browser runs only then.
    pub(super) quilts: Option<Entity<crate::quilts::QuiltsPage>>,
    /// The quilt the Quilts page has open, by its id, which it opens on again when it is
    /// made.
    pub(super) quilt_open: Option<String>,
    /// The quilt shown beside a chat, with that chat's id; it shows while the chat does.
    pub(super) quilt_beside: Option<(String, Entity<crate::quilts::QuiltsPage>)>,
    /// Whether the chat a quilt came from shows in a card over the Quilts page.
    pub(super) quilt_chat: bool,
    /// Every quilt, so a chat's quilt cards can name theirs and open it.
    pub(crate) quilt_list: Vec<patchwork_core::quilts::Quilt>,
    /// The quilts' pictures, for the chats' cards and the gallery.
    pub(crate) thumbs: Entity<crate::quilts::Thumbs>,
    pub branch: Option<String>,
    /// The folder Source Control last read: the open task's worktree, or the project's folder.
    pub git_root: Option<String>,
    /// Counts the reads of git's state, so only the newest one is applied.
    pub(super) git_seq: u64,
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
    pub goal_clear: Option<GoalClear>,
    pub goal_closed: Option<u64>,
    /// The open task's worktree and the strip that offers merging it.
    pub worktree: Option<WorktreeStrip>,
    /// The strips of tasks switched away from while a step ran or had something to say, by task
    /// id, so coming back shows it.
    pub parked_worktrees: HashMap<String, WorktreeStrip>,
    /// Whether the user was told where each project's tasks work, by project id; None while read.
    pub worktree_told: HashMap<String, Option<bool>>,
    /// What each task's worktree needs from the user, as the sidebar marks it.
    pub worktree_marks: HashMap<String, WorktreeMark>,
    /// The tasks as they were when the marks were last read, so a change reads them again.
    pub worktree_marks_key: Option<u64>,
    /// The task row under the mouse, which shows its archive button in place of its time.
    pub hovered_row: Option<String>,
    /// A redraw is due in a second, for the elapsed times on screen.
    pub(super) ticking: bool,
    pub queue_open: bool,
    /// The queued strip's rows, for its scrollbar.
    pub queue_scroll: ScrollHandle,
    pub queued_edit: Option<QueuedEdit>,
    pub chat_drafts: ChatDrafts,
    pub steer_pressed: bool,
    pub lineage: Lineage,
    /// What filling the window was last saved as, to open again at the next launch.
    pub(super) saved_place: Option<patchwork_core::threads::LastPlace>,
    /// What was shown before and after what is shown now, newest last, for going back and
    /// forward.
    pub(super) history_back: Vec<Place>,
    pub(super) history_forward: Vec<Place>,
    /// Whether the open chat was opened from an orchestrator card, and shows as [`Place::Peek`].
    pub peeking: bool,
    pub(super) _prompt_keys: gpui::Subscription,
    pub changes_view: Entity<ChangesView>,
    /// The Devices view, made the first time it is shown.
    pub devices_view: Option<Entity<crate::sidebar::devices::DevicesView>>,
    /// Each reply's files as they stand now, by message id, and a count of the git updates that
    /// makes them load again.
    pub reply_files: HashMap<String, ReplyFiles>,
    pub changes_generation: u64,
    /// Where git's state arrives from the project watcher's thread.
    pub(super) project_events: mpsc::UnboundedSender<ProjectChanged>,
    /// The last error an action hit, shown above the status bar until dismissed.
    pub error: Option<String>,
    /// The error the toast's timer runs for and that timer, whether the toast is sliding away,
    /// and whether the mouse is over it, which holds it.
    pub(super) error_timer: Option<(String, Task<()>)>,
    pub(super) error_leaving: bool,
    pub(super) error_hovered: bool,
    /// The file panel right of the chat, its width once dragged, and where a drag of it started.
    pub canvas: Option<Entity<Canvas>>,
    pub(super) canvas_width: Option<f32>,
    pub(super) canvas_resizing: Option<(f32, f32)>,
    /// The mobile app in the open project, the project it was last looked for in, and the
    /// Android panel, which takes the file panel's place while it is open.
    pub mobile: Option<patchwork_core::android::MobileProject>,
    pub(super) mobile_for: Option<String>,
    pub(crate) android: Option<Entity<crate::android::AndroidPanel>>,
    /// The terminal and Output panel under the chat, whether it shows, its height, and the drag
    /// of its handle: where it started and the height then.
    pub(super) panel: Entity<patchwork_sidecar::panel::Panel>,
    pub(super) panel_open: bool,
    pub(super) panel_height: f32,
    pub(super) panel_resizing: Option<(f32, f32)>,
    pub composer: Entity<Composer>,
    /// The sidebar and the chat, each drawn as its own view so a scroll in one leaves the other's frame.
    pub(super) sidebar_region: Entity<Region>,
    pub(super) chat_region: Entity<Region>,
    /// The chat's background, under the chat region, so a moving backdrop leaves the chat's frame.
    pub(super) backdrop_region: Entity<Region>,
    /// The Quilts page's background, under the page, as `backdrop_region` is under the chat.
    pub(super) quilts_backdrop_region: Entity<Region>,
    /// The status bar, so its spinner redraws only the bar as it turns, not the whole workspace.
    pub(super) status_region: Entity<Region>,
    pub sidebar_width: f32,
    /// Where a sidebar drag started: the pointer's x and the width then.
    pub(super) resizing: Option<(f32, f32)>,
    /// Rows of the task tree picked together with Ctrl or Shift.
    pub picking: crate::sidebar::pick::Picking,
    /// Replies whose folded steps are open, and steps whose details are.
    pub open_folds: HashSet<String>,
    /// Notices on running replies, by message id, with the thread each is in.
    pub notices: HashMap<String, (String, ReplyNotice)>,
    pub open_steps: HashSet<String>,
    /// Runs of subagents listed open, by their first call.
    pub open_agents: HashSet<String>,
    /// Review cards turned from how they first show, by reply.
    pub open_reviews: HashSet<String>,
    pub find: crate::chat::find::FindState,
    pub rail: crate::chat::rail::RailState,
    pub proposed: crate::chat::proposed::ProposedPlans,
    pub user: crate::chat::user::UserState,
    pub backgrounds: crate::chat::backdrop::Backgrounds,
    pub checks: crate::chat::checks::ChecksState,
    pub secrets: crate::chat::secret::SecretState,
    /// The project folder last looked for, and whether it was found once the look is done.
    pub folder_check: Option<(String, Option<bool>)>,
    /// Replies seen streaming, whose steps start open once they settle.
    pub streamed: HashSet<String>,
    /// The transcript, drawn a screenful at a time, and the chat and items it last held.
    pub transcript_list: ListState,
    /// The transcript's width last laid out, and, while it keeps changing, the wait for it to hold.
    pub transcript_width: gpui::Pixels,
    pub transcript_settle: Option<Task<()>>,
    /// The wait for the clock or the sun to change the theme.
    pub(super) theme_switch: Option<Task<()>>,
    /// The project whose look the app is drawn in.
    pub(super) themed_project: Option<String>,
    pub transcript_rows: (Option<String>, Vec<crate::chat::Row>),
    /// The pieces each shown message was last cut into, so a frame cuts only what changed.
    pub transcript_pieces: Vec<crate::chat::Pieces>,
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
    pub(super) settings_dialog: Option<Entity<patchwork_settings::SettingsDialog>>,
    /// The version whose update card the user closed.
    pub(super) update_dismissed: Option<String>,
    /// How many replies the last session cut off wait to be resumed, with Settings set to ask.
    pub(super) interrupted: usize,
    /// The Add a project dialog, while open.
    pub(super) add_project: Option<Entity<AddProject>>,
    pub(super) background_dialog: Option<background_dialog::BackgroundDialog>,
    pub(super) granular_dialog: Option<granular::GranularDialog>,
    /// Git's sign-in questions, while a command waits on one.
    pub(super) credentials: super::credential::Credentials,
    /// First-run setup, shown instead of everything else until the settings say it is done.
    pub(super) onboarding: Option<Entity<crate::onboarding::Onboarding>>,
    /// The toolbar's machine menu, while open, and where the press that last closed it was.
    pub(super) machine_menu: Option<Entity<Menu>>,
    pub(super) machine_menu_dismissed: Option<gpui::Point<gpui::Pixels>>,
    /// The task tree's open menu, the row being renamed, and where sorted tasks were.
    pub context_menu: Option<ContextMenu>,
    pub renaming: Option<Renaming>,
    pub sort_undo: HashMap<String, SortUndo>,
    /// The window's title as last set: the app's name, then the open chat's title after a dash.
    pub(super) title: String,
}

impl Workspace {
    pub fn new(core: Arc<Core>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = core.settings();
        let collapsed = core.collapsed_sections().into_iter().collect();
        let update_dismissed = core.dismissed_update();
        let threads = core.list_threads();
        let chat_apps = core.thread_chat_apps();
        let chat_app_status = core.gateway_status();
        let folders = core.list_folders();
        let bots = core.bots();
        let workspace = cx.entity().downgrade();
        let composer = cx.new(|cx| Composer::new(workspace.clone(), window, cx));
        let changes_view = cx.new(|cx| ChangesView::new(workspace, window, cx));
        // The panel above the composer answers its keys before the composer's bindings do.
        let weak = cx.entity().downgrade();
        let prompt_keys = cx.intercept_keystrokes(move |ev, window, cx| {
            let _ = weak.update(cx, |w, cx| w.on_prompt_key(&ev.keystroke, window, cx));
        });
        let project_events = Self::listen_for_project_changes(cx);
        let entity = cx.entity();
        let sidebar_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, _, cx| match w.sidebar_view() {
                    Some(view) => crate::sidebar::render(w, view, cx).into_any_element(),
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
        let backdrop_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, _, cx| crate::chat::backdrop::backdrop_layer(w, cx),
                cx,
            )
        });
        let quilts_backdrop_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, _, cx| crate::chat::backdrop::quilts_backdrop_layer(w, cx),
                cx,
            )
        });
        let orchestrator = cx.new(|cx| Orchestrator::new(&entity, composer.clone(), window, cx));
        let orchestrator_backdrop = cx.new(|cx| OrchestratorBackdrop::new(&orchestrator, cx));
        let thumbs = cx.new(|_| crate::quilts::Thumbs::new(Arc::clone(&core)));
        cx.observe(&thumbs, |_, _, cx| cx.notify()).detach();
        let status_region =
            cx.new(|cx| Region::new(&entity, |w, _, cx| w.status_bar(cx).into_any_element(), cx));
        let (panel, panel_height) = panel_area::new_panel(Arc::clone(&core), &settings, window, cx);
        let themed_project = settings.active_project_id.clone();
        let mut this = Self {
            core,
            settings,
            threads,
            folders,
            bots,
            bot_shown: None,
            bot_page: None,
            bot_details: false,
            collapsed,
            chat_apps,
            chat_app_status,
            archived_collapsed: HashSet::new(),
            thread: None,
            view: Some(View::Tasks),
            page: Page::Orchestrator,
            orchestrator_tasks: true,
            orchestrator,
            orchestrator_backdrop,
            quilts: None,
            quilt_open: None,
            quilt_beside: None,
            quilt_chat: false,
            quilt_list: Vec::new(),
            thumbs,
            branch: None,
            git_root: None,
            git_seq: 0,
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
            goal_clear: None,
            goal_closed: None,
            worktree: None,
            parked_worktrees: HashMap::new(),
            worktree_told: HashMap::new(),
            worktree_marks: HashMap::new(),
            worktree_marks_key: None,
            hovered_row: None,
            ticking: false,
            queue_open: true,
            queue_scroll: ScrollHandle::new(),
            queued_edit: None,
            chat_drafts: ChatDrafts::default(),
            steer_pressed: false,
            lineage: Lineage::default(),
            saved_place: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            peeking: false,
            _prompt_keys: prompt_keys,
            changes_view,
            devices_view: None,
            reply_files: HashMap::new(),
            changes_generation: 0,
            project_events,
            error: None,
            error_timer: None,
            error_leaving: false,
            error_hovered: false,
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
            backdrop_region,
            quilts_backdrop_region,
            status_region,
            sidebar_width: SIDEBAR_WIDTH,
            resizing: None,
            picking: crate::sidebar::pick::Picking::default(),
            open_folds: HashSet::new(),
            notices: HashMap::new(),
            open_steps: HashSet::new(),
            open_agents: HashSet::new(),
            open_reviews: HashSet::new(),
            checks: crate::chat::checks::ChecksState::default(),
            secrets: crate::chat::secret::SecretState::default(),
            folder_check: None,
            find: crate::chat::find::FindState::default(),
            rail: crate::chat::rail::RailState::new(cx),
            proposed: crate::chat::proposed::ProposedPlans::default(),
            user: crate::chat::user::UserState::default(),
            backgrounds: crate::chat::backdrop::Backgrounds::default(),
            streamed: HashSet::new(),
            // Every row is measured up front, or the scrollbar thumb jumps as rows scroll in.
            transcript_list: ListState::new(0, ListAlignment::Top, gpui::px(800.)).measure_all(),
            transcript_width: gpui::px(0.),
            transcript_settle: None,
            theme_switch: None,
            themed_project,
            transcript_rows: (None, Vec::new()),
            transcript_pieces: Vec::new(),
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
            credentials: super::credential::Credentials::default(),
            onboarding: None,
            title: patchwork_ui::APP_TITLE.into(),
            machine_menu: None,
            machine_menu_dismissed: None,
            context_menu: None,
            renaming: None,
            sort_undo: HashMap::new(),
        };
        let last = this
            .core
            .last_thread_id()
            .filter(|id| this.threads.iter().any(|t| &t.id == id));
        if let Some(id) = last {
            this.open_thread(&id, cx);
        }
        this.restore_place(cx);
        // Getting the window ready is not a step to go back from.
        this.history_back.clear();
        this.watch_rail_focus(window, cx);
        this.watch_themes(window, cx);
        this.await_theme_switch(window, cx);
        this.watch_window(window, cx);
        this.load_git(cx);
        this.load_quilt_list(cx);
        // The composer starts focused, so typing and the app's keys work before any click.
        let composer = this.composer.read(cx).editor.focus_handle(cx);
        window.focus(&composer, cx);
        // A vault with a password opens nothing saved until it is entered, and one without a
        // keychain to keep its key needs a password set.
        if this.settings.onboarded
            && matches!(
                patchwork_core::vault::status(),
                Status::Waiting | Status::Unset | Status::Open(Method::File)
            )
        {
            this.open_settings(Some("security"), window, cx);
        }
        this
    }

    /// The sender of the core's project changes, which this view applies as they arrive.
    fn listen_for_project_changes(cx: &mut Context<Self>) -> mpsc::UnboundedSender<ProjectChanged> {
        let (sender, mut received) = mpsc::unbounded::<ProjectChanged>();
        cx.spawn(async move |this, cx| {
            while let Some(changed) = received.next().await {
                let applied = this.update(cx, |this, cx| this.on_project_changed(changed, cx));
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();
        sender
    }

    /// Follows the window: its focus, its appearance, and its coming back to the front.
    fn watch_window(&self, window: &mut Window, cx: &mut Context<Self>) {
        // A branch merged or a worktree changed outside the app shows once the window is back,
        // and within a minute while it waits. Source Control reads git again too, for changes
        // the file watcher missed, such as on a network drive or past the system's watch limit.
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.reload_worktree_marks(cx);
                this.read_git(false, cx);
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                if this.update(cx, Self::reload_worktree_marks).is_err() {
                    break;
                }
            }
        })
        .detach();
        // A focused control that goes away, such as a shell's kill button, hands focus to the
        // nearest place around it still shown, so the app's keys keep working.
        cx.on_focus_lost(window, |this, window, cx| {
            let target = window
                .focus_lost_restore_target(cx)
                .unwrap_or_else(|| this.focus.clone());
            window.focus(&target, cx);
        })
        .detach();
        // A key hides the mouse pointer only while something has the focus to type in. With
        // nothing focused, Space opens and closes the sidebar and the pointer stays.
        let focus = self.focus.clone();
        // Following the system, the app changes palette when the system does.
        cx.observe_window_appearance(window, |_, window, cx| {
            patchwork_ui::theme::follow(window.appearance(), cx);
        })
        .detach();
        cx.on_focus(&focus, window, |_, _, cx| {
            cx.set_cursor_hide_mode(gpui::CursorHideMode::Never);
        })
        .detach();
        cx.on_blur(&focus, window, |_, _, cx| {
            cx.set_cursor_hide_mode(gpui::CursorHideMode::default());
        })
        .detach();
    }

    /// What the sidebar shows, if it is open: the chats' view, or beside the orchestrator page
    /// the task list that page opens and closes for itself.
    pub(super) fn sidebar_view(&self) -> Option<View> {
        match self.page {
            Page::Chats => self.view.filter(|_| !self.peeking),
            Page::Orchestrator => self.orchestrator_tasks.then_some(View::Tasks),
            Page::Quilts => None,
        }
    }

    /// Whether the chat has its composer: not on a subagent's transcript, nor on a chat looked in
    /// on from the orchestrator page.
    pub fn composer_shown(&self) -> bool {
        !self.peeking && self.thread.as_ref().is_none_or(|t| t.parent.is_none())
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
        self.thread
            .as_ref()?
            .messages
            .iter()
            .find(|m| m.id == id)
            .map(Arc::as_ref)
    }

    /// Draws the window again in a second, for the elapsed times it shows.
    pub fn tick_each_second(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        // Once a second, the finest step the elapsed times on screen show.
        patchwork_ui::after(cx, Duration::from_secs(1), |this, cx| {
            this.ticking = false;
            cx.notify();
        });
    }
}

pub(super) fn message_mut<'a>(thread: &'a mut Thread, id: &str) -> Option<&'a mut Message> {
    thread
        .messages
        .iter_mut()
        .find(|m| m.id == id)
        .map(Arc::make_mut)
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub(super) fn end_thinking(parts: &mut [Part]) {
    if let Some(Part::Thinking { ended_at, .. }) = parts.last_mut()
        && ended_at.is_none()
    {
        *ended_at = Some(patchwork_ui::now_ms());
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
