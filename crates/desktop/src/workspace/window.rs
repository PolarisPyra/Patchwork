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
use patchwork_core::chats::{Message, MessageStatus, Part, Chat};
use patchwork_core::worktree::WorktreeMark;
use patchwork_core::vault::{Method, Status};
use patchwork_core::{source_control::FileChange, projects::ProjectChanged};
use crate::canvas::Canvas;
use crate::chat::transcript::reply::approvals::AllowChoice;
use crate::chat::composer::Composer;
use crate::chat::transcript::reply::changed_files::ReplyFiles;
use crate::chat::above_composer::goal_bar::GoalClear;
use crate::chat::above_composer::worktree_status::WorktreeStrip;
use crate::chat::top_right::subagent_list::Lineage;
use crate::chat::above_composer::plan_steps::ShownPlan;
use crate::chat::above_composer::agent_questions::Answering;
use crate::chat::above_composer::queued_messages::{ChatDrafts, QueuedEdit};
use crate::orchestrator::{Orchestrator, OrchestratorBackdrop};
use patchwork_modals::add_project::AddProject;
use patchwork_ui::controls::menu::Menu;
use patchwork_modals::search::Palette;
use crate::sidebar::source_control::ChangesView;
use crate::sidebar::tasks::row_menu::{ContextMenu, Renaming, SortUndo};
use crate::workspace::hosts::background;
use crate::workspace::dock::place;

/// The sidebar's views, one per activity bar icon.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Tasks,
    Changes,
    Issues,
    Archived,
}

/// Somewhere the back and forward buttons return to.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Orchestrator,
    /// A chat, by its id.
    Chat(String),
    /// A chat opened from an orchestrator card: read-only, with the sidebar closed.
    Peek(String),
    /// An agent's page, by the agent's id.
    Agent(String),
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
    /// Every chat, each with only the messages the task list reads.
    pub chats: Vec<Arc<Chat>>,
    pub folders: Vec<Folder>,
    /// Every agent, oldest first.
    pub agents: Vec<patchwork_core::agents::Agent>,
    /// The agent whose page shows in place of the chat, and its page once drawn.
    pub agent_shown: Option<String>,
    pub agent_page: Option<Entity<crate::agents::AgentPage>>,
    /// The agent's details are shown beside an agent's chat.
    pub agent_details: bool,
    /// The chat apps each chat came from or has on it now, by chat id.
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
    pub chat: Option<Chat>,
    /// Each project's panes and their tabs, by project; `None` for chats without one.
    pub(crate) layouts: HashMap<Option<String>, crate::chat::panes::Layout>,
    /// The last pane number given out.
    pub(crate) pane_seq: crate::chat::panes::PaneId,
    /// The layouts as last kept in the app state.
    pub(crate) saved_layouts: String,
    /// The chats whose tabs were closed, the latest last, for reopening.
    pub(crate) closed_tabs: Vec<String>,
    /// The tab a dragged tab is held over, waiting to show.
    pub(crate) spring_tab: Option<String>,
    /// The tab a dragged tab would go before if dropped now.
    pub(crate) tab_drop_before: Option<String>,
    /// Whether each message sent also goes to the chat every other pane shows.
    pub(crate) broadcasting: bool,
    /// The layouts the user named, in the order they were saved.
    pub(crate) named_layouts: Vec<(String, crate::chat::panes::Node)>,
    /// The keyboard handles of the lines between panes, by split path and part.
    pub(crate) line_focus: HashMap<(Vec<usize>, usize), gpui::FocusHandle>,
    /// The chat closed to start a new one, whose tab the new chat takes.
    pub(crate) left_tab: Option<String>,
    /// The next chat opened gets a new tab rather than the open one's.
    pub(crate) new_tab: bool,
    /// The Issues view's lists.
    pub(crate) issues: crate::sidebar::issues::IssueLists,
    /// The chats carried on in their agent's CLI in a terminal, not yet brought back.
    pub(crate) in_cli: std::collections::HashSet<String>,
    pub view: Option<View>,
    pub page: Page,
    /// Whether the orchestrator page has the task list beside it. Its own, so opening or closing
    /// it leaves the chats' sidebar as it was.
    pub(crate) orchestrator_tasks: bool,
    pub(crate) orchestrator: Entity<Orchestrator>,
    /// The orchestrator page's backdrop, drawn under it as a view of its own.
    pub(crate) orchestrator_backdrop: Entity<OrchestratorBackdrop>,
    /// The Quilts page while it shows. Closed with it, so its browser runs only then.
    pub(crate) quilts: Option<Entity<patchwork_pages::quilts::QuiltsPage>>,
    /// The quilt the Quilts page has open, by its id, which it opens on again when it is
    /// made.
    pub(crate) quilt_open: Option<String>,
    /// The quilt shown beside a chat, with that chat's id; it shows while the chat does.
    pub(crate) quilt_beside: Option<(String, Entity<patchwork_pages::quilts::QuiltsPage>)>,
    /// Whether the chat a quilt came from shows in a card over the Quilts page.
    pub(crate) quilt_chat: bool,
    /// Every quilt, so a chat's quilt cards can name theirs and open it.
    pub(crate) quilt_list: Vec<patchwork_core::quilts::Quilt>,
    /// The quilts' pictures, for the chats' cards and the gallery.
    pub(crate) thumbs: Entity<patchwork_pages::quilts::Thumbs>,
    pub branch: Option<String>,
    /// The folder Source Control last read: the open task's worktree, or the project's folder.
    pub git_root: Option<String>,
    /// Counts the reads of git's state, so only the newest one is applied.
    pub(crate) git_seq: u64,
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
    pub(crate) ticking: bool,
    pub queue_open: bool,
    /// The queued strip's rows, for its scrollbar.
    pub queue_scroll: ScrollHandle,
    pub queued_edit: Option<QueuedEdit>,
    pub chat_drafts: ChatDrafts,
    pub steer_pressed: bool,
    pub lineage: Lineage,
    /// What filling the window was last saved as, to open again at the next launch.
    pub(crate) saved_place: Option<patchwork_core::chats::LastPlace>,
    /// What was shown before and after what is shown now, newest last, for going back and
    /// forward.
    pub(crate) history_back: Vec<Place>,
    pub(crate) history_forward: Vec<Place>,
    /// Whether the open chat was opened from an orchestrator card, and shows as [`Place::Peek`].
    pub peeking: bool,
    pub(crate) _prompt_keys: gpui::Subscription,
    pub changes_view: Entity<ChangesView>,
    /// The Devices view, made the first time it is shown.
    pub devices_view: Option<Entity<crate::sidebar::projects::machines::DevicesView>>,
    /// Each reply's files as they stand now, by message id, and a count of the git updates that
    /// makes them load again.
    pub reply_files: HashMap<String, ReplyFiles>,
    pub changes_generation: u64,
    /// Where git's state arrives from the project watcher's chat.
    pub(crate) project_events: mpsc::UnboundedSender<ProjectChanged>,
    /// The last error an action hit, shown above the status bar until dismissed.
    pub error: Option<String>,
    /// The error the toast's timer runs for and that timer, whether the toast is sliding away,
    /// and whether the mouse is over it, which holds it.
    pub(crate) error_timer: Option<(String, Task<()>)>,
    pub(crate) error_leaving: bool,
    pub(crate) error_hovered: bool,
    /// The file panel right of the chat, its width once dragged, and where a drag of it started.
    pub canvas: Option<Entity<Canvas>>,
    pub(crate) canvas_width: Option<f32>,
    pub(crate) canvas_resizing: Option<(f32, f32)>,
    /// The mobile app in the open project, the project it was last looked for in, and the
    /// Android panel, which takes the file panel's place while it is open.
    pub mobile: Option<patchwork_core::android::MobileProject>,
    pub(crate) mobile_for: Option<String>,
    pub(crate) android: Option<Entity<patchwork_pages::android::AndroidPanel>>,
    /// The terminal and Output panel under the chat, whether it shows, its height, and the drag
    /// of its handle: where it started and the height then.
    pub(crate) dock: Entity<crate::workspace::dock::Dock>,
    pub(crate) panel_open: bool,
    pub(crate) panel_height: f32,
    pub(crate) panel_resizing: Option<(f32, f32)>,
    pub composer: Entity<Composer>,
    /// The sidebar and the chat, each drawn as its own view so a scroll in one leaves the other's frame.
    pub(crate) sidebar_region: Entity<Region>,
    pub(crate) chat_view: Entity<crate::chat::ChatView>,
    /// The chat's background, under the chat region, so a moving backdrop leaves the chat's frame.
    pub(crate) backdrop_region: Entity<Region>,
    /// The Quilts page's background, under the page, as `backdrop_region` is under the chat.
    pub(crate) quilts_backdrop_region: Entity<Region>,
    /// The status bar, so its spinner redraws only the bar as it turns, not the whole workspace.
    pub(crate) status_region: Entity<Region>,
    pub sidebar_width: f32,
    /// Where a sidebar drag started: the pointer's x and the width then.
    pub(crate) resizing: Option<(f32, f32)>,
    /// Rows of the task tree picked together with Ctrl or Shift.
    pub picking: crate::sidebar::tasks::multi_select::Picking,
    /// Replies whose folded steps are open, and steps whose details are.
    pub open_folds: HashSet<String>,
    /// Notices on running replies, by message id, with the chat each is in.
    pub notices: HashMap<String, (String, ReplyNotice)>,
    pub open_steps: HashSet<String>,
    /// Runs of subagents listed open, by their first call.
    pub open_agents: HashSet<String>,
    /// Review cards turned from how they first show, by reply.
    pub open_reviews: HashSet<String>,
    pub rail: crate::chat::transcript::turn_rail::RailState,
    pub proposed: crate::chat::above_composer::proposed_plan::ProposedPlans,
    pub user: crate::chat::transcript::user_message::UserState,
    pub backgrounds: crate::chat::backdrop::Backgrounds,
    pub checks: crate::chat::transcript::reply::checks::ChecksState,
    pub secrets: crate::chat::above_composer::secret_request::SecretState,
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
    pub(crate) theme_switch: Option<Task<()>>,
    /// The project whose look the app is drawn in.
    pub(crate) themed_project: Option<String>,
    pub transcript_rows: (Option<String>, Vec<crate::chat::Row>),
    /// The pieces each shown message was last cut into, so a frame cuts only what changed.
    pub transcript_pieces: Vec<crate::chat::Pieces>,
    /// When the open chat was chosen, which its transcript waits on for at most `OPEN_HOLD`.
    pub opened_at: std::time::Instant,
    pub tree_scroll: ScrollHandle,
    pub focus: FocusHandle,
    pub usage: crate::sidebar::usage::UsageState,
    /// A reload of the chats is already on its way.
    pub(crate) reload_queued: bool,
    /// Search, while open, and what had focus before it opened.
    pub(crate) palette: Option<Entity<Palette>>,
    pub(crate) palette_return: Option<FocusHandle>,
    /// Settings, while open.
    pub(crate) settings_dialog: Option<Entity<patchwork_modals::settings::SettingsDialog>>,
    /// The version whose update card the user closed.
    pub(crate) update_dismissed: Option<String>,
    /// How many replies the last session cut off wait to be resumed, with Settings set to ask.
    pub(crate) interrupted: Entity<patchwork_modals::interrupted::InterruptedCard>,
    /// The Add a project dialog, while open.
    pub(crate) add_project: Option<Entity<AddProject>>,
    /// A chat's or an agent's background picker, while open, with whose background it picks.
    pub(crate) background_dialog: Option<(
        background::Owner,
        Entity<patchwork_modals::background::BackgroundDialog>,
    )>,
    pub(crate) granular_dialog: Option<Entity<patchwork_modals::granular::GranularDialog>>,
    /// Git's sign-in questions, shown while a command waits on one.
    pub(crate) credentials: Entity<patchwork_modals::credential::CredentialDialog>,
    /// First-run setup, shown instead of everything else until the settings say it is done.
    pub(crate) onboarding: Option<Entity<patchwork_pages::onboarding::Onboarding>>,
    /// The toolbar's machine menu, while open, and where the press that last closed it was.
    pub(crate) machine_menu: Option<Entity<Menu>>,
    pub(crate) machine_menu_dismissed: Option<gpui::Point<gpui::Pixels>>,
    /// The task tree's open menu, the row being renamed, and where sorted tasks were.
    pub context_menu: Option<ContextMenu>,
    pub renaming: Option<Renaming>,
    pub sort_undo: HashMap<String, SortUndo>,
    /// The window's title as last set: the app's name, then the open chat's title after a dash.
    pub(crate) title: String,
}

impl Workspace {
    pub fn new(core: Arc<Core>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = core.settings();
        let collapsed = core.collapsed_sections().into_iter().collect();
        let update_dismissed = core.dismissed_update();
        let chats = core.list_chats();
        let chat_apps = core.chat_chat_apps();
        let chat_app_status = core.gateway_status();
        let folders = core.list_folders();
        let agents = core.agents();
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
        let home_focus = cx.focus_handle();
        let chat_view = cx.new(|cx| crate::chat::ChatView::new(&entity, home_focus.clone(), cx));
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
        let thumbs = cx.new(|_| patchwork_pages::quilts::Thumbs::new(Arc::clone(&core)));
        cx.observe(&thumbs, |_, _, cx| cx.notify()).detach();
        let status_region =
            cx.new(|cx| Region::new(&entity, |w, _, cx| w.status_bar(cx).into_any_element(), cx));
        let (dock, panel_height) = place::new_panel(Arc::clone(&core), &settings, window, cx);
        let themed_project = settings.active_project_id.clone();
        let credentials = cx.new(|_| patchwork_modals::credential::CredentialDialog::default());
        cx.subscribe_in(
            &credentials,
            window,
            |this, _, _: &patchwork_modals::credential::Answered, window, cx| {
                window.focus(&this.focus, cx);
            },
        )
        .detach();
        cx.observe(&credentials, |_, _, cx| cx.notify()).detach();
        let interrupted = cx.new(|_| patchwork_modals::interrupted::InterruptedCard::default());
        cx.subscribe(
            &interrupted,
            |this, _, _: &patchwork_modals::interrupted::Resume, cx| {
                let core = Arc::clone(&this.core);
                let task = cx.background_spawn(async move { core.resume_interrupted() });
                cx.spawn(async move |this, cx| {
                    task.await;
                    this.update(cx, Workspace::reload)
                })
                .detach();
            },
        )
        .detach();
        cx.observe(&interrupted, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            core,
            settings,
            chats,
            folders,
            agents,
            agent_shown: None,
            agent_page: None,
            agent_details: false,
            collapsed,
            chat_apps,
            chat_app_status,
            archived_collapsed: HashSet::new(),
            chat: None,
            layouts: HashMap::new(),
            pane_seq: 0,
            saved_layouts: String::new(),
            closed_tabs: Vec::new(),
            spring_tab: None,
            tab_drop_before: None,
            broadcasting: false,
            named_layouts: Vec::new(),
            line_focus: HashMap::new(),
            left_tab: None,
            new_tab: false,
            issues: crate::sidebar::issues::IssueLists::default(),
            in_cli: std::collections::HashSet::new(),
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
            dock,
            panel_open: false,
            panel_height,
            panel_resizing: None,
            composer,
            sidebar_region,
            chat_view,
            backdrop_region,
            quilts_backdrop_region,
            status_region,
            sidebar_width: SIDEBAR_WIDTH,
            resizing: None,
            picking: crate::sidebar::tasks::multi_select::Picking::default(),
            open_folds: HashSet::new(),
            notices: HashMap::new(),
            open_steps: HashSet::new(),
            open_agents: HashSet::new(),
            open_reviews: HashSet::new(),
            checks: crate::chat::transcript::reply::checks::ChecksState::default(),
            secrets: crate::chat::above_composer::secret_request::SecretState::default(),
            folder_check: None,
            rail: crate::chat::transcript::turn_rail::RailState::new(cx),
            proposed: crate::chat::above_composer::proposed_plan::ProposedPlans::default(),
            user: crate::chat::transcript::user_message::UserState::default(),
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
            focus: home_focus,
            usage: crate::sidebar::usage::UsageState::new(cx),
            reload_queued: false,
            palette: None,
            palette_return: None,
            settings_dialog: None,
            update_dismissed,
            interrupted,
            add_project: None,
            background_dialog: None,
            granular_dialog: None,
            credentials,
            onboarding: None,
            title: patchwork_ui::APP_TITLE.into(),
            machine_menu: None,
            machine_menu_dismissed: None,
            context_menu: None,
            renaming: None,
            sort_undo: HashMap::new(),
        };
        this.restore_layouts();
        let last = this
            .core
            .last_chat_id()
            .filter(|id| this.chats.iter().any(|t| &t.id == id));
        if let Some(id) = last {
            this.open_chat(&id, cx);
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
        this.preload_usage(cx);
        this.watch_cli_updates(cx);
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
    pub(crate) fn sidebar_view(&self) -> Option<View> {
        match self.page {
            Page::Chats => self.view.filter(|_| !self.peeking),
            Page::Orchestrator => self.orchestrator_tasks.then_some(View::Tasks),
            Page::Quilts => None,
        }
    }

    /// Whether the chat has its composer: not on a subagent's transcript, nor on a chat looked in
    /// on from the orchestrator page.
    pub fn composer_shown(&self) -> bool {
        !self.peeking && self.chat.as_ref().is_none_or(|t| t.parent.is_none())
    }

    /// The project the window is about: the open chat's, else the active one.
    pub fn project(&self) -> Option<&Project> {
        let id = match &self.chat {
            Some(chat) => chat.project_id.as_ref(),
            None => self.settings.active_project_id.as_ref(),
        }?;
        self.settings.projects.iter().find(|p| &p.id == id)
    }

    /// The open chat's message with this id.
    pub fn message(&self, id: &str) -> Option<&Message> {
        self.chat
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

pub(crate) fn message_mut<'a>(chat: &'a mut Chat, id: &str) -> Option<&'a mut Message> {
    chat.messages
        .iter_mut()
        .find(|m| m.id == id)
        .map(Arc::make_mut)
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub(crate) fn end_thinking(parts: &mut [Part]) {
    if let Some(Part::Thinking { ended_at, .. }) = parts.last_mut()
        && ended_at.is_none()
    {
        *ended_at = Some(patchwork_ui::now_ms());
    }
}

/// A reply is streaming.
pub fn is_working(chat: &Chat) -> bool {
    chat.messages
        .iter()
        .any(|m| m.status == MessageStatus::Streaming)
}

/// The agent is at work: a reply is streaming, or tasks a finished reply left running still are.
pub fn is_busy(chat: &Chat) -> bool {
    is_working(chat)
        || chat
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
