//! The commands that have keys, their defaults, and binding the keys the user picked.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{Action, App, KeyBinding, KeyBindingContextPredicate, KeyDownEvent, NoAction};
use patchwork_core::Core;

/// Where the user's keys are kept: a command's key, or null for one they unbound.
pub(super) const STATE_KEY: &str = "keybindings";

/// The pages of the pane, in order.
pub(super) const PAGES: [&str; 6] = ["Main", "Panel", "Chat", "Sidebar", "Archive", "Composer"];

/// A command: its id, page, title and what it does, and whether its key is fixed.
pub(super) struct Command {
    pub(super) id: &'static str,
    pub(super) page: &'static str,
    pub(super) title: &'static str,
    pub(super) desc: Option<&'static str>,
    pub(super) fixed: bool,
}

const fn cmd(
    id: &'static str,
    page: &'static str,
    title: &'static str,
    desc: Option<&'static str>,
) -> Command {
    Command {
        id,
        page,
        title,
        desc,
        fixed: false,
    }
}

pub(super) const fn fixed(
    id: &'static str,
    page: &'static str,
    title: &'static str,
    desc: Option<&'static str>,
) -> Command {
    Command {
        id,
        page,
        title,
        desc,
        fixed: true,
    }
}

pub(super) const COMMANDS: &[Command] = &[
    cmd(
        "palette.toggle",
        "Main",
        "Search",
        Some("Open or close search"),
    ),
    cmd(
        "chat.new",
        "Main",
        "New chat or task",
        Some("A chat, or a task in the current project"),
    ),
    cmd("settings.open", "Main", "Open settings", None),
    cmd(
        "orchestrator.show",
        "Sidebar",
        "Show orchestrator",
        Some("Opens the Orchestrator page"),
    ),
    cmd(
        "sidebar.showTasks",
        "Sidebar",
        "Show tasks",
        Some("Opens the sidebar on chats and tasks"),
    ),
    cmd(
        "sidebar.showChanges",
        "Sidebar",
        "Show source control",
        Some("Opens the sidebar on the project's changes and commits"),
    ),
    cmd("quilts.show", "Sidebar", "Show quilts", None),
    cmd(
        "sidebar.showArchived",
        "Sidebar",
        "Show archived",
        Some("Opens the sidebar on archived chats and tasks"),
    ),
    cmd(
        "terminal.toggle",
        "Panel",
        "Toggle terminal",
        Some("Opens the panel on the terminal, starting one if none is open"),
    ),
    cmd("terminal.new", "Panel", "New terminal", None),
    cmd("panel.toggle", "Panel", "Toggle panel", None),
    cmd("output.toggle", "Panel", "Toggle output", None),
    fixed("palette.close", "Main", "Close search", None),
    fixed("settings.close", "Main", "Close settings", None),
    fixed("canvas.close", "Main", "Close the file panel", None),
    fixed(
        "transcript.selectAll",
        "Chat",
        "Select all",
        Some("Selects the text of the chat shown, once you click in it"),
    ),
    fixed(
        "transcript.clearSelection",
        "Chat",
        "Clear highlight",
        Some("Unselects highlighted chat text"),
    ),
    cmd("chat.find", "Chat", "Find in chat", None),
    cmd(
        "chat.back",
        "Chat",
        "Go back",
        Some("Opens the chat shown before this one"),
    ),
    cmd(
        "chat.forward",
        "Chat",
        "Go forward",
        Some("Opens the chat you went back from"),
    ),
    fixed(
        "sidebar.selectAll",
        "Sidebar",
        "Select all",
        Some("Every chat and task shown, once you click in the list"),
    ),
    fixed(
        "sidebar.clearSelection",
        "Sidebar",
        "Clear selection",
        Some("Unselects the picked chats and tasks"),
    ),
    cmd(
        "workspace.deleteChat",
        "Sidebar",
        "Delete",
        Some(
            "The picked chats and tasks, or the open chat if none; deletes them in the archive. \
             Outside a text field",
        ),
    ),
    fixed(
        "sidebar.selectAllArchived",
        "Archive",
        "Select all",
        Some("Every archived chat, once you click in the list"),
    ),
];

/// The built-in keys: the command, its key, and whether it waits for focus outside a text field.
pub(super) const DEFAULT_RULES: &[(&str, &str, bool)] = &[
    ("palette.toggle", "mod+k", false),
    ("chat.new", "mod+t", false),
    ("chat.find", "mod+f", false),
    ("chat.back", "alt+left", false),
    ("chat.forward", "alt+right", false),
    ("settings.open", "mod+,", false),
    ("terminal.toggle", "mod+`", false),
    ("terminal.new", "mod+shift+~", false),
    ("panel.toggle", "mod+j", false),
    ("output.toggle", "mod+shift+u", false),
    ("orchestrator.show", "mod+1", false),
    ("sidebar.showTasks", "mod+2", false),
    ("sidebar.showChanges", "mod+3", false),
    ("quilts.show", "mod+4", false),
    ("sidebar.showArchived", "mod+5", false),
    ("workspace.deleteChat", "delete", true),
    ("transcript.selectAll", "mod+a", true),
    ("sidebar.selectAll", "mod+a", true),
    ("sidebar.selectAllArchived", "mod+a", true),
    ("sidebar.clearSelection", "escape", false),
    ("canvas.close", "escape", false),
    ("transcript.clearSelection", "escape", false),
    ("settings.close", "escape", false),
    ("palette.close", "escape", false),
];

/// Keys the editor and the mouse own, listed on the page they belong to.
pub(super) const FIXED_KEYS: &[(&str, &[&str], &str)] = &[
    ("Chat", &["Mouse back"], "Go back"),
    ("Chat", &["Mouse forward"], "Go forward"),
    ("Panel", &["Ctrl", "Shift", "C"], "Copy in the terminal"),
    ("Panel", &["Ctrl", "Shift", "V"], "Paste in the terminal"),
    (
        "Panel",
        &["Ctrl", "C"],
        "Copy the terminal's selection, or stop what runs",
    ),
    ("Panel", &["Ctrl", "V"], "Paste in the terminal"),
    (
        "Panel",
        &["Shift", "Insert"],
        "Paste the primary selection in the terminal",
    ),
    (
        "Panel",
        &["Ctrl", "Shift", "A"],
        "Select all in the terminal",
    ),
    ("Panel", &["Escape"], "Clear the terminal's selection"),
    (
        "Panel",
        &["Shift", "Page Up"],
        "Scroll the terminal up a page",
    ),
    (
        "Panel",
        &["Shift", "Page Down"],
        "Scroll the terminal down a page",
    ),
    (
        "Panel",
        &["Ctrl", "Shift", "Up"],
        "Scroll the terminal up a line",
    ),
    (
        "Panel",
        &["Ctrl", "Shift", "Down"],
        "Scroll the terminal down a line",
    ),
    (
        "Panel",
        &["Ctrl", "Home"],
        "Scroll to the top of the terminal",
    ),
    (
        "Panel",
        &["Ctrl", "End"],
        "Scroll to the bottom of the terminal",
    ),
    (
        "Panel",
        &["Ctrl", "Backspace"],
        "Delete the word before the cursor",
    ),
    (
        "Panel",
        &["Ctrl", "Delete"],
        "Delete the word after the cursor",
    ),
    (
        "Sidebar",
        &["Ctrl", "Click"],
        "Select or unselect a chat or task",
    ),
    (
        "Archive",
        &["Ctrl", "Click"],
        "Select or unselect an archived chat",
    ),
    ("Composer", &["Enter"], "Send the message"),
    ("Composer", &["Shift", "Enter"], "New line"),
    ("Composer", &["Ctrl", "Z"], "Undo"),
    ("Composer", &["Ctrl", "Shift", "Z"], "Redo"),
    (
        "Composer",
        &["Up", "Down"],
        "Recall earlier messages from the first or last line",
    ),
    (
        "Composer",
        &["Alt", "Up"],
        "Edit the latest queued message, with an empty composer",
    ),
    (
        "Composer",
        &["Ctrl", "Enter"],
        "Steer with the queued message; the composer draft is not sent",
    ),
];

pub(super) type Overrides = HashMap<String, Option<String>>;

pub(super) fn load(core: &Core) -> Overrides {
    let mut overrides: Overrides = core
        .state(STATE_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    // A key set for the Quilts page before it was named that keeps working.
    if let Some(key) = overrides.remove("artifacts.show") {
        overrides.entry("quilts.show".to_owned()).or_insert(key);
    }
    overrides
}

/// A command's key: the user's, or the built-in one.
pub(super) fn key_for(overrides: &Overrides, id: &str) -> Option<String> {
    match overrides.get(id) {
        Some(key) => key.clone(),
        None => DEFAULT_RULES
            .iter()
            .rev()
            .find(|(c, _, _)| *c == id)
            .map(|(_, k, _)| k.to_string()),
    }
}

/// A command's key as the palette shows it, "Ctrl T", or None when it has none.
pub fn key_label(core: &Core, id: &str) -> Option<String> {
    key_for(&load(core), id).map(|key| format_key(&key).join(" "))
}

fn is_fixed(id: &str) -> bool {
    COMMANDS.iter().any(|c| c.id == id && c.fixed)
}

/// A stored key as the parts to show: `["Ctrl", "Shift", "K"]`.
pub(super) fn format_key(key: &str) -> Vec<String> {
    key.split('+')
        .map(|part| match part {
            "mod" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "escape" => "Esc".to_string(),
            "up" => "Up".to_string(),
            "down" => "Down".to_string(),
            "left" => "Left".to_string(),
            "right" => "Right".to_string(),
            "plus" => "+".to_string(),
            p => crate::ui::display_title(p),
        })
        .collect()
}

/// The key pressed, in the stored shape: "mod+shift+k". None for a bare modifier.
pub(super) fn key_from_event(e: &KeyDownEvent) -> Option<String> {
    let k = &e.keystroke;
    let name = k.key.as_str();
    if matches!(
        name,
        "control" | "shift" | "alt" | "platform" | "function" | "capslock"
    ) {
        return None;
    }
    let mut parts = Vec::new();
    if k.modifiers.control || k.modifiers.platform {
        parts.push("mod");
    }
    if k.modifiers.alt {
        parts.push("alt");
    }
    if k.modifiers.shift {
        parts.push("shift");
    }
    parts.push(if name == "+" { "plus" } else { name });
    Some(parts.join("+"))
}

/// Why `key` cannot run `command`, if it cannot.
pub(super) fn key_problem(command: &str, key: &str) -> Option<String> {
    if DEFAULT_RULES
        .iter()
        .any(|(c, k, _)| *k == key && is_fixed(c))
    {
        return Some(format!(
            "{} is an app-wide shortcut and cannot be reassigned",
            format_key(key).join("+")
        ));
    }
    let parts: Vec<&str> = key.split('+').collect();
    let bare = parts.last().copied().unwrap_or_default();
    let printable = bare.chars().count() == 1 || bare == "space" || bare == "plus";
    let modified = parts.contains(&"mod") || parts.contains(&"alt");
    if !printable || modified {
        return None;
    }
    let outside_text = DEFAULT_RULES
        .iter()
        .any(|(c, _, outside)| *c == command && *outside);
    (!outside_text)
        .then(|| "This shortcut works while typing, so it needs Ctrl, Cmd or Alt".to_string())
}

/// The other commands on the same key.
pub(super) fn conflicts(overrides: &Overrides, id: &str) -> Vec<&'static Command> {
    let Some(mine) = key_for(overrides, id) else {
        return Vec::new();
    };
    let outside = |c: &str| DEFAULT_RULES.iter().any(|(r, _, o)| *r == c && *o);
    COMMANDS
        .iter()
        .filter(|c| c.id != id && key_for(overrides, c.id).as_deref() == Some(mine.as_str()))
        .filter(|c| !(outside(c.id) && outside(id)) || mine != "mod+a")
        .collect()
}

/// A stored key as GPUI writes it: "mod+shift+k" is "ctrl-shift-k".
fn gpui_key(key: &str) -> String {
    key.split('+')
        .map(|p| match p {
            "mod" => "ctrl",
            "plus" => "+",
            p => p,
        })
        .collect::<Vec<_>>()
        .join("-")
}

/// The action a command runs in this window, for the commands it has.
fn action_for(id: &str) -> Option<Box<dyn Action>> {
    Some(match id {
        "palette.toggle" => Box::new(crate::TogglePalette),
        "chat.new" => Box::new(crate::NewChat),
        "chat.find" => Box::new(crate::FindInChat),
        "chat.back" => Box::new(crate::GoBack),
        "chat.forward" => Box::new(crate::GoForward),
        "settings.open" => Box::new(crate::OpenSettings),
        "orchestrator.show" => Box::new(crate::ShowOrchestrator),
        "sidebar.showTasks" => Box::new(crate::ShowTasks),
        "sidebar.showChanges" => Box::new(crate::ShowChanges),
        "quilts.show" => Box::new(crate::ShowQuilts),
        "sidebar.showArchived" => Box::new(crate::ShowArchived),
        "terminal.toggle" => Box::new(crate::ToggleTerminal),
        "terminal.new" => Box::new(crate::NewTerminal),
        "panel.toggle" => Box::new(crate::TogglePanel),
        "output.toggle" => Box::new(crate::ToggleOutput),
        "workspace.deleteChat" => Box::new(crate::DeleteChat),
        _ => return None,
    })
}

/// The commands whose keys a focused terminal leaves to the app; every other key goes to the
/// shell.
const SKIP_SHELL: &[&str] = &[
    "terminal.toggle",
    "terminal.new",
    "panel.toggle",
    "output.toggle",
    "palette.toggle",
    "settings.open",
];

/// Builds the window's keymap.
///
/// It holds moving focus, quitting, the text fields' keys, and each command's key as the user
/// left it. In a terminal, Tab and the commands' keys other than the panel's,
/// search's and settings' go to the shell.
pub fn bind_keys(core: &Core, cx: &mut App) {
    let overrides = load(core);
    cx.clear_key_bindings();
    cx.bind_keys([
        KeyBinding::new("tab", crate::FocusNext, None),
        KeyBinding::new("shift-tab", crate::FocusPrev, None),
        KeyBinding::new("ctrl-q", crate::Quit, None),
        KeyBinding::new("tab", NoAction, Some("Terminal")),
        KeyBinding::new("shift-tab", NoAction, Some("Terminal")),
    ]);
    let terminal = KeyBindingContextPredicate::parse("Terminal")
        .ok()
        .map(Rc::new);
    let mut bindings = Vec::new();
    for c in COMMANDS {
        let (Some(key), Some(action)) = (key_for(&overrides, c.id), action_for(c.id)) else {
            continue;
        };
        match KeyBinding::load(&gpui_key(&key), action, None, false, None, cx) {
            Ok(binding) => bindings.push(binding),
            Err(e) => {
                log::warn!(
                    key:%, command:% = c.id, error:% = e;
                    "could not bind {{key}} to {{command}}: {{error}}"
                );
            }
        }
        if !SKIP_SHELL.contains(&c.id)
            && let Ok(binding) = KeyBinding::load(
                &gpui_key(&key),
                Box::new(NoAction),
                terminal.clone(),
                false,
                None,
                cx,
            )
        {
            bindings.push(binding);
        }
    }
    cx.bind_keys(bindings);
    cx.bind_keys(crate::ui::text_area::bindings());
    cx.bind_keys(crate::ui::selection::bindings());
}
