//! The commands that have keys, their defaults, and binding the keys the user picked.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{Action, App, KeyBinding, KeyBindingContextPredicate, Keystroke, NoAction};
use patchwork_backend::Core;

/// Where the user's keys are kept: a command's keys, apart by spaces, or null for one they
/// unbound.
pub(super) const STATE_KEY: &str = "keybindings";

/// The pages of the pane, in order.
pub(super) const PAGES: [&str; 8] = [
    "Main", "Editing", "Panes", "Panel", "Chat", "Sidebar", "Archive", "Composer",
];

/// The editing keys every text field and selection in the app answers to, as stored keys. No
/// command may take them, so copy and paste work the same everywhere.
const EDITING_KEYS: &[&str] = &["mod+c", "mod+x", "mod+v", "mod+z", "mod+y"];

/// A command: its id, page, title and what it does, and whether its key is fixed.
#[derive(Debug)]
pub(super) struct Command {
    pub(super) id: &'static str,
    pub(super) page: &'static str,
    pub(super) title: &'static str,
    pub(super) desc: Option<&'static str>,
    pub(super) fixed: bool,
}

/// Every command in the shortcuts list, as its id and title, for settings search.
pub fn searchable() -> impl Iterator<Item = (&'static str, &'static str)> {
    COMMANDS
        .iter()
        .filter(|c| !c.fixed)
        .map(|c| (c.id, c.title))
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
    cmd(
        "pane.splitRight",
        "Panes",
        "Split right",
        Some("Moves the open chat to a new pane on the right, or adds an empty one"),
    ),
    cmd(
        "pane.splitDown",
        "Panes",
        "Split down",
        Some("Moves the open chat to a new pane below, or adds an empty one"),
    ),
    cmd("pane.focusLeft", "Panes", "Focus pane to the left", None),
    cmd("pane.focusRight", "Panes", "Focus pane to the right", None),
    cmd("pane.focusUp", "Panes", "Focus pane above", None),
    cmd("pane.focusDown", "Panes", "Focus pane below", None),
    cmd(
        "pane.focus1",
        "Panes",
        "Focus first pane",
        Some("Panes count in reading order, left to right and top to bottom"),
    ),
    cmd("pane.focus2", "Panes", "Focus second pane", None),
    cmd("pane.focus3", "Panes", "Focus third pane", None),
    cmd("pane.focus4", "Panes", "Focus fourth pane", None),
    cmd(
        "pane.nextTab",
        "Panes",
        "Next tab",
        Some("The next tab in the focused pane"),
    ),
    cmd(
        "pane.prevTab",
        "Panes",
        "Previous tab",
        Some("The previous tab in the focused pane"),
    ),
    cmd(
        "pane.reopenTab",
        "Panes",
        "Reopen closed tab",
        Some("The tab closed last, if its chat is still listed"),
    ),
    cmd(
        "pane.moveTabLeft",
        "Panes",
        "Move tab left",
        Some("To the pane on the left, splitting when there is none"),
    ),
    cmd(
        "pane.moveTabRight",
        "Panes",
        "Move tab right",
        Some("To the pane on the right, splitting when there is none"),
    ),
    cmd(
        "pane.moveTabUp",
        "Panes",
        "Move tab up",
        Some("To the pane above, splitting when there is none"),
    ),
    cmd(
        "pane.moveTabDown",
        "Panes",
        "Move tab down",
        Some("To the pane below, splitting when there is none"),
    ),
    cmd(
        "pane.swapLeft",
        "Panes",
        "Swap with pane left",
        Some("Trades places with the pane on the left, keeping each place's size"),
    ),
    cmd(
        "pane.swapRight",
        "Panes",
        "Swap with pane right",
        Some("Trades places with the pane on the right, keeping each place's size"),
    ),
    cmd(
        "pane.swapUp",
        "Panes",
        "Swap with pane above",
        Some("Trades places with the pane above, keeping each place's size"),
    ),
    cmd(
        "pane.swapDown",
        "Panes",
        "Swap with pane below",
        Some("Trades places with the pane below, keeping each place's size"),
    ),
    cmd(
        "pane.maximize",
        "Panes",
        "Maximize pane",
        Some("Shows only the focused pane, or every pane again"),
    ),
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
        "chat.closeTab",
        "Chat",
        "Close tab",
        Some("Closes the open chat's tab, when it has one"),
    ),
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
    ("chat.closeTab", "mod+w", false),
    ("chat.back", "alt+left", false),
    ("chat.forward", "alt+right", false),
    ("settings.open", "mod+,", false),
    ("pane.splitRight", "mod+\\", false),
    ("pane.focusLeft", "mod+alt+left", false),
    ("pane.focusRight", "mod+alt+right", false),
    ("pane.focusUp", "mod+alt+up", false),
    ("pane.focusDown", "mod+alt+down", false),
    ("pane.focus1", "alt+1", false),
    ("pane.focus2", "alt+2", false),
    ("pane.focus3", "alt+3", false),
    ("pane.focus4", "alt+4", false),
    ("pane.nextTab", "mod+tab", false),
    ("pane.prevTab", "mod+shift+tab", false),
    ("pane.reopenTab", "mod+shift+t", false),
    ("pane.moveTabLeft", "mod+alt+shift+left", false),
    ("pane.moveTabRight", "mod+alt+shift+right", false),
    ("pane.moveTabUp", "mod+alt+shift+up", false),
    ("pane.moveTabDown", "mod+alt+shift+down", false),
    ("pane.maximize", "mod+shift+enter", false),
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
    ("Editing", &["Ctrl", "C"], "Copy"),
    ("Editing", &["Ctrl", "X"], "Cut"),
    ("Editing", &["Ctrl", "V"], "Paste"),
    ("Editing", &["Ctrl", "Z"], "Undo"),
    ("Editing", &["Ctrl", "Y"], "Redo"),
    ("Editing", &["Ctrl", "A"], "Select all"),
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

/// The user's keys by command: its keys apart by spaces, or None when they took them all off.
/// Commands left out keep their built-in keys.
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
    // The Issues view is gone, and its key with it.
    overrides.remove("sidebar.showIssues");
    overrides
}

/// A command's built-in keys.
pub(super) fn default_keys(id: &str) -> Vec<String> {
    DEFAULT_RULES
        .iter()
        .filter(|(c, _, _)| *c == id)
        .map(|(_, k, _)| (*k).to_owned())
        .collect()
}

/// A command's keys: the user's, or the built-in ones.
pub(super) fn keys_for(overrides: &Overrides, id: &str) -> Vec<String> {
    match overrides.get(id) {
        Some(Some(keys)) => keys.split_whitespace().map(str::to_owned).collect(),
        Some(None) => Vec::new(),
        None => default_keys(id),
    }
}

/// A command's first key, the one menus and search show.
pub(super) fn key_for(overrides: &Overrides, id: &str) -> Option<String> {
    keys_for(overrides, id).into_iter().next()
}

/// Gives command `id` the keys `keys`, leaving it out when they are its built-in ones.
pub(super) fn store_keys(overrides: &mut Overrides, id: &str, keys: &[String]) {
    if keys == default_keys(id).as_slice() {
        overrides.remove(id);
    } else {
        let kept = (!keys.is_empty()).then(|| keys.join(" "));
        overrides.insert(id.to_owned(), kept);
    }
}

/// The command with id `id`.
pub(super) fn command(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A command's key as the palette shows it, "Ctrl T", or None when it has none.
pub fn key_label(core: &Core, id: &str) -> Option<String> {
    key_for(&load(core), id).map(|key| format_key(&key).join(" "))
}

pub(super) fn is_fixed(id: &str) -> bool {
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
            p => patchwork_ui_markdown::display_title(p),
        })
        .collect()
}

/// The key pressed, in the stored shape: "mod+shift+k". None for a bare modifier.
pub(super) fn key_from_keystroke(k: &Keystroke) -> Option<String> {
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
    if EDITING_KEYS.contains(&key) {
        return Some(format!(
            "{} copies, pastes or undoes everywhere and cannot be reassigned",
            format_key(key).join("+")
        ));
    }
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

/// Whether command `other`, also on `key`, clashes with command `id`. Fixed commands on one key
/// work in different places, so they never clash with each other, and neither do the Select all
/// keys that each wait for their own list.
pub(super) fn clashes(id: &str, other: &Command, key: &str) -> bool {
    let outside = |c: &str| DEFAULT_RULES.iter().any(|(r, _, o)| *r == c && *o);
    let both_fixed = other.fixed && is_fixed(id);
    let own_lists = outside(other.id) && outside(id) && key == "mod+a";
    other.id != id && !both_fixed && !own_lists
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

/// The commands of page `page` that run an action, as their ids and titles, for search to list.
#[must_use]
pub fn page_commands(page: &str) -> Vec<(&'static str, &'static str)> {
    COMMANDS
        .iter()
        .filter(|c| c.page == page && action_for(c.id).is_some())
        .map(|c| (c.id, c.title))
        .collect()
}

/// The action command `id` runs, for search to send.
#[must_use]
pub fn command_action(id: &str) -> Option<Box<dyn Action>> {
    action_for(id)
}

/// The action a command runs in this window, for the commands it has.
fn action_for(id: &str) -> Option<Box<dyn Action>> {
    Some(match id {
        "palette.toggle" => Box::new(patchwork_ui_components::TogglePalette),
        "chat.new" => Box::new(patchwork_ui_components::NewChat),
        "chat.find" => Box::new(patchwork_ui_components::FindInChat),
        "chat.closeTab" => Box::new(patchwork_ui_components::CloseChatTab),
        "chat.back" => Box::new(patchwork_ui_components::GoBack),
        "chat.forward" => Box::new(patchwork_ui_components::GoForward),
        "settings.open" => Box::new(patchwork_ui_components::OpenSettings),
        "orchestrator.show" => Box::new(patchwork_ui_components::ShowOrchestrator),
        "sidebar.showTasks" => Box::new(patchwork_ui_components::ShowTasks),
        "sidebar.showChanges" => Box::new(patchwork_ui_components::ShowChanges),
        "quilts.show" => Box::new(patchwork_ui_components::ShowQuilts),
        "sidebar.showArchived" => Box::new(patchwork_ui_components::ShowArchived),
        "terminal.toggle" => Box::new(patchwork_ui_components::ToggleTerminal),
        "terminal.new" => Box::new(patchwork_ui_components::NewTerminal),
        "panel.toggle" => Box::new(patchwork_ui_components::TogglePanel),
        "output.toggle" => Box::new(patchwork_ui_components::ToggleOutput),
        "workspace.deleteChat" => Box::new(patchwork_ui_components::DeleteChat),
        "pane.splitRight" => Box::new(patchwork_ui_components::SplitPaneRight),
        "pane.splitDown" => Box::new(patchwork_ui_components::SplitPaneDown),
        "pane.focusLeft" => Box::new(patchwork_ui_components::FocusPaneLeft),
        "pane.focusRight" => Box::new(patchwork_ui_components::FocusPaneRight),
        "pane.focusUp" => Box::new(patchwork_ui_components::FocusPaneUp),
        "pane.focusDown" => Box::new(patchwork_ui_components::FocusPaneDown),
        "pane.focus1" => Box::new(patchwork_ui_components::FocusPane1),
        "pane.focus2" => Box::new(patchwork_ui_components::FocusPane2),
        "pane.focus3" => Box::new(patchwork_ui_components::FocusPane3),
        "pane.focus4" => Box::new(patchwork_ui_components::FocusPane4),
        "pane.nextTab" => Box::new(patchwork_ui_components::NextTab),
        "pane.prevTab" => Box::new(patchwork_ui_components::PrevTab),
        "pane.reopenTab" => Box::new(patchwork_ui_components::ReopenTab),
        "pane.moveTabLeft" => Box::new(patchwork_ui_components::MoveTabLeft),
        "pane.moveTabRight" => Box::new(patchwork_ui_components::MoveTabRight),
        "pane.moveTabUp" => Box::new(patchwork_ui_components::MoveTabUp),
        "pane.moveTabDown" => Box::new(patchwork_ui_components::MoveTabDown),
        "pane.swapLeft" => Box::new(patchwork_ui_components::SwapPaneLeft),
        "pane.swapRight" => Box::new(patchwork_ui_components::SwapPaneRight),
        "pane.swapUp" => Box::new(patchwork_ui_components::SwapPaneUp),
        "pane.swapDown" => Box::new(patchwork_ui_components::SwapPaneDown),
        "pane.maximize" => Box::new(patchwork_ui_components::MaximizePane),
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
/// It holds moving focus, quitting (Ctrl+Q or Meta+Q), the text fields' keys, and each command's key as the user
/// left it. In a terminal, Tab and the commands' keys other than the panel's,
/// search's and settings' go to the shell.
pub fn bind_keys(core: &Core, cx: &mut App) {
    let overrides = load(core);
    cx.clear_key_bindings();
    cx.bind_keys([
        KeyBinding::new("tab", patchwork_ui_components::FocusNext, None),
        KeyBinding::new("shift-tab", patchwork_ui_components::FocusPrev, None),
        KeyBinding::new("ctrl-q", patchwork_ui_components::Quit, None),
        // Meta on Linux, Command on macOS, the Windows key on Windows.
        KeyBinding::new("super-q", patchwork_ui_components::Quit, None),
        KeyBinding::new("tab", NoAction, Some("Terminal")),
        KeyBinding::new("shift-tab", NoAction, Some("Terminal")),
    ]);
    let terminal = KeyBindingContextPredicate::parse("Terminal")
        .ok()
        .map(Rc::new);
    let mut bindings = Vec::new();
    for c in COMMANDS {
        for key in keys_for(&overrides, c.id) {
            bind_one(c, &key, terminal.as_ref(), &mut bindings, cx);
        }
    }
    cx.bind_keys(bindings);
    cx.bind_keys(patchwork_ui_editor::bindings());
    cx.bind_keys(patchwork_ui_markdown::selection::bindings());
}

/// Binds `key` to command `c`'s action, and in a terminal leaves it to the shell unless the
/// command is one the terminal lets through.
fn bind_one(
    c: &Command,
    key: &str,
    terminal: Option<&Rc<KeyBindingContextPredicate>>,
    bindings: &mut Vec<KeyBinding>,
    cx: &App,
) {
    let Some(action) = action_for(c.id) else {
        return;
    };
    match KeyBinding::load(&gpui_key(key), action, None, false, None, cx) {
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
            &gpui_key(key),
            Box::new(NoAction),
            terminal.cloned(),
            false,
            None,
            cx,
        )
    {
        bindings.push(binding);
    }
}
