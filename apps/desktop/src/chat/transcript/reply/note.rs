//! The pill a chat app agent's chat shows for each Patchwork tool it ran, centred in the
//! transcript so the agent's history reads at a glance.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Role, SharedString, div, prelude::*, px};
use patchwork_backend::chats::{Message, StepStatus, ToolCall};

use crate::chat::transcript::turn_contents::{PATCHWORK_TOOL, is_running};
use patchwork_ui_theme::{IconName, Theme, fs_xs};
use patchwork_ui_theme::icon;
use patchwork_ui_components::spinner;

/// The icon and the words a Patchwork tool's pill shows.
fn note_of(tool: &ToolCall) -> (IconName, String) {
    let id = tool.name.strip_prefix(PATCHWORK_TOOL).unwrap_or(&tool.name);
    let (glyph, label) = match id {
        "list_tasks" => (IconName::Eye, "Read tasks"),
        "list_chats" => (IconName::ListBullets, "Listed chats"),
        "read_chat" => (IconName::Eye, "Read a chat"),
        "start_chat" => (IconName::ChatCircle, "Started a chat"),
        "send_to_chat" => (IconName::PaperPlaneRight, "Sent to a chat"),
        "wait_for_chat" | "notify_when_done" => (IconName::Bell, "Waited for a chat"),
        "stop_chat" => (IconName::Stop, "Stopped a chat"),
        "answer_chat" => (IconName::ChatText, "Answered a chat"),
        "set_chat_options" => (IconName::GearSix, "Changed chat options"),
        "rename_chat" => (IconName::PencilSimple, "Renamed a chat"),
        "archive_chat" => (IconName::Archive, "Archived a chat"),
        "delete_chat" => (IconName::Trash, "Deleted a chat"),
        "open_chat" => (IconName::ChatCircle, "Opened a chat"),
        "open_project" => (IconName::FolderOpen, "Opened a project"),
        "open_file" => (IconName::FileText, "Opened a file"),
        "open_settings" => (IconName::GearSix, "Opened settings"),
        "export_chat" => (IconName::Export, "Exported a chat"),
        "scan_imports" | "import_chats" => (IconName::FileText, "Imported chats"),
        "send_project_file" => (IconName::FileText, "Sent a file"),
        "send_picture" => (IconName::Image, "Sent a picture"),
        "send_screenshot" => (IconName::Image, "Sent a screenshot"),
        "send_chat_changes" => (IconName::FileText, "Sent changes"),
        "list_projects" => (IconName::ListBullets, "Listed projects"),
        "list_project_files" => (IconName::MagnifyingGlass, "Browsed project files"),
        "add_project" => (IconName::FolderPlus, "Added a project"),
        "rename_project" => (IconName::PencilSimple, "Renamed a project"),
        "remove_project" => (IconName::Trash, "Removed a project"),
        "get_settings" => (IconName::GearSix, "Read settings"),
        "set_setting" => (IconName::GearSix, "Changed a setting"),
        "list_themes" | "read_theme" | "preview_theme" => (IconName::Palette, "Looked at themes"),
        "save_theme" | "apply_theme" | "delete_theme" => (IconName::Palette, "Changed a theme"),
        "list_scheduled_tasks" => (IconName::CalendarBlank, "Listed scheduled tasks"),
        "run_scheduled_task" => (IconName::Play, "Ran a scheduled task"),
        "set_scheduled_task_enabled" | "delete_scheduled_task" => {
            (IconName::CalendarBlank, "Changed a scheduled task")
        }
        "gateway_status" => (IconName::DeviceMobile, "Checked chat apps"),
        "set_chat_app_enabled" | "make_pairing_code" | "restart_gateway" | "remove_pairing" => {
            (IconName::DeviceMobile, "Changed chat apps")
        }
        "list_providers" | "list_models" => (IconName::ListBullets, "Listed models"),
        "install_agent" => (IconName::ArrowCircleUp, "Installed an agent"),
        "list_mcp_servers" | "list_agents" | "list_hooks" => {
            (IconName::Plug, "Listed integrations")
        }
        "restart_mcp_server"
        | "set_mcp_server_enabled"
        | "remove_mcp_server"
        | "set_hook_enabled" => (IconName::PlugsConnected, "Changed an integration"),
        "list_skills" => (IconName::Books, "Listed skills"),
        "set_skill_enabled" => (IconName::Books, "Changed a skill"),
        "set_chat_notices" | "unmute_all_chats" => (IconName::Bell, "Changed notifications"),
        "usage_limits" => (IconName::Gauge, "Checked usage"),
        "resume_chat_at_reset" => (IconName::ArrowClockwise, "Set a chat to resume"),
        "list_capabilities" => (IconName::ListChecks, "Listed what it can do"),
        "diagnostics" => (IconName::Stethoscope, "Ran diagnostics"),
        "check_for_updates" => (IconName::ArrowCircleUp, "Checked for updates"),
        "install_update" => (IconName::ArrowCircleUp, "Installed an update"),
        "request_secret" => (IconName::Key, "Asked for a secret"),
        other => {
            return (
                IconName::Wrench,
                format!("Used {}", other.replace('_', " ")),
            );
        }
    };
    (glyph, label.to_string())
}

/// The centred pill for Patchwork tool `tool` in reply `m`.
pub(super) fn note_pill(m: &Message, i: usize, tool: &ToolCall, t: &Theme) -> Div {
    let (glyph, label) = note_of(tool);
    let running = m.is_streaming() && is_running(tool);
    let failed = tool.status == StepStatus::Error;
    let color = if failed { t.danger } else { t.text_muted };
    let mark = if running {
        spinner(11., color).into_any_element()
    } else if failed {
        icon(IconName::XCircle, 12.)
            .text_color(color)
            .into_any_element()
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
