//! The window: the toolbar, the activity bar, the sidebar, the chat and the status bar, laid out as
//! `features/workspace/Workspace.tsx` lays them out, over the state they all read.

mod background_dialog;
mod bars;
mod onboarding;
mod panel_area;
mod region;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    Context, Entity, FocusHandle, Focusable, FollowMode, IntoElement, ListAlignment, ListState,
    MouseButton, MouseMoveEvent, MouseUpEvent, Render, Role, ScrollHandle, StyleRefinement, Window,
    deferred, div, prelude::*, px, relative,
};
use patchwork_server::Server;
use patchwork_server::orchestration::{AgentEvent, AgentEventEnvelope};
use patchwork_server::permissions::{PermissionKind, PermissionScope};
use patchwork_server::projects::{Folder, Project};
use patchwork_server::settings::Settings;
use patchwork_server::threads::{Attachment, Message, MessageStatus, NewThread, Part, Thread};
use patchwork_server::workspace::{FileChange, ProjectChanged};

use crate::canvas::{CANVAS_MAX_FRACTION, CANVAS_MIN, Canvas, CanvasTab};
use crate::chat::approvals::AllowChoice;
use crate::chat::composer::Composer;
use crate::chat::files::ReplyFiles;
use crate::chat::goal::GoalClear;
use crate::chat::lineage::Lineage;
use crate::chat::plan::ShownPlan;
use crate::chat::prompts::Answering;
use crate::chat::queue::QueuedEdit;
use crate::add_project::{AddProject, Closed as AddProjectClosed};
use crate::menu::Menu;
use crate::palette::{Action, Closed, Palette};
use crate::sidebar::changes::ChangesView;
use crate::sidebar::menu::{ContextMenu, Renaming, SortUndo};
use crate::theme::{ACTIVITY_BAR_W, FONT_UI, FS_MD, RADIUS, RADIUS_SM, STATUSBAR_H, Theme, theme};
use crate::ui::{button, display_title, icon};
use region::Region;

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
    ticking: bool,
    pub queue_open: bool,
    pub queued_edit: Option<QueuedEdit>,
    pub steer_pressed: bool,
    pub lineage: Lineage,
    _prompt_keys: gpui::Subscription,
    pub changes_view: Entity<ChangesView>,
    /// Each reply's files as they stand now, by message id, and a count of the git updates that
    /// makes them load again.
    pub reply_files: HashMap<String, ReplyFiles>,
    pub changes_generation: u64,
    /// Where git's state arrives from the project watcher's thread.
    project_events: mpsc::UnboundedSender<ProjectChanged>,
    /// The last error an action hit, shown above the status bar until dismissed.
    pub error: Option<String>,
    /// The file panel right of the chat, its width once dragged, and where a drag of it started.
    pub canvas: Option<Entity<Canvas>>,
    canvas_width: Option<f32>,
    canvas_resizing: Option<(f32, f32)>,
    /// The terminal and Output panel under the chat, whether it shows, its height, and the drag
    /// of its handle: where it started and the height then.
    panel: Entity<crate::panel::Panel>,
    panel_open: bool,
    panel_height: f32,
    panel_resizing: Option<(f32, f32)>,
    pub composer: Entity<Composer>,
    /// The sidebar and the chat, each drawn as its own view so a scroll in one leaves the other's frame.
    sidebar_region: Entity<Region>,
    chat_region: Entity<Region>,
    /// The status bar, so its spinner redraws only the bar as it turns, not the whole workspace.
    status_region: Entity<Region>,
    pub sidebar_width: f32,
    /// Where a sidebar drag started: the pointer's x and the width then.
    resizing: Option<(f32, f32)>,
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
    pub backgrounds: crate::chat::backdrop::Backgrounds,
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
    pub tree_scroll: ScrollHandle,
    pub focus: FocusHandle,
    pub usage: crate::sidebar::usage::UsageState,
    /// A reload of the threads is already on its way.
    reload_queued: bool,
    /// Search, while open, and what had focus before it opened.
    palette: Option<Entity<Palette>>,
    palette_return: Option<FocusHandle>,
    /// Settings, while open.
    settings_dialog: Option<Entity<crate::settings::SettingsDialog>>,
    /// The Add a project dialog, while open.
    add_project: Option<Entity<AddProject>>,
    background_dialog: Option<background_dialog::BackgroundDialog>,
    /// First-run setup, shown instead of everything else until the settings say it is done.
    onboarding: Option<Entity<crate::onboarding::Onboarding>>,
    /// The toolbar's project menu, while open, and where the press that last closed it was.
    project_menu: Option<Entity<Menu>>,
    project_menu_dismissed: Option<gpui::Point<gpui::Pixels>>,
    /// The task tree's open menu, the row being renamed, and where sorted tasks were.
    pub context_menu: Option<ContextMenu>,
    pub renaming: Option<Renaming>,
    pub sort_undo: HashMap<String, SortUndo>,
    /// The window's title as last set: "Patchwork", then the open chat's title after a dash.
    title: String,
}

impl Workspace {
    pub fn new(server: Arc<Server>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = server.store.settings();
        let collapsed = server.collapsed_sections().into_iter().collect();
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
        let (panel, panel_height) = panel_area::new_panel(server.clone(), &settings, window, cx);
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
            picking: Default::default(),
            open_folds: HashSet::new(),
            open_steps: HashSet::new(),
            open_agents: HashSet::new(),
            resume_ask: None,
            checks: Default::default(),
            secrets: Default::default(),
            folder_check: None,
            find: Default::default(),
            rail: crate::chat::rail::RailState::new(cx),
            proposed: Default::default(),
            user: Default::default(),
            backgrounds: Default::default(),
            streamed: HashSet::new(),
            transcript_list: ListState::new(0, ListAlignment::Top, gpui::px(800.)),
            transcript_rows: (None, Vec::new()),
            tree_scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            usage: crate::sidebar::usage::UsageState::new(cx),
            reload_queued: false,
            palette: None,
            palette_return: None,
            settings_dialog: None,
            add_project: None,
            background_dialog: None,
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

    /// Draws the window again in a second, for the elapsed times it shows.
    pub fn tick_each_second(&mut self, cx: &mut Context<Self>) {
        if self.ticking {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                this.ticking = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn open_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.server.thread(id) {
            Ok(thread) => {
                let changed_project =
                    self.thread.as_ref().map(|t| &t.project_id) != Some(&thread.project_id);
                // Leaving a chat with its options still open counts as putting them away.
                if self.thread.as_ref().map(|t| t.id.as_str()) != Some(id)
                    && let Some(shown) = self.shown_choice.take()
                {
                    self.dismissed_choices.insert(shown);
                }
                self.remember_project(thread.project_id.clone());
                self.thread = Some(thread);
                self.open_folds.clear();
                self.open_steps.clear();
                self.open_agents.clear();
                if let Err(e) = self.server.set_last_thread_id(id) {
                    log::warn!("could not record the open chat: {e}");
                }
                self.transcript_list.set_follow_mode(FollowMode::Tail);
                if changed_project {
                    self.load_git(cx);
                }
            }
            Err(e) => log::warn!("could not open chat {id}: {e}"),
        }
        cx.notify();
    }

    /// Reads the project's branch and changed files off the main thread, and watches the project
    /// so they stay current whoever changes its files: an agent, an editor, or git in a terminal.
    pub fn load_git(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project() else {
            self.branch = None;
            self.changes.clear();
            self.changes_view
                .update(cx, |v, cx| v.refresh(None, true, cx));
            return;
        };
        let (id, root) = (project.id.clone(), project.path.clone());
        let events = self.project_events.clone();
        if let Err(e) = self.server.watch_project(&id, move |changed| {
            let _ = events.unbounded_send(changed);
        }) {
            self.error = Some(e);
        }
        let task = cx.background_spawn(async move {
            let branch = patchwork_server::workspace::git_branch(&root);
            let changes = patchwork_server::workspace::git_changes(&root).unwrap_or_default();
            (branch, changes)
        });
        cx.spawn(async move |this, cx| {
            let (branch, changes) = task.await;
            this.update(cx, |this, cx| {
                if this.project().map(|p| &p.id) != Some(&id) {
                    return;
                }
                this.branch = branch;
                this.changes = changes;
                this.changes_view
                    .update(cx, |v, cx| v.refresh(Some(id), true, cx));
                cx.notify();
            })
        })
        .detach();
    }

    /// Git's state after a settled burst of changes in the watched project.
    fn on_project_changed(&mut self, changed: ProjectChanged, cx: &mut Context<Self>) {
        if self.project().map(|p| &p.id) != Some(&changed.project_id) {
            return;
        }
        if let Some(canvas) = &self.canvas {
            let paths = changed.paths.clone();
            canvas.update(cx, |c, cx| c.on_paths(&paths, cx));
        }
        let Some(changes) = changed.changes else {
            return;
        };
        let had_changes = !self.changes.is_empty();
        let git = changed.branch != self.branch
            || had_changes != !changes.is_empty()
            || changed.paths.iter().any(|p| p.starts_with(".git/"));
        self.branch = changed.branch;
        self.changes = changes;
        self.changes_generation += 1;
        let id = changed.project_id;
        self.changes_view
            .update(cx, |v, cx| v.refresh(Some(id), git, cx));
        cx.notify();
    }

    /// Opens a changed file's diff, or the file itself when it has no changes.
    pub fn open_change(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let tab = if self.changes.iter().any(|c| c.path == path) {
            CanvasTab::Diff
        } else {
            CanvasTab::Code
        };
        self.open_canvas(path, None, tab, window, cx);
    }

    /// Opens `path` in the file panel, on `tab`, with the reply it came from.
    pub fn open_canvas(
        &mut self,
        path: &str,
        message: Option<Message>,
        tab: CanvasTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.project() else {
            return;
        };
        let (id, root) = (project.id.clone(), project.path.clone());
        let workspace = cx.entity().downgrade();
        let path = Some(path.to_string());
        self.canvas =
            Some(cx.new(|cx| Canvas::new(workspace, id, root, path, message, tab, window, cx)));
        cx.notify();
    }

    pub fn close_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvas = None;
        cx.notify();
    }

    /// The review notes go to the open thread when it works in their project, else to a new task.
    pub fn send_review(&mut self, project: &str, text: String, cx: &mut Context<Self>) {
        let open = self
            .thread
            .as_ref()
            .filter(|t| t.project_id.as_deref() == Some(project))
            .map(|t| t.id.clone());
        match open {
            Some(id) => self.send_to(&id, text, cx),
            None => self.start_task("Review", text, cx),
        }
    }

    /// The file panel's width: the dragged one, else `max(420px, 46%)` of the row it sits in,
    /// kept between its narrowest and 70% of the row.
    fn canvas_width(&self, window: &Window) -> (f32, f32) {
        let sidebar = if self.view.is_some() {
            self.sidebar_width
        } else {
            0.
        };
        let body = f32::from(window.viewport_size().width) - ACTIVITY_BAR_W - sidebar;
        let max = body * CANVAS_MAX_FRACTION;
        let width = self.canvas_width.unwrap_or((body * 0.46).max(420.));
        (width.min(max).max(CANVAS_MIN), max)
    }

    /// The file panel and the handle that resizes it, while its project is the open one.
    fn canvas_area(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let canvas = self.canvas.clone()?;
        if Some(&canvas.read(cx).project_id) != self.project().map(|p| &p.id) {
            return None;
        }
        let t = theme(cx);
        let (width, max) = self.canvas_width(window);
        let line = t.border;
        let handle = div()
            .id("canvas-resize")
            .role(Role::Splitter)
            .aria_label("Resize file panel")
            .aria_orientation(gpui::Orientation::Vertical)
            .aria_numeric_value(width.round() as f64)
            .aria_min_numeric_value(CANVAS_MIN as f64)
            .aria_max_numeric_value(max.round() as f64)
            .tab_index(0)
            .relative()
            .w(px(4.))
            .flex_none()
            .cursor_col_resize()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(1.5))
                    .w(px(1.))
                    .bg(line),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                    this.canvas_resizing = Some((f32::from(e.position.x), width));
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, e: &gpui::KeyDownEvent, _, cx| {
                let step = match e.keystroke.key.as_str() {
                    "left" => 16.,
                    "right" => -16.,
                    _ => return,
                };
                this.canvas_width = Some((width + step).min(max).max(CANVAS_MIN));
                cx.stop_propagation();
                cx.notify();
            }));
        Some(
            div()
                .flex()
                .flex_none()
                .min_h_0()
                .child(handle)
                .child(div().flex().w(px(width)).min_h_0().child(canvas)),
        )
    }

    /// The last error an action hit, above the status bar until dismissed (`ErrorToast.tsx`).
    fn error_toast(&self, t: &Theme, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let error = self.error.clone()?;
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            deferred(
                div()
                    .id("error-toast")
                    .absolute()
                    .right(px(16.))
                    .bottom(px(STATUSBAR_H + 12.))
                    .max_w(px(420.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(t.danger)
                    .bg(t.bg_elevated)
                    .py(px(8.))
                    .pr(px(8.))
                    .pl(px(12.))
                    .text_size(px(crate::theme::FS_SM))
                    .shadow(crate::ui::popup_shadow(t))
                    .role(Role::Alert)
                    .child(div().min_w_0().child(error))
                    .child(
                        button("dismiss-error", "Dismiss")
                            .size(px(28.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("x", 14.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = None;
                                cx.notify();
                            })),
                    ),
            )
            .with_priority(1),
        )
    }

    /// Opens `id` and sends it `text`, as the pull request's feedback does.
    pub fn send_to(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        self.reveal_thread(id, cx);
        let server = Arc::clone(&self.server);
        let id = id.to_string();
        let task =
            cx.background_spawn(async move { server.send_message(id, text, Vec::new(), true) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Starts a task in the open project with `text` its first message, named `title`.
    pub fn start_task(&mut self, title: &str, text: String, cx: &mut Context<Self>) {
        let Some(project) = self.project().map(|p| p.id.clone()) else {
            return;
        };
        let new = self
            .composer
            .read(cx)
            .new_thread(Some(project), &self.settings);
        self.start_thread(new, title, text, cx);
    }

    /// Creates `new`, named `title`, opens it, and sends `text` as its first message.
    pub fn start_thread(
        &mut self,
        new: NewThread,
        title: &str,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let thread = match self.server.create_thread(new) {
            Ok(thread) => thread,
            Err(e) => {
                self.error = Some(e.to_string());
                return cx.notify();
            }
        };
        if let Err(e) = self.server.rename_thread(&thread.id, title) {
            log::warn!("could not name the thread: {e}");
        }
        self.reload(cx);
        self.reveal_thread(&thread.id, cx);
        let server = Arc::clone(&self.server);
        let id = thread.id;
        let task =
            cx.background_spawn(async move { server.send_message(id, text, Vec::new(), false) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    this.error = Some(e.to_string());
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Applies one event from an agent. Text streams in place; anything else reloads what the
    /// window shows from the store, which saved it before sending it.
    pub fn on_event(&mut self, envelope: AgentEventEnvelope, cx: &mut Context<Self>) {
        let open = self.thread.as_mut().filter(|t| t.id == envelope.thread_id);
        match envelope.event {
            AgentEvent::TextDelta { message_id, text } => {
                if let Some(message) = open.and_then(|t| message_mut(t, &message_id)) {
                    end_thinking(&mut message.parts);
                    message.text.push_str(&text);
                    match message.parts.last_mut() {
                        Some(Part::Text { text: last }) => last.push_str(&text),
                        _ => message.parts.push(Part::Text { text }),
                    }
                    cx.notify();
                }
            }
            AgentEvent::Thinking {
                message_id,
                text,
                started_at,
                ended_at,
            } => {
                if let Some(message) = open.and_then(|t| message_mut(t, &message_id)) {
                    match message.parts.last_mut() {
                        Some(Part::Thinking {
                            text: last,
                            ended_at: None,
                            ..
                        }) => last.push_str(&text),
                        _ => message.parts.push(Part::Thinking {
                            text,
                            started_at: started_at.unwrap_or_else(crate::ui::now_ms),
                            ended_at,
                        }),
                    }
                    cx.notify();
                }
            }
            _ => self.queue_reload(cx),
        }
    }

    /// Reloads the threads and the open chat once, however many events asked for it this frame.
    fn queue_reload(&mut self, cx: &mut Context<Self>) {
        if self.reload_queued {
            return;
        }
        self.reload_queued = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            this.update(cx, |this, cx| {
                this.reload_queued = false;
                this.reload(cx);
            })
        })
        .detach();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.threads = self.server.list_threads();
        self.folders = self.server.list_folders();
        if let Some(id) = self.thread.as_ref().map(|t| t.id.clone()) {
            match self.server.thread(&id) {
                Ok(thread) => self.thread = Some(thread),
                Err(_) => self.thread = None,
            }
        }
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_string());
        }
        self.save_collapsed();
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        for project in &self.settings.projects {
            self.collapsed.insert(project.id.clone());
        }
        self.save_collapsed();
        cx.notify();
    }

    pub fn save_collapsed(&self) {
        let ids: Vec<String> = self.collapsed.iter().cloned().collect();
        if let Err(e) = self.server.set_collapsed_sections(&ids) {
            log::warn!("could not save the open sections: {e}");
        }
    }

    pub fn archive(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Err(e) = self.server.set_thread_archived(id, true) {
            log::warn!("could not archive {id}: {e}");
        }
        if self.thread.as_ref().is_some_and(|t| t.id == id) {
            self.thread = None;
        }
        self.reload(cx);
    }

    /// Sends the composer's text to the open chat, or starts `new` with it, in the role of
    /// `role`, when none is open.
    pub fn send(
        &mut self,
        text: String,
        attachments: Vec<Attachment>,
        new: Option<NewThread>,
        role: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.send_with(text, attachments, new, role, false, cx);
    }

    /// `send`, holding the message in the chat's queue for after the running turn when `queue`
    /// is set, rather than steering the turn with it.
    pub fn send_with(
        &mut self,
        text: String,
        attachments: Vec<Attachment>,
        new: Option<NewThread>,
        role: Option<String>,
        queue: bool,
        cx: &mut Context<Self>,
    ) {
        let id = match (&self.thread, new) {
            (Some(thread), _) => thread.id.clone(),
            (None, None) => return,
            (None, Some(new)) => match self.server.create_thread(new) {
                Ok(thread) => {
                    let id = thread.id.clone();
                    if role.is_some()
                        && let Err(e) = self.server.set_thread_role(&id, role)
                    {
                        log::warn!("could not set the chat's agent: {e}");
                    }
                    self.thread = Some(thread);
                    id
                }
                Err(e) => {
                    log::warn!("could not start a chat: {e}");
                    return;
                }
            },
        };
        let server = Arc::clone(&self.server);
        let task =
            cx.background_spawn(async move { server.send_message(id, text, attachments, queue) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    log::warn!("could not send the message: {e}");
                }
                this.reload(cx);
                this.transcript_list.set_follow_mode(FollowMode::Tail);
            })
        })
        .detach();
    }

    /// Sends `command` to the open chat's agent, as the compact picker's Compact now does.
    pub fn send_command(&mut self, command: &str, cx: &mut Context<Self>) {
        let Some(id) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        let server = Arc::clone(&self.server);
        let command = command.to_string();
        let task =
            cx.background_spawn(async move { server.send_message(id, command, Vec::new(), false) });
        cx.spawn(async move |this, cx| {
            let sent = task.await;
            this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    log::warn!("could not send the command: {e}");
                }
                this.reload(cx);
            })
        })
        .detach();
    }

    /// Starts a chat from the open one's history up to `message_id`, and opens it.
    pub fn fork(&mut self, thread_id: &str, message_id: &str, cx: &mut Context<Self>) {
        match self.server.fork_thread(thread_id, message_id) {
            Ok(thread) => {
                self.reload(cx);
                self.open_thread(&thread.id, cx);
            }
            Err(e) => log::warn!("could not fork the chat: {e}"),
        }
    }

    /// Clears the chat pane so the next message starts a new chat.
    pub fn new_chat(&mut self, cx: &mut Context<Self>) {
        self.thread = None;
        cx.notify();
    }

    /// Starts a chat (no project) or a task in `project`, at the top of `folder` when given, shows
    /// it in the task list, opens it, and puts the cursor in the composer.
    pub fn new_thread_in(
        &mut self,
        project: Option<String>,
        folder: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new = self
            .composer
            .read(cx)
            .new_thread(project.clone(), &self.settings);
        let thread = match self.server.create_thread(new) {
            Ok(thread) => thread,
            Err(e) => return log::warn!("could not start a chat: {e}"),
        };
        if let Some(folder) = &folder {
            let ids: Vec<String> = std::iter::once(thread.id.clone())
                .chain(
                    self.threads
                        .iter()
                        .filter(|t| t.folder_id.as_ref() == Some(folder) && t.archived_at.is_none())
                        .map(|t| t.id.clone()),
                )
                .collect();
            if let Err(e) = self.server.reorder_threads(&ids, Some(folder)) {
                log::warn!("could not put the task in its folder: {e}");
            }
        }
        self.remember_project(project);
        self.reload(cx);
        self.reveal_thread(&thread.id, cx);
        let editor = self.composer.read(cx).editor.clone();
        window.focus(&editor.read(cx).focus_handle(cx), cx);
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(thread) = &self.thread {
            self.server.cancel_turn(&thread.id);
        }
        cx.notify();
    }

    fn on_resize(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel_resizing.is_some() {
            return self.drag_panel(f32::from(event.position.y), window, cx);
        }
        if let Some((start_x, start_width)) = self.canvas_resizing {
            let width = start_width - (f32::from(event.position.x) - start_x);
            self.canvas_width = Some(width);
            return cx.notify();
        }
        if let Some((start_x, start_width)) = self.resizing {
            let width = start_width + f32::from(event.position.x) - start_x;
            self.sidebar_width = width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
            cx.notify();
        }
    }
}

fn message_mut<'a>(thread: &'a mut Thread, id: &str) -> Option<&'a mut Message> {
    thread.messages.iter_mut().find(|m| m.id == id)
}

/// Ends the reasoning block still streaming, if any, as the next step starts.
fn end_thinking(parts: &mut [Part]) {
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

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.settings.onboarded {
            return self.onboarding_view(window, cx);
        }
        let title = match &self.thread {
            Some(thread) => format!("Patchwork - {}", display_title(&thread.title)),
            None => "Patchwork".into(),
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        let t = *theme(cx);
        let sidebar = self.view.map(|_| {
            let width = px(self.sidebar_width);
            self.sidebar_region
                .clone()
                .cached(StyleRefinement::default().flex_none().w(width))
        });
        let canvas = self.canvas_area(window, cx);
        let panel = self.panel_area(window, cx);
        let background_dialog = self.background_dialog(window, cx);
        let error = self.error_toast(&t, cx);
        div()
            .id("workspace")
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_action(|_: &crate::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::FocusPrev, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|this, _: &crate::TogglePalette, window, cx| {
                this.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::NewChat, window, cx| {
                let project = this.project().map(|p| p.id.clone());
                this.new_thread_in(project, None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &crate::FindInChat, window, cx| {
                if this.thread.is_some() {
                    this.open_find(window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &crate::OpenSettings, window, cx| {
                if this.settings_dialog.is_none() {
                    this.open_settings(None, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &crate::ShowTasks, _, cx| {
                this.view = Some(View::Tasks);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowChanges, _, cx| {
                this.view = Some(View::Changes);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ShowArchived, _, cx| {
                this.view = Some(View::Archived);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::ToggleTerminal, window, cx| {
                this.toggle_terminal(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::NewTerminal, window, cx| {
                    this.new_terminal(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::TogglePanel, window, cx| {
                    this.toggle_panel(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &crate::ToggleOutput, window, cx| {
                this.toggle_output(window, cx)
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg_app)
            .font_family(FONT_UI)
            .text_size(px(FS_MD))
            .line_height(relative(1.5))
            .text_color(t.text)
            .on_mouse_move(cx.listener(Self::on_resize))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if this.panel_resizing.take().is_some() {
                        this.keep_panel_height();
                        cx.notify();
                    }
                    if this.resizing.take().is_some() || this.canvas_resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.activity_bar(cx))
                    .children(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .min_h_0()
                                    .child(self.chat_region.clone().cached(
                                        StyleRefinement::default().flex_1().min_w_0().min_h_0(),
                                    ))
                                    .children(canvas),
                            )
                            .children(panel),
                    ),
            )
            .children(error)
            .child(
                self.status_region.clone().cached(
                    StyleRefinement::default()
                        .flex_none()
                        .w_full()
                        .h(px(STATUSBAR_H)),
                ),
            )
            .children(self.palette.clone().map(|p| deferred(p).with_priority(2)))
            .children(
                self.settings_dialog
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(
                self.add_project
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(background_dialog)
            .children(
                self.project_menu
                    .as_ref()
                    .map(|m| Menu::view(m, bars::project_menu_at())),
            )
            .into_any_element()
    }
}

impl Workspace {
    /// Starts dragging the sidebar's edge from `x`.
    pub fn start_resize(&mut self, x: f32) {
        self.resizing = Some((x, self.sidebar_width));
    }

    /// Opens search, or closes it when it is open.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            return self.close_palette(None, window, cx);
        }
        self.palette_return = window.focused(cx);
        let items = crate::palette::items(self);
        let palette = cx.new(|cx| Palette::new(items, window, cx));
        cx.subscribe_in(&palette, window, |this, _, closed: &Closed, window, cx| {
            this.close_palette(closed.0.clone(), window, cx)
        })
        .detach();
        self.palette = Some(palette);
        cx.notify();
    }

    /// Opens Settings → Permissions on `scope`, with `rule_id` picked out.
    pub fn open_rule(
        &mut self,
        scope: PermissionScope,
        rule_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(Some("permissions"), window, cx);
        if let Some(dialog) = &self.settings_dialog {
            dialog.update(cx, |d, _| d.focus_rule(scope, rule_id));
        }
    }

    /// Opens Settings on the section `section`, by id.
    pub fn open_settings(
        &mut self,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_dialog.is_some() {
            return;
        }
        let server = Arc::clone(&self.server);
        let dialog = cx.new(|cx| crate::settings::SettingsDialog::new(server, section, window, cx));
        cx.subscribe_in(&dialog, window, |this, _, event, window, cx| match event {
            crate::settings::SettingsEvent::Changed => {
                this.settings = this.server.store.settings();
                // "Run setup again" leaves Settings for the setup.
                if !this.settings.onboarded {
                    this.settings_dialog = None;
                }
                // A picture added or removed in Settings changes the ones a chat can show.
                this.backgrounds = Default::default();
                this.sync_panel(cx);
                cx.set_global(crate::theme::Theme::for_setting(
                    this.settings.theme,
                    window.appearance(),
                ));
                cx.refresh_windows();
            }
            crate::settings::SettingsEvent::ThreadsChanged => this.reload(cx),
            crate::settings::SettingsEvent::Closed => {
                this.settings_dialog = None;
                window.focus(&this.focus, cx);
                cx.notify();
            }
        })
        .detach();
        self.settings_dialog = Some(dialog);
        cx.notify();
    }

    fn close_palette(
        &mut self,
        action: Option<Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette = None;
        match self.palette_return.take() {
            Some(focus) => window.focus(&focus, cx),
            None => window.focus(&self.focus, cx),
        }
        match action {
            Some(Action::NewChat) => self.new_thread_in(None, None, window, cx),
            Some(Action::NewTask(id)) => self.new_thread_in(Some(id), None, window, cx),
            Some(Action::NewWorktreeTask(id)) => self.new_task_in_worktree(id, window, cx),
            Some(Action::RunChecks(id)) => self.rerun_checks(id, cx),
            Some(Action::ChatBackground(id)) => self.open_background_dialog(&id, window, cx),
            Some(Action::UseLocalModel) => self.use_local_model(cx),
            Some(Action::ToggleTheme) => self.toggle_theme(window, cx),
            Some(Action::OpenSettings) => self.open_settings(None, window, cx),
            Some(Action::OpenFolder) => self.open_folder(window, cx),
            Some(Action::OpenThread(id)) => self.reveal_thread(&id, cx),
            Some(Action::OpenProject(id)) => self.select_project(&id, cx),
            None => {}
        }
        cx.notify();
    }

    /// Makes `id` the project new tasks go in.
    fn remember_project(&mut self, id: Option<String>) {
        if self.settings.active_project_id == id {
            return;
        }
        self.settings.active_project_id = id.clone();
        if let Err(e) = self
            .server
            .store
            .update_settings(|s| s.active_project_id = id)
        {
            log::warn!("could not record the active project: {e}");
        }
    }

    /// Opens the project's newest chat, or an empty one in it when it has none.
    pub fn select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.remember_project(Some(id.to_string()));
        let first = self
            .threads
            .iter()
            .find(|t| {
                t.project_id.as_deref() == Some(id) && t.archived_at.is_none() && t.parent.is_none()
            })
            .map(|t| t.id.clone());
        match first {
            Some(thread) => self.open_thread(&thread, cx),
            None => self.new_chat(cx),
        }
    }

    /// Shows the thread in the task list, opening its project and folder, and opens it.
    fn reveal_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        self.view = Some(View::Tasks);
        if let Some(thread) = self.threads.iter().find(|t| t.id == id) {
            let mut opened = false;
            if let Some(project) = &thread.project_id {
                opened |= self.collapsed.remove(project);
            }
            if let Some(folder) = &thread.folder_id {
                opened |= self.collapsed.remove(&format!("folder:{folder}"));
            }
            if opened {
                self.save_collapsed();
            }
        }
        self.open_thread(id, cx);
    }

    /// Light when the window shows a dark theme, else dark.
    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use patchwork_server::settings::Theme as ThemeSetting;
        let light = theme(cx).bg_app == crate::theme::Theme::light().bg_app;
        let setting = if light {
            ThemeSetting::Dark
        } else {
            ThemeSetting::Light
        };
        self.settings.theme = setting;
        if let Err(e) = self.server.store.update_settings(|s| s.theme = setting) {
            log::warn!("could not save the theme: {e}");
        }
        cx.set_global(crate::theme::Theme::for_setting(
            setting,
            window.appearance(),
        ));
        cx.refresh_windows();
    }

    /// Opens the dialog that picks a folder to add as a project.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_project.is_some() {
            return;
        }
        let return_focus = window.focused(cx);
        let server = Arc::clone(&self.server);
        let dialog = cx.new(|cx| AddProject::new(server, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            move |this, _, closed: &AddProjectClosed, window, cx| {
                this.add_project = None;
                match &return_focus {
                    Some(focus) => window.focus(focus, cx),
                    None => window.focus(&this.focus, cx),
                }
                if let Some(project) = &closed.0 {
                    // A new project starts closed like the rest of the list; one already there keeps
                    // its state.
                    if !this.settings.projects.iter().any(|p| p.id == project.id) {
                        this.collapsed.insert(project.id.clone());
                        this.save_collapsed();
                    }
                    this.settings = this.server.store.settings();
                    this.select_project(&project.id, cx);
                }
                cx.notify();
            },
        )
        .detach();
        self.add_project = Some(dialog);
        cx.notify();
    }
}
