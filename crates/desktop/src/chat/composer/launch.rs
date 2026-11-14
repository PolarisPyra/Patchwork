//! The agent, model and effort of the chats a request starts from the Orchestrator page, picked
//! there with the chat's model picker and effort menu.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClickEvent, Window, div, prelude::*};
use patchwork_server::provider::AgentKind;
use patchwork_server::settings::Settings;
use patchwork_server::threads::NewThread;

use super::menus::{Menu, effort_label, efforts};
use super::model_picker::ordered;
use super::state::Composer;
use crate::ui::theme::theme;

impl Composer {
    /// The agent and model a request's new chat gets: the ones picked on the Orchestrator page,
    /// or the default agent and its default model.
    #[must_use]
    pub fn launch_pick(&self, settings: &Settings) -> (AgentKind, Option<String>) {
        self.launch
            .clone()
            .filter(|(agent, _)| settings.agents.contains(agent))
            .unwrap_or_else(|| {
                let agent = settings.default_agent();
                (agent, settings.tokens.default_models.get(&agent).cloned())
            })
    }

    /// The name the Orchestrator page shows for a model.
    #[must_use]
    pub fn model_name(&self, agent: AgentKind, model: Option<&str>) -> String {
        self.models_of(agent)
            .and_then(|list| list.iter().find(|m| m.id.as_deref() == model))
            .map(|m| m.label.clone())
            .or_else(|| model.map(str::to_string))
            .unwrap_or_else(|| "Default".into())
    }

    /// A new thread in `project` with the agent and model picked on the Orchestrator page.
    #[must_use]
    pub fn launch_thread(&self, project_id: Option<String>, settings: &Settings) -> NewThread {
        let (agent, model) = self.launch_pick(settings);
        let mut new = self.new_thread(project_id, settings);
        new.effort = self.launch_effort(settings);
        new.agent = agent;
        new.model = model;
        new
    }

    /// The effort a request's new chat gets: the one last picked, when the model picked on the
    /// Orchestrator page has it.
    fn launch_effort(&self, settings: &Settings) -> Option<String> {
        let (agent, model) = self.launch_pick(settings);
        let levels = efforts(agent, model.as_deref(), self.models_of(agent));
        self.draft.effort.clone().filter(|e| levels.contains(e))
    }

    /// The name the Orchestrator page shows for the effort. None when the model has no levels
    /// to pick from.
    #[must_use]
    pub fn launch_effort_name(&self, settings: &Settings) -> Option<String> {
        let (agent, model) = self.launch_pick(settings);
        if efforts(agent, model.as_deref(), self.models_of(agent)).is_empty() {
            return None;
        }
        let effort = self.launch_effort(settings);
        Some(effort.map_or_else(|| "Default".into(), |e| effort_label(&e)))
    }

    /// The Orchestrator page's effort menu is open.
    #[must_use]
    pub fn launch_effort_open(&self) -> bool {
        self.launching && self.menu == Some(Menu::Effort)
    }

    /// The Orchestrator page's effort chip was clicked: opens the effort menu for it, or closes
    /// it. The press that just closed it from outside does not open it again.
    pub fn toggle_launch_effort(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ClickEvent::Mouse(click) = event
            && self.dismissed.take() == Some((Menu::Effort, click.down.position))
        {
            return;
        }
        if self.launch_effort_open() {
            return self.close_menu(window, cx);
        }
        self.open_menu(Menu::Effort, window, cx);
        self.launching = true;
    }

    /// The effort menu the Orchestrator page shows under its chip, while it is open.
    pub fn launch_effort_menu(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.launch_effort_open() {
            return None;
        }
        let workspace = self.workspace.upgrade()?;
        let workspace = workspace.read(cx);
        let mut picks = self.picks(workspace);
        (picks.agent, picks.model) = self.launch_pick(&workspace.settings);
        picks.effort = self.launch_effort(&workspace.settings);
        let t = *theme(cx);
        let rows = self.rows(Menu::Effort, &picks, false, &t);
        self.menu_picks = rows.iter().map(|r| r.pick.clone()).collect();
        if self.highlight >= rows.len() {
            self.highlight = rows.iter().position(|r| r.checked).unwrap_or(0);
        }
        let focus = self.menu_focus.clone();
        let menu = self.menu_box(Menu::Effort, rows, &focus, &t, cx);
        // The page's text color is the backdrop's, not the theme's.
        Some(div().text_color(t.text).child(menu).into_any_element())
    }

    /// The Orchestrator page's model picker is open.
    #[must_use]
    pub fn launch_picker_open(&self) -> bool {
        self.launching && self.menu == Some(Menu::Model) && self.picker.is_some()
    }

    /// The Orchestrator page's chip was clicked: opens the model picker for it, or closes it.
    /// The press that just closed it from outside does not open it again.
    pub fn toggle_launch_picker(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let ClickEvent::Mouse(click) = event
            && self.dismissed.take() == Some((Menu::Model, click.down.position))
        {
            return;
        }
        if self.launch_picker_open() {
            return self.close_menu(window, cx);
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let settings = &workspace.read(cx).settings;
        let (agents, current) = (ordered(&settings.agents), self.launch_pick(settings));
        if self.menu.is_none() {
            self.menu_came_from = window.focused(cx);
        }
        self.menu = Some(Menu::Model);
        self.launching = true;
        self.open_model_picker(&agents, current, window, cx);
        cx.notify();
    }

    /// The model picker the Orchestrator page shows under its chip, while it is open.
    pub fn launch_picker(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.launch_picker_open() {
            return None;
        }
        let settings = &self.workspace.upgrade()?.read(cx).settings;
        let (agents, (agent, model)) = (ordered(&settings.agents), self.launch_pick(settings));
        let t = *theme(cx);
        let picker = self.model_picker(&agents, (agent, model.as_deref()), &t, window, cx);
        // The page's text color is the backdrop's, not the theme's.
        Some(div().text_color(t.text).child(picker).into_any_element())
    }
}
