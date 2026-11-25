//! The pill a chat app agent's chat shows for each Patchwork tool it ran, centred in the
//! transcript so the agent's history reads at a glance.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Role, SharedString, div, prelude::*, px};
use patchwork_core::chats::{Message, MessageStatus, StepStatus, ToolCall};

use crate::chat::transcript::turn_contents::{PATCHWORK_TOOL, is_running};
use patchwork_ui::theme::{Theme, fs_xs};
use patchwork_ui::{icon, spinner};

/// The icon and the words a Patchwork tool's pill shows.
fn note_of(tool: &ToolCall) -> (&'static str, String) {
    let id = tool.name.strip_prefix(PATCHWORK_TOOL).unwrap_or(&tool.name);
    let (glyph, label) = match id {
        "list_tasks" => ("eye", "Read tasks"),
        "list_chats" => ("list-bullets", "Listed chats"),
        "read_chat" => ("eye", "Read a chat"),
        "start_chat" => ("chat-circle", "Started a chat"),
        "send_to_chat" => ("paper-plane-right", "Sent to a chat"),
        "wait_for_chat" | "notify_when_done" => ("bell", "Waited for a chat"),
        "stop_chat" => ("stop", "Stopped a chat"),
        "answer_chat" => ("chat-text", "Answered a chat"),
        "set_chat_options" => ("gear-six", "Changed chat options"),
        "rename_chat" => ("pencil-simple", "Renamed a chat"),
        "archive_chat" => ("archive", "Archived a chat"),
        "delete_chat" => ("trash", "Deleted a chat"),
        "open_chat" => ("chat-circle", "Opened a chat"),
        "open_project" => ("folder-open", "Opened a project"),
        "open_file" => ("file-text", "Opened a file"),
        "open_settings" => ("gear-six", "Opened settings"),
        "export_chat" => ("export", "Exported a chat"),
        "scan_imports" | "import_chats" => ("file-text", "Imported chats"),
        "send_project_file" => ("file-text", "Sent a file"),
        "send_picture" => ("image", "Sent a picture"),
        "send_screenshot" => ("image", "Sent a screenshot"),
        "send_chat_changes" => ("file-text", "Sent changes"),
        "list_projects" => ("list-bullets", "Listed projects"),
        "list_project_files" => ("magnifying-glass", "Browsed project files"),
        "add_project" => ("folder-plus", "Added a project"),
        "rename_project" => ("pencil-simple", "Renamed a project"),
        "remove_project" => ("trash", "Removed a project"),
        "get_settings" => ("gear-six", "Read settings"),
        "set_setting" => ("gear-six", "Changed a setting"),
        "list_themes" | "read_theme" | "preview_theme" => ("palette", "Looked at themes"),
        "save_theme" | "apply_theme" | "delete_theme" => ("palette", "Changed a theme"),
        "list_scheduled_tasks" => ("calendar-blank", "Listed scheduled tasks"),
        "run_scheduled_task" => ("play", "Ran a scheduled task"),
        "set_scheduled_task_enabled" | "delete_scheduled_task" => {
            ("calendar-blank", "Changed a scheduled task")
        }
        "gateway_status" => ("device-mobile", "Checked chat apps"),
        "set_chat_app_enabled" | "make_pairing_code" | "restart_gateway" | "remove_pairing" => {
            ("device-mobile", "Changed chat apps")
        }
        "list_providers" | "list_models" => ("list-bullets", "Listed models"),
        "install_agent" => ("arrow-circle-up", "Installed an agent"),
        "list_mcp_servers" | "list_agents" | "list_hooks" => ("plug", "Listed integrations"),
        "restart_mcp_server"
        | "set_mcp_server_enabled"
        | "remove_mcp_server"
        | "set_hook_enabled" => ("plugs-connected", "Changed an integration"),
        "list_skills" => ("books", "Listed skills"),
        "set_skill_enabled" => ("books", "Changed a skill"),
        "set_chat_notices" | "unmute_all_chats" => ("bell", "Changed notifications"),
        "usage_limits" => ("gauge", "Checked usage"),
        "resume_chat_at_reset" => ("arrow-clockwise", "Set a chat to resume"),
        "list_capabilities" => ("list-checks", "Listed what it can do"),
        "diagnostics" => ("stethoscope", "Ran diagnostics"),
        "check_for_updates" => ("arrow-circle-up", "Checked for updates"),
        "install_update" => ("arrow-circle-up", "Installed an update"),
        "request_secret" => ("key", "Asked for a secret"),
        other => return ("wrench", format!("Used {}", other.replace('_', " "))),
    };
    (glyph, label.to_string())
}

/// The centred pill for Patchwork tool `tool` in reply `m`.
pub(super) fn note_pill(m: &Message, i: usize, tool: &ToolCall, t: &Theme) -> Div {
    let (glyph, label) = note_of(tool);
    let running = m.status == MessageStatus::Streaming && is_running(tool);
    let failed = tool.status == StepStatus::Error;
    let color = if failed { t.danger } else { t.text_muted };
    let mark = if running {
        spinner(11., color).into_any_element()
    } else if failed {
        icon("x-circle", 12.).text_color(color).into_any_element()
    } else {
        icon(glyph, 12.).text_color(color).into_any_element()
    };
    let spoken = if failed {
        format!("{label} failed")
    } else {
        label.clone()
    };
    div().pb(px(8.)).flex().justify_center().child(
        div()
            .id(SharedString::from(format!("{}-note-{i}", m.id)))
            .role(Role::Status)
            .aria_label(spoken)
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(22.))
            .px(px(10.))
            .rounded_full()
            .bg(t.text_tint_8)
            .opacity(0.75)
            .text_size(px(fs_xs()))
            .text_color(color)
            .child(mark)
            .child(label),
    )
}
