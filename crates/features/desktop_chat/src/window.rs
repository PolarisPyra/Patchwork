//! The chat's state on the window, and what the chat asks of the window that holds it.
//!
//! [`ChatUi`] is the open chat with its transcript view and the strips above the composer. The
//! desktop's window keeps one and derefs to it, and implements [`ChatWindow`] for what only it
//! can do: navigation, the other screens, dialogs and the panels around the chat.

// Rust guideline compliant 2026-11-27

use std::collections::{HashMap, HashSet};
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, Context, Entity, FocusHandle, Pixels, Point, ScrollHandle, Task, Window};
use patchwork_client::AppState;
use patchwork_client::local::core::Core;
use patchwork_client::local::core::agents::Agent;
use patchwork_client::local::core::android::MobileProject;
use patchwork_client::local::core::chats::{Chat, Message, Part};
use patchwork_client::local::core::permissions::{PermissionKind, PermissionScope};
use patchwork_client::local::core::projects::Project;
use patchwork_client::local::core::quilts::QuiltKind;
use patchwork_client::local::core::source_control::CredentialRequest;
use patchwork_client::local::core::terminal::Launch;
use patchwork_client::local::core::worktree::WorktreeMark;
use patchwork_design_icons::IconName;

use crate::ChatView;
use crate::Composer;
use crate::above_composer::agent_questions::Answering;
use crate::above_composer::goal_bar::GoalClear;
use crate::above_composer::plan_steps::ShownPlan;
use crate::above_composer::proposed_plan::ProposedPlans;
use crate::above_composer::queued_messages::{ChatDrafts, QueuedEdit};
use crate::above_composer::secret_request::SecretState;
use crate::above_composer::worktree_status::WorktreeStrip;
use crate::backdrop::Backgrounds;
use crate::panes::{Layout, Node, PaneId};
use crate::top_right::subagent_list::Lineage;
use crate::transcript::reply::approvals::AllowChoice;
use crate::transcript::view_state::TranscriptView;

/// What a running reply's head line adds about it until the reply's next output.
#[derive(Debug)]
pub enum ReplyNotice {
    /// The agent is retrying a model request: why, the attempt, and how many it makes (0 when
    /// unknown).
    Retrying(String, Option<u32>, Option<u32>),
    /// The agent has said nothing since this time (ms since epoch).
    Quiet(u64),
}

/// What a context menu the chat opens is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    /// A pane's tab strip.
    Pane(PaneId),
    /// The tabs of a project, or of chats without one.
    Tabs(Option<String>),
    /// The saved layouts.
    Layouts,
    /// A chat's tab.
    Chat(String),
}

/// The open chat and the chat's view state, kept on the window.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct ChatUi {
    pub core: Arc<Core>,
    /// The agent whose page shows in place of the chat.
    pub agent_shown: Option<String>,
    /// The agent's details are shown beside an agent's chat.
    pub agent_details: bool,
    /// The open chat, in full.
    pub chat: Option<Chat>,
    /// Each project's panes and their tabs, by project; `None` for chats without one.
    pub layouts: HashMap<Option<String>, Layout>,
    /// The last pane number given out.
    pub pane_seq: PaneId,
    /// The layouts as last kept in the app state.
    pub saved_layouts: String,
    /// The chats whose tabs were closed, the latest last, for reopening.
    pub closed_tabs: Vec<String>,
    /// The tab a dragged tab is held over, waiting to show.
    pub spring_tab: Option<String>,
    /// The tab a dragged tab would go before if dropped now.
    pub tab_drop_before: Option<String>,
    /// Whether each message sent also goes to the chat every other pane shows.
    pub broadcasting: bool,
    /// The layouts the user named, in the order they were saved.
    pub named_layouts: Vec<(String, Node)>,
    /// The keyboard handles of the lines between panes, by split path and part.
    pub line_focus: HashMap<(Vec<usize>, usize), FocusHandle>,
    /// The chat closed to start a new one, whose tab the new chat takes.
    pub left_tab: Option<String>,
    /// The next chat opened gets a new tab rather than the open one's.
    pub new_tab: bool,
    /// The chats carried on in their agent's CLI in a terminal, not yet brought back.
    pub in_cli: HashSet<String>,
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
    /// A redraw is due in a second, for the elapsed times on screen.
    pub ticking: bool,
    pub queue_open: bool,
    /// The queued strip's rows, for its scrollbar.
    pub queue_scroll: ScrollHandle,
    pub queued_edit: Option<QueuedEdit>,
    pub chat_drafts: ChatDrafts,
    pub steer_pressed: bool,
    pub lineage: Lineage,
    /// Whether the open chat was opened from an orchestrator card, and shows as a peek.
    pub peeking: bool,
    /// The mobile app in the open project.
    pub mobile: Option<MobileProject>,
    pub composer: Entity<Composer>,
    /// Notices on running replies, by message id, with the chat each is in.
    pub notices: HashMap<String, (String, ReplyNotice)>,
    /// The open chat's transcript as drawn: its scroll, its rows, and what is open in it.
    pub transcript: TranscriptView,
    pub proposed: ProposedPlans,
    pub backgrounds: Backgrounds,
    pub secrets: SecretState,
    /// The project folder last looked for, and whether it was found once the look is done.
    pub folder_check: Option<(String, Option<bool>)>,
    /// While the transcript's width keeps changing, the wait for it to hold.
    pub transcript_settle: Option<Task<()>>,
    /// When the open chat was chosen, which its transcript waits on for at most `OPEN_HOLD`.
    pub opened_at: Instant,
    pub focus: FocusHandle,
    /// A reload of the chats is already on its way.
    pub reload_queued: bool,
}

impl ChatUi {
    /// The chat's state with nothing open, over `core`, sending with `composer`, and handing the
    /// keys back to `focus`.
    pub fn new(
        core: Arc<Core>,
        composer: Entity<Composer>,
        focus: FocusHandle,
        cx: &mut App,
    ) -> Self {
        Self {
            core,
            agent_shown: None,
            agent_details: false,
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
            in_cli: HashSet::new(),
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
            ticking: false,
            queue_open: true,
            queue_scroll: ScrollHandle::new(),
            queued_edit: None,
            chat_drafts: ChatDrafts::default(),
            steer_pressed: false,
            lineage: Lineage::default(),
            peeking: false,
            mobile: None,
            composer,
            notices: HashMap::new(),
            transcript: TranscriptView::new(cx),
            proposed: ProposedPlans::default(),
            backgrounds: Backgrounds::default(),
            secrets: SecretState::default(),
            folder_check: None,
            transcript_settle: None,
            opened_at: Instant::now(),
            focus,
            reload_queued: false,
        }
    }
}

/// The window the chat sits in: it holds the [`ChatUi`], and does what only the window can.
pub trait ChatWindow: Sized + 'static + DerefMut<Target = ChatUi> {
    /// The view that draws the chat's panes.
    fn chat_view(&self) -> &Entity<ChatView<Self>>;

    /// Leaves the orchestrator or Quilts page for the chats.
    fn show_chats_page(&mut self);

    /// Shows the Changes view in the sidebar.
    fn show_changes_view(&mut self);

    /// The icon of a kind of quilt.
    fn quilt_icon(kind: QuiltKind) -> IconName;

    /// The picture of version `n` of quilt `id`, asking for it when it is not taken yet.
    fn quilt_picture(&self, id: &str, n: u32, dark: bool, cx: &mut App) -> Option<PathBuf>;

    /// What a kind of permission is called.
    fn permission_label(kind: PermissionKind) -> &'static str;

    /// What a rule's scope is called, with the projects it can name.
    fn scope_name(scope: &PermissionScope, projects: &[Project]) -> String;

    /// Reads the quilts again, and the Quilts page while it shows.
    fn reload_quilts(&mut self, cx: &mut Context<Self>);

    /// Restarts into the downloaded update.
    fn restart_to_update(&mut self, cx: &mut Context<Self>);

    /// Opens Settings at `section`, moving Settings there when it is open already.
    fn reopen_settings(
        &mut self,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Asks the user for git's sign-in.
    fn ask_credentials(
        &mut self,
        request: CredentialRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Has Settings count the costs again, while it is open.
    fn refresh_costs(&mut self, cx: &mut Context<Self>);

    /// Asks how much the new chat `chat_id` may do on its own.
    fn ask_granular(&mut self, chat_id: String, window: &mut Window, cx: &mut Context<Self>);

    /// Opens the context menu for `target` at `at`.
    fn open_context_menu(
        &mut self,
        target: MenuTarget,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Opens `path` in the file panel, as its diff or its code.
    fn open_file(
        &mut self,
        path: &str,
        message: Option<Message>,
        diff: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Opens `path` under `root` in the file panel, as its diff or its code.
    fn open_file_at(
        &mut self,
        path: &str,
        root: String,
        message: Option<Message>,
        diff: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Whether the Android panel is open.
    fn android_open(&self, cx: &App) -> bool;

    /// Draws the app in the open project's look.
    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>);

    /// Goes back to the orchestrator page the chat was opened from.
    fn back_to_orchestrator(&mut self, cx: &mut Context<Self>);

    /// Whether there is a place to go back to.
    fn can_go_back(&self) -> bool;

    /// The custom agent the open chat runs as.
    fn chat_agent<'a>(&'a self, cx: &'a App) -> Option<&'a Agent>;

    /// Goes back to the place shown before.
    fn go_back(&mut self, cx: &mut Context<Self>);

    /// Reads the open project's git state.
    fn load_git(&mut self, cx: &mut Context<Self>);

    /// Opens a terminal in `cwd`.
    fn new_terminal_in(&mut self, cwd: String, window: &mut Window, cx: &mut Context<Self>);

    /// Draws the change in where updating stands.
    fn on_update_change(&mut self, cx: &mut Context<Self>);

    /// Opens a changed file of the project.
    fn open_change(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>);

    /// Opens chat `id`.
    fn open_chat(&mut self, id: &str, cx: &mut Context<Self>);

    /// Shows version `n` of quilt `id` beside the chat.
    fn open_quilt_beside(
        &mut self,
        id: String,
        n: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Opens Settings at a permission rule.
    fn open_rule(
        &mut self,
        scope: PermissionScope,
        rule_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );

    /// Reads git again, waiting for it when `sync`.
    fn read_git(&mut self, sync: bool, cx: &mut Context<Self>);

    /// Reads the custom agents again.
    fn reload_agents(&mut self, cx: &mut Context<Self>);

    /// Makes project `id` the active one.
    fn remember_project(&mut self, id: Option<String>, cx: &mut App);

    /// Shows chat `id` in the sidebar and opens it.
    fn reveal_chat(&mut self, id: &str, cx: &mut Context<Self>);

    /// Selects project `id`.
    fn select_project(&mut self, id: &str, cx: &mut Context<Self>);

    /// Runs `launch` in a terminal named `name`.
    fn show_in_terminal(&mut self, launch: Launch, name: String, cx: &mut Context<Self>);

    /// Whether the agent's notes show beside the chat.
    fn shows_notes(&self, cx: &App) -> bool;

    /// Shows or hides the panel under the chat as the settings say.
    fn sync_panel(&mut self, cx: &mut Context<Self>);

    /// Shows or hides the agent's details.
    fn toggle_agent_details(&mut self, cx: &mut Context<Self>);

    /// Opens or closes the Android panel.
    fn toggle_android(&mut self, window: &mut Window, cx: &mut Context<Self>);

    /// Whether the chat has its composer: not on a subagent's transcript, nor on a chat looked in
    /// on from the orchestrator page.
    fn composer_shown(&self) -> bool {
        !self.peeking && self.chat.as_ref().is_none_or(|t| t.parent.is_none())
    }

    /// The project the window is about: the open chat's, else the active one.
    fn project<'a>(&'a self, cx: &'a App) -> Option<&'a Project> {
        let id = match &self.chat {
            Some(chat) => chat.project_id.as_ref(),
            None => AppState::get(cx).settings.window.active_project_id.as_ref(),
        }?;
        AppState::get(cx)
            .settings
            .projects
            .iter()
            .find(|p| &p.id == id)
    }

    /// The open chat's message with this id.
    fn message(&self, id: &str) -> Option<&Message> {
        self.chat
            .as_ref()?
            .messages
            .iter()
            .find(|m| m.id == id)
            .map(Arc::as_ref)
    }

    /// Draws the window again in a second, for the elapsed times it shows.
    fn tick_each_second(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        // Once a second, the finest step the elapsed times on screen show.
        patchwork_design_tasks::after(cx, Duration::from_secs(1), |this, cx| {
            this.ticking = false;
            cx.notify();
        });
    }
}

/// The open chat's message `id`, to change.
pub fn message_mut<'a>(chat: &'a mut Chat, id: &str) -> Option<&'a mut Message> {
    chat.messages
        .iter_mut()
        .find(|m| m.id == id)
        .map(Arc::make_mut)
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
pub fn end_thinking(parts: &mut [Part]) {
    if let Some(Part::Thinking { ended_at, .. }) = parts.last_mut()
        && ended_at.is_none()
    {
        *ended_at = Some(patchwork_platform::now_ms());
    }
}

/// A reply is streaming.
#[must_use]
pub fn is_working(chat: &Chat) -> bool {
    chat.is_replying()
}

/// The agent is at work: a reply is streaming, or tasks a finished reply left running still are.
#[must_use]
pub fn is_busy(chat: &Chat) -> bool {
    chat.is_busy()
}
