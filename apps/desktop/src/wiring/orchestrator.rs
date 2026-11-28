//! The orchestrator page in the window: its view of the chat box's launch choices, and what it
//! asks of the workspace.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, App, ClickEvent, Entity, Window, prelude::*};
use patchwork_core::chats::NewChat;
use patchwork_desktop_orchestrator::{
    Launch, LaunchMenu, Orchestrator, OrchestratorEvent, OrchestratorHost,
};
use patchwork_client::AppState;
use std::rc::Rc;

use patchwork_desktop_chat::Composer;
use crate::window::Workspace;

/// The orchestrator's view of the chat box, which holds what a request's new chat starts with.
struct Host(Entity<Composer>);

impl OrchestratorHost for Host {
    fn launch(&self, cx: &App) -> Launch {
        let settings = &AppState::get(cx).settings;
        let composer = self.0.read(cx);
        let (agent, model) = composer.launch_pick(settings);
        let open = if composer.launch_picker_open() {
            Some(LaunchMenu::Model)
        } else if composer.launch_effort_open() {
            Some(LaunchMenu::Effort)
        } else if composer.launch_project_open() {
            Some(LaunchMenu::Project)
        } else {
            None
        };
        Launch {
            agent,
            model_name: composer.model_name(agent, model.as_deref()),
            model,
            effort: composer.launch_effort_name(settings),
            project: composer.launch_project(settings),
            open,
        }
    }

    fn launch_menu(
        &self,
        menu: LaunchMenu,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.0.update(cx, |composer, cx| match menu {
            LaunchMenu::Model => composer.launch_picker(window, cx),
            LaunchMenu::Effort => composer.launch_effort_menu(cx),
            LaunchMenu::Project => composer.launch_project_menu(cx),
        })
    }

    fn toggle_launch_menu(
        &self,
        menu: LaunchMenu,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.update(cx, |composer, cx| match menu {
            LaunchMenu::Model => composer.toggle_launch_picker(event, window, cx),
            LaunchMenu::Effort => composer.toggle_launch_effort(event, window, cx),
            LaunchMenu::Project => composer.toggle_launch_project(event, window, cx),
        });
    }

    fn new_chat(&self, cx: &App) -> NewChat {
        let settings = &AppState::get(cx).settings;
        let composer = self.0.read(cx);
        composer.launch_chat(composer.launch_project(settings), settings)
    }
}

impl Workspace {
    /// Makes the orchestrator page over `composer`, and handles what it asks of the window.
    pub(crate) fn new_orchestrator(
        composer: &Entity<Composer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Orchestrator> {
        let host: Rc<dyn OrchestratorHost> = Rc::new(Host(composer.clone()));
        let orchestrator = cx.new(|cx| Orchestrator::new(host, window, cx));
        // The model picker is the chat box's, drawn on the page, and so is the project menu.
        let page = orchestrator.downgrade();
        cx.observe(composer, move |_, _, cx| {
            let _ = page.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.subscribe(
            &orchestrator,
            |this, _, event: &OrchestratorEvent, cx| match event {
                OrchestratorEvent::OpenChat(id) => this.open_chat(id, cx),
                OrchestratorEvent::PeekChat(id) => this.peek_chat(id, cx),
            },
        )
        .detach();
        orchestrator
    }
}
