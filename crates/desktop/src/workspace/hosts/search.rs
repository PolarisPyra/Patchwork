//! What search lists, from the window's state: actions, then every chat and task by project,
//! then the projects.

// Rust guideline compliant 2026-10-09

use patchwork_core::chats::{MessageStatus, Role as MessageRole};
use patchwork_modals::search::{Action, Item};
use patchwork_modals::settings::key_label;
use patchwork_ui::display_title;

use crate::workspace::Workspace;

/// What search lists: actions, then every chat and task by project, then the projects.
pub(crate) fn items(w: &Workspace) -> Vec<Item> {
    let item = |group: &str, label: String, hint: Option<String>, action| Item {
        group: group.into(),
        label,
        hint,
        action,
    };
    let new_key = key_label(&w.core, "chat.new");
    let project = w.project();
    let mut items = vec![item(
        "Actions",
        "New chat".into(),
        new_key.clone().filter(|_| project.is_none()),
        Action::NewChat,
    )];
    if let Some(project) = project {
        items.push(item(
            "Actions",
            format!("New task in {}", project.name),
            new_key,
            Action::NewTask(project.id.clone()),
        ));
        items.push(item(
            "Actions",
            format!("New task in a worktree of {}", project.name),
            None,
            Action::NewWorktreeTask(project.id.clone()),
        ));
    }
    let chat = w.chat.as_ref();
    // The reply the checks are for: the last one the open task got.
    let checkable = chat.filter(|_| project.is_some()).and_then(|t| {
        t.messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::Assistant && m.status != MessageStatus::Streaming)
    });
    if let Some(reply) = checkable {
        items.push(item(
            "Actions",
            "Run checks for the open task".into(),
            None,
            Action::RunChecks(reply.id.clone()),
        ));
    }
    if let Some(chat) = chat.filter(|t| t.parent.is_none()) {
        items.push(item(
            "Actions",
            "Change this chat’s background".into(),
            None,
            Action::ChatBackground(chat.id.clone()),
        ));
    }
    items.push(item(
        "Actions",
        "Toggle theme".into(),
        None,
        Action::ToggleTheme,
    ));
    items.push(item(
        "Actions",
        "Open settings".into(),
        key_label(&w.core, "settings.open"),
        Action::OpenSettings,
    ));
    items.push(item(
        "Actions",
        "Open folder…".into(),
        None,
        Action::OpenFolder,
    ));
    let projects = &w.settings.projects;
    items.extend(
        w.chats
            .iter()
            .filter(|t| t.archived_at.is_none() && t.parent.is_none())
            .map(|t| {
                // An agent's runs are found under the agent's name.
                let agent = t
                    .agent_id
                    .as_ref()
                    .and_then(|id| w.agents.iter().find(|b| &b.id == id));
                let project = t
                    .project_id
                    .as_ref()
                    .and_then(|id| projects.iter().find(|p| &p.id == id));
                let group = match (agent, project) {
                    (Some(agent), _) => agent.name.as_str(),
                    (None, Some(p)) => p.name.as_str(),
                    (None, None) => "Chats",
                };
                item(
                    group,
                    display_title(&t.title),
                    None,
                    Action::OpenChat(t.id.clone()),
                )
            }),
    );
    items.extend(projects.iter().map(|p| {
        item(
            "Projects",
            p.name.clone(),
            Some(p.path.clone()),
            Action::OpenProject(p.id.clone()),
        )
    }));
    items
}
