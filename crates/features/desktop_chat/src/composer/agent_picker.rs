//! The custom-agent menu: no agent, then the project's agents, the active one checked, and a
//! menu of them to open one's file, in the app's context-menu look.

// Rust guideline compliant 2026-10-09

use gpui::{AppContext, Context};
use patchwork_client::local::core::extensions::agents::Agent;
use patchwork_design_components::menu::MenuItem;
use patchwork_design_icons::IconName;

use super::Composer;
use super::menus::{Pick, Roles};

/// More agents than this and typing narrows the menu.
const FILTER_FROM: usize = 8;

impl Composer {
    /// The agent menu's rows, with the agent named `current` checked, and whether typing should
    /// narrow them.
    pub(super) fn agent_items(
        &self,
        current: Option<&str>,
        cx: &Context<Self>,
    ) -> (Vec<MenuItem>, bool) {
        let agents = match &self.roles {
            Roles::Loaded(agents) => agents.as_slice(),
            Roles::Loading => &[],
        };
        let pick = |role: Option<String>| {
            let composer = cx.entity().downgrade();
            move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let _ = composer.update(cx, |c, cx| {
                    c.commit(Pick::Role(role.clone()), cx);
                    cx.notify();
                });
            }
        };
        let mut items = vec![
            MenuItem::new("No agent", pick(None))
                .icon(IconName::Prohibit)
                .radio(current.is_none()),
        ];
        items.extend(agents.iter().enumerate().map(|(i, agent)| {
            let item = MenuItem::new(agent.name.clone(), pick(Some(agent.name.clone())))
                .icon(IconName::Robot)
                .radio(current == Some(agent.name.as_str()));
            if i == 0 { item.separated() } else { item }
        }));
        if !agents.is_empty() {
            let edits = agents
                .iter()
                .map(|agent| {
                    let agent = agent.clone();
                    MenuItem::new(agent.name.clone(), move |_, cx| open_agent(&agent, cx))
                        .icon(IconName::Robot)
                })
                .collect();
            items.push(
                MenuItem::submenu("Edit agent", edits)
                    .icon(IconName::PencilSimple)
                    .separated(),
            );
        }
        (items, agents.len() > FILTER_FROM)
    }
}

/// Opens an agent's file in the editor.
fn open_agent(agent: &Agent, cx: &gpui::App) {
    let path = std::path::PathBuf::from(&agent.path);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let (dir, name) = (
        dir.to_string_lossy().into_owned(),
        name.to_string_lossy().into_owned(),
    );
    cx.background_spawn(async move {
        if let Err(e) = patchwork_platform::open_in_editor(&dir, &name, None) {
            log::warn!(error:% = e; "could not open the agent: {{error}}");
        }
    })
    .detach();
}
