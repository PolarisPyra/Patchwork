//! The workspace's state: the open chat, the views around it, and the window's regions.

// Rust guideline compliant 2026-10-09

use patchwork_client::GitModel;
use patchwork_client::AppState;

use std::sync::Arc;
use std::time::Duration;
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{Task, AnyElement, Entity, Focusable, Subscription, WeakEntity, Window, div, prelude::*};
use patchwork_core::Core;
use patchwork_core::permissions::PermissionKind;
use patchwork_core::projects::Project;
use patchwork_core::chats::Message;
use patchwork_core::vault::{Method, Status};
use patchwork_core::projects::ProjectChanged;
use patchwork_desktop_canvas::Canvas;
use patchwork_desktop_chat::prelude::*;
use patchwork_desktop_chat::{ChatUi, ChatView};
use patchwork_desktop_orchestrator::{Orchestrator, OrchestratorBackdrop};
use patchwork_desktop_source_control::ChangesView;
use crate::row_menu::ContextMenu;
use crate::nav::{Page, Place, View};
use crate::panels::dock as place;

pub use patchwork_desktop_sidebar::{SIDEBAR_MAX, SIDEBAR_MIN, SIDEBAR_WIDTH};

pub struct Workspace {
    /// The open chat and the chat's view state; the workspace derefs to it.
    pub ui: ChatUi,
    /// The page of the agent shown in place of the chat, once drawn.
    pub agent_page: Option<Entity<patchwork_desktop_agents::AgentPage>>,
    pub view: Option<View>,
    pub page: Page,
    /// Whether the orchestrator page has the task list beside it. Its own, so opening or closing
    /// it leaves the chats' sidebar as it was.
    pub(crate) orchestrator_tasks: bool,
    pub(crate) orchestrator: Entity<Orchestrator>,
    /// The orchestrator page's backdrop, drawn under it as a view of its own.
    pub(crate) orchestrator_backdrop: Entity<OrchestratorBackdrop>,
    /// The Quilts page while it shows. Closed with it, so its browser runs only then.
    pub(crate) quilts: Option<Entity<patchwork_desktop_quilts::QuiltsPage>>,
    /// The quilt the Quilts page has open, by its id, which it opens on again when it is
    /// made.
    pub(crate) quilt_open: Option<String>,
    /// The quilt shown beside a chat, with that chat's id; it shows while the chat does.
    pub(crate) quilt_beside: Option<(String, Entity<patchwork_desktop_quilts::QuiltsPage>)>,
    /// Whether the chat a quilt came from shows in a card over the Quilts page.
    pub(crate) quilt_chat: bool,
    /// The quilts' pictures, for the chats' cards and the gallery.
    pub(crate) thumbs: Entity<patchwork_desktop_quilts::Thumbs>,
    /// What filling the window was last saved as, to open again at the next launch.
    pub(crate) saved_place: Option<patchwork_core::chats::LastPlace>,
    /// What was shown before and after what is shown now, newest last, for going back and
    /// forward.
    pub(crate) history_back: Vec<Place>,
    pub(crate) history_forward: Vec<Place>,
    pub(crate) _prompt_keys: gpui::Subscription,
    pub changes_view: Entity<ChangesView>,
    /// The Devices view, made the first time it is shown.
    pub devices_view: Option<Entity<crate::machines::DevicesView>>,
    /// Where git's state arrives from the project watcher's chat.
    pub(crate) project_events: mpsc::UnboundedSender<ProjectChanged>,
    /// The file panel right of the chat, its width once dragged, and where a drag of it started.
    pub canvas: Option<Entity<Canvas>>,
    pub(crate) canvas_width: Option<f32>,
    pub(crate) canvas_resizing: Option<(f32, f32)>,
    /// The project the mobile app was last looked for in, and the Android panel, which takes the
    /// file panel's place while it is open.
    pub(crate) mobile_for: Option<String>,
    pub(crate) android: Option<Entity<patchwork_desktop_emulator::AndroidPanel>>,
    /// The terminal and Output panel under the chat, which keeps whether it shows and its height.
    pub(crate) dock: Entity<patchwork_desktop_dock::Dock>,
    /// The sidebar and the chat, each drawn as its own view so a scroll in one leaves the other's frame.
    pub(crate) sidebar: Entity<patchwork_desktop_sidebar::Sidebar>,
    pub(crate) chat_view: Entity<ChatView<Workspace>>,
    /// The chat's background, under the chat region, so a moving backdrop leaves the chat's frame.
    pub(crate) backdrop_region: Entity<Region>,
    /// The Quilts page's background, under the page, as `backdrop_region` is under the chat.
    pub(crate) quilts_backdrop_region: Entity<Region>,
    /// The status bar, so its spinner redraws only the bar as it turns, not the whole workspace.
    pub(crate) status_region: Entity<Region>,
    pub sidebar_width: f32,
    /// Where a sidebar drag started: the pointer's x and the width then.
    pub(crate) resizing: Option<(f32, f32)>,
    /// The wait for the clock or the sun to change the theme.
    pub(crate) theme_switch: Option<Task<()>>,
    /// The project whose look the app is drawn in.
    pub(crate) themed_project: Option<String>,
    pub usage: Entity<patchwork_desktop_usage::Usage>,
    /// The dialogs and menus over the window, while open.
    pub(crate) dialogs: Entity<crate::dialogs::Dialogs>,
    /// The version whose update card the user closed.
    pub(crate) update_dismissed: Option<String>,
    /// First-run setup, shown instead of everything else until the settings say it is done.
    pub(crate) onboarding: Option<Entity<patchwork_desktop_onboarding::Onboarding>>,
    /// The open context menu.
    pub context_menu: Option<ContextMenu>,
    /// The window's title as last set: the app's name, then the open chat's title after a dash.
    pub(crate) title: String,
}

impl Workspace {
    pub fn new(core: Arc<Core>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = AppState::get(cx).settings.shared();
        let update_dismissed = core.dismissed_update();
        let workspace = cx.entity().downgrade();
        let composer = Self::new_composer(workspace.clone(), window, cx);
        let changes_view = Self::new_changes_view(window, cx);
        let usage = Self::new_usage(window, cx);
        // The panel above the composer answers its keys before the composer's bindings do.
        let weak = cx.entity().downgrade();
        let prompt_keys = cx.intercept_keystrokes(move |ev, window, cx| {
            let _ = weak.update(cx, |w, cx| w.on_prompt_key(&ev.keystroke, window, cx));
        });
        let project_events = Self::listen_for_project_changes(cx);
        let entity = cx.entity();
        let sidebar = Self::new_sidebar(window, cx);
        let home_focus = cx.focus_handle();
        let chat_view = cx.new(|cx| ChatView::new(&entity, home_focus.clone(), cx));
        let backdrop_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, _, cx| patchwork_desktop_chat::backdrop::backdrop_layer(w, cx),
                cx,
            )
        });
        let quilts_backdrop_region = cx.new(|cx| {
            Region::new(
                &entity,
                |w, _, cx| patchwork_desktop_chat::backdrop::quilts_backdrop_layer(w, cx),
                cx,
            )
        });
        let orchestrator = Self::new_orchestrator(window, cx);
        let orchestrator_backdrop = cx.new(|cx| OrchestratorBackdrop::new(&orchestrator, cx));
        let thumbs = cx.new(|_| patchwork_desktop_quilts::Thumbs::new(Arc::clone(&core)));
        cx.observe(&thumbs, |_, _, cx| cx.notify()).detach();
        // The views drawn over the workspace observe it, so they draw again with it.
        cx.observe_global::<AppState>(|_, cx| cx.notify()).detach();
        cx.observe(&GitModel::entity(cx), |_, _, cx| cx.notify())
            .detach();
        let status_region =
            cx.new(|cx| Region::new(&entity, |w, _, cx| w.status_bar(cx).into_any_element(), cx));
        let dock = place::new_panel(Arc::clone(&core), &settings, window, cx);
        let themed_project = settings.window.active_project_id.clone();
        let credentials =
            cx.new(|_| patchwork_desktop_dialogs::credential::CredentialDialog::default());
        cx.subscribe_in(
            &credentials,
            window,
            |this, _, _: &patchwork_desktop_dialogs::credential::Answered, window, cx| {
                window.focus(&this.focus, cx);
            },
        )
        .detach();
        cx.observe(&credentials, |_, _, cx| cx.notify()).detach();
        let interrupted =
            cx.new(|_| patchwork_desktop_dialogs::interrupted::InterruptedCard::default());
        cx.subscribe(
            &interrupted,
            |this, _, _: &patchwork_desktop_dialogs::interrupted::Resume, cx| {
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
        let dialogs = cx.new(|_| crate::dialogs::Dialogs::new(credentials, interrupted));
        let ui = ChatUi::new(core, composer, home_focus, cx);
        let mut this = Self {
            ui,
            agent_page: None,
            view: Some(View::Tasks),
            page: Page::Orchestrator,
            orchestrator_tasks: true,
            orchestrator,
            orchestrator_backdrop,
            quilts: None,
            quilt_open: None,
            quilt_beside: None,
            quilt_chat: false,
            thumbs,
            saved_place: None,
            history_back: Vec::new(),
            history_forward: Vec::new(),
            _prompt_keys: prompt_keys,
            changes_view,
            devices_view: None,
            project_events,
            canvas: None,
            canvas_width: None,
            canvas_resizing: None,
            mobile_for: None,
            android: None,
            dock,
            sidebar,
            chat_view,
            backdrop_region,
            quilts_backdrop_region,
            status_region,
            sidebar_width: SIDEBAR_WIDTH,
            resizing: None,
            theme_switch: None,
            themed_project,
            usage,
            dialogs,
            update_dismissed,
            onboarding: None,
            title: patchwork_views_actions::APP_TITLE.into(),
            context_menu: None,
        };
        this.restore_layouts();
        let last = this
            .core
            .last_chat_id()
            .filter(|id| AppState::get(cx).chats.iter().any(|t| &t.id == id));
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
        this.watch_cli_updates(cx);
        // The composer starts focused, so typing and the app's keys work before any click.
        let composer = this.composer.read(cx).editor.focus_handle(cx);
        window.focus(&composer, cx);
        // A vault with a password opens nothing saved until it is entered, and one without a
        // keychain to keep its key needs a password set.
        if AppState::get(cx).settings.onboarded
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
            patchwork_design_theme::follow(window.appearance(), cx);
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

impl std::ops::Deref for Workspace {
    type Target = ChatUi;

    fn deref(&self) -> &ChatUi {
        &self.ui
    }
}

impl std::ops::DerefMut for Workspace {
    fn deref_mut(&mut self) -> &mut ChatUi {
        &mut self.ui
    }
}

impl ChatWindow for Workspace {
    fn chat_view(&self) -> &Entity<ChatView<Self>> {
        &self.chat_view
    }

    fn show_chats_page(&mut self) {
        self.page = Page::Chats;
    }

    fn show_changes_view(&mut self) {
        self.view = Some(View::Changes);
    }

    fn quilt_icon(kind: patchwork_core::quilts::QuiltKind) -> patchwork_design_icons::IconName {
        patchwork_desktop_quilts::kind_icon(kind)
    }

    fn quilt_picture(
        &self,
        id: &str,
        n: u32,
        dark: bool,
        cx: &mut gpui::App,
    ) -> Option<std::path::PathBuf> {
        let thumbs = self.thumbs.clone();
        let taken = thumbs
            .read(cx)
            .get(id, n, dark)
            .map(Option::<&std::path::PathBuf>::cloned);
        if taken.is_none() {
            thumbs.update(cx, |thumbs, cx| thumbs.want(id, n, dark, cx));
        }
        taken.flatten()
    }

    fn permission_label(kind: PermissionKind) -> &'static str {
        patchwork_desktop_settings::settings::kind_label(kind)
    }

    fn scope_name(
        scope: &patchwork_core::permissions::PermissionScope,
        projects: &[Project],
    ) -> String {
        patchwork_desktop_settings::settings::scope_name(scope, projects)
    }

    fn reload_quilts(&mut self, cx: &mut Context<Self>) {
        self.load_quilt_list(cx);
        if let Some(page) = &self.quilts {
            page.update(cx, patchwork_desktop_quilts::QuiltsPage::reload);
        }
    }

    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        crate::updates::restart(cx);
    }

    fn reopen_settings(
        &mut self,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dialogs_mut(cx, |d| d.settings = None);
        self.open_settings(section, window, cx);
    }

    fn ask_credentials(
        &mut self,
        request: patchwork_core::source_control::CredentialRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dialog = self.dialogs.read(cx).credentials.clone();
        dialog.update(cx, |d, cx| d.ask(request, window, cx));
    }

    fn refresh_costs(&mut self, cx: &mut Context<Self>) {
        if let Some(dialog) = self.settings_dialog(cx) {
            dialog.update(
                cx,
                patchwork_desktop_settings::settings::SettingsDialog::refresh_costs,
            );
        }
    }

    fn ask_granular(&mut self, chat_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let scope = patchwork_desktop_dialogs::granular::GranularScope::Chat(chat_id);
        self.open_granular_dialog(scope, None, window, cx);
    }

    fn open_context_menu(
        &mut self,
        target: patchwork_desktop_chat::MenuTarget,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::row_menu::Target;
        use patchwork_desktop_chat::MenuTarget;
        let target = match target {
            MenuTarget::Pane(pane) => Target::Pane(pane),
            MenuTarget::Tabs(project) => Target::Tabs(project),
            MenuTarget::Layouts => Target::Layouts,
            MenuTarget::Chat(id) => Target::Chat(id),
        };
        Workspace::open_context_menu(self, target, at, window, cx);
    }

    fn open_file(
        &mut self,
        path: &str,
        message: Option<Message>,
        diff: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_canvas(path, message, canvas_tab(diff), window, cx);
    }

    fn open_file_at(
        &mut self,
        path: &str,
        root: String,
        message: Option<Message>,
        diff: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_canvas_at(path, root, message, canvas_tab(diff), window, cx);
    }

    fn android_open(&self, cx: &gpui::App) -> bool {
        Workspace::android_open(self, cx)
    }

    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Workspace::apply_theme(self, window, cx);
    }

    fn back_to_orchestrator(&mut self, cx: &mut Context<Self>) {
        Workspace::back_to_orchestrator(self, cx);
    }

    fn can_go_back(&self) -> bool {
        Workspace::can_go_back(self)
    }

    fn chat_agent<'a>(&'a self, cx: &'a gpui::App) -> Option<&'a patchwork_core::agents::Agent> {
        Workspace::chat_agent(self, cx)
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        Workspace::go_back(self, cx);
    }

    fn load_git(&mut self, cx: &mut Context<Self>) {
        Workspace::load_git(self, cx);
    }

    fn new_terminal_in(&mut self, cwd: String, window: &mut Window, cx: &mut Context<Self>) {
        Workspace::new_terminal_in(self, cwd, window, cx);
    }

    fn on_update_change(&mut self, cx: &mut Context<Self>) {
        Workspace::on_update_change(self, cx);
    }

    fn open_change(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        Workspace::open_change(self, path, window, cx);
    }

    fn open_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        Workspace::open_chat(self, id, cx);
    }

    fn open_quilt_beside(
        &mut self,
        id: String,
        n: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Workspace::open_quilt_beside(self, id, n, window, cx);
    }

    fn open_rule(
        &mut self,
        scope: patchwork_core::permissions::PermissionScope,
        rule_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Workspace::open_rule(self, scope, rule_id, window, cx);
    }

    fn read_git(&mut self, sync: bool, cx: &mut Context<Self>) {
        Workspace::read_git(self, sync, cx);
    }

    fn reload_agents(&mut self, cx: &mut Context<Self>) {
        Workspace::reload_agents(self, cx);
    }

    fn remember_project(&mut self, id: Option<String>, cx: &mut gpui::App) {
        Workspace::remember_project(self, id, cx);
    }

    fn reveal_chat(&mut self, id: &str, cx: &mut Context<Self>) {
        Workspace::reveal_chat(self, id, cx);
    }

    fn select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        Workspace::select_project(self, id, cx);
    }

    fn show_in_terminal(
        &mut self,
        launch: patchwork_core::terminal::Launch,
        name: String,
        cx: &mut Context<Self>,
    ) {
        Workspace::show_in_terminal(self, launch, name, cx);
    }

    fn shows_notes(&self, cx: &gpui::App) -> bool {
        Workspace::shows_notes(self, cx)
    }

    fn sync_panel(&mut self, cx: &mut Context<Self>) {
        Workspace::sync_panel(self, cx);
    }

    fn toggle_agent_details(&mut self, cx: &mut Context<Self>) {
        Workspace::toggle_agent_details(self, cx);
    }

    fn toggle_android(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        Workspace::toggle_android(self, window, cx);
    }
}

/// The file panel's tab for a file opened as its diff or as its code.
fn canvas_tab(diff: bool) -> patchwork_desktop_canvas::CanvasTab {
    if diff {
        patchwork_desktop_canvas::CanvasTab::Diff
    } else {
        patchwork_desktop_canvas::CanvasTab::Code
    }
}
