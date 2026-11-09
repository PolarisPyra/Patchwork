//! Settings → Shortcuts (`app/KeybindingsPane.tsx`, `lib/shortcutDefaults.ts`): every command a
//! key can run, by page, with its key. Clicking one records the next key pressed as its new one;
//! the changes are kept in the app state under `keybindings`, in the web UI's shape, and the
//! window's keymap is built from them with [`bind_keys`].

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    Action, AnyElement, App, Context, Entity, Focusable, FontWeight, KeyBinding,
    KeyBindingContextPredicate, KeyDownEvent, NoAction, Role, SharedString, Window, div,
    prelude::*, px,
};
use patchwork_server::Server;

use crate::chat::text_area::{Changed, TextArea};
use crate::settings::SettingsDialog;
use crate::settings::sigils::SIGILS_PAGE;
use crate::theme::{FS_MD, FS_SM, FS_XS, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, tooltip};

/// Where the user's keys are kept: a command's key, or null for one they unbound.
const STATE_KEY: &str = "keybindings";

/// The pages of the pane, in order.
const PAGES: [&str; 6] = ["Main", "Panel", "Chat", "Sidebar", "Archive", "Composer"];

/// A command: its id, page, title and what it does, and whether its key is fixed.
struct Command {
    id: &'static str,
    page: &'static str,
    title: &'static str,
    desc: Option<&'static str>,
    fixed: bool,
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

const fn fixed(
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

const COMMANDS: &[Command] = &[
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
    cmd(
        "chat.newHere",
        "Main",
        "New task here",
        Some("In the selected chat's project or folder; a chat if none is selected"),
    ),
    cmd("settings.open", "Main", "Open settings", None),
    cmd(
        "composer.focus",
        "Main",
        "Jump to the composer",
        Some("Outside a text field"),
    ),
    cmd(
        "sidebar.toggle",
        "Main",
        "Toggle sidebar",
        Some("Closes the sidebar, or reopens its last view; outside a text field"),
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
    cmd(
        "transcript.quoteSelection",
        "Chat",
        "Quote in reply",
        Some("Adds highlighted chat text to the message box as a quote"),
    ),
    cmd("chat.find", "Chat", "Find in chat", None),
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
        "workspace.closeChat",
        "Sidebar",
        "Remove selected",
        Some("The picked chats and tasks, and the open chat while the composer is not active"),
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
const DEFAULT_RULES: &[(&str, &str, bool)] = &[
    ("palette.toggle", "mod+k", false),
    ("chat.new", "mod+t", false),
    ("chat.newHere", "mod+n", false),
    ("chat.find", "mod+f", false),
    ("settings.open", "mod+,", false),
    ("terminal.toggle", "mod+`", false),
    ("terminal.new", "mod+shift+~", false),
    ("panel.toggle", "mod+j", false),
    ("output.toggle", "mod+shift+u", false),
    ("composer.focus", "tab", true),
    ("sidebar.toggle", "space", true),
    ("sidebar.showTasks", "mod+1", false),
    ("sidebar.showChanges", "mod+2", false),
    ("sidebar.showArchived", "mod+3", false),
    ("workspace.closeChat", "mod+w", false),
    ("workspace.deleteChat", "delete", true),
    ("transcript.selectAll", "mod+a", true),
    ("sidebar.selectAll", "mod+a", true),
    ("sidebar.selectAllArchived", "mod+a", true),
    ("sidebar.clearSelection", "escape", false),
    ("canvas.close", "escape", false),
    ("transcript.clearSelection", "escape", false),
    ("transcript.quoteSelection", "r", true),
    ("settings.close", "escape", false),
    ("palette.close", "escape", false),
];

/// Keys the editor and the mouse own, listed on the page they belong to.
const FIXED_KEYS: &[(&str, &[&str], &str)] = &[
    ("Panel", &["Ctrl", "Shift", "C"], "Copy in the terminal"),
    ("Panel", &["Ctrl", "Shift", "V"], "Paste in the terminal"),
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

type Overrides = HashMap<String, Option<String>>;

fn load(server: &Server) -> Overrides {
    server
        .store
        .state(STATE_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// A command's key: the user's, or the built-in one.
fn key_for(overrides: &Overrides, id: &str) -> Option<String> {
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
pub fn key_label(server: &Server, id: &str) -> Option<String> {
    key_for(&load(server), id).map(|key| format_key(&key).join(" "))
}

fn is_fixed(id: &str) -> bool {
    COMMANDS.iter().any(|c| c.id == id && c.fixed)
}

/// A stored key as the parts to show: ["Ctrl", "Shift", "K"].
fn format_key(key: &str) -> Vec<String> {
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
            p if p.chars().count() == 1 => p.to_uppercase(),
            p => {
                let mut c = p.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        })
        .collect()
}

/// The key pressed, in the stored shape: "mod+shift+k". None for a bare modifier.
fn key_from_event(e: &KeyDownEvent) -> Option<String> {
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
fn key_problem(command: &str, key: &str) -> Option<String> {
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
fn conflicts(overrides: &Overrides, id: &str) -> Vec<&'static Command> {
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
        "settings.open" => Box::new(crate::OpenSettings),
        "sidebar.showTasks" => Box::new(crate::ShowTasks),
        "sidebar.showChanges" => Box::new(crate::ShowChanges),
        "sidebar.showArchived" => Box::new(crate::ShowArchived),
        "terminal.toggle" => Box::new(crate::ToggleTerminal),
        "terminal.new" => Box::new(crate::NewTerminal),
        "panel.toggle" => Box::new(crate::TogglePanel),
        "output.toggle" => Box::new(crate::ToggleOutput),
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

/// Builds the window's keymap: moving focus, quitting, the text fields' keys, and each command's
/// key as the user left it. In a terminal, Tab and the commands' keys other than the panel's,
/// search's and settings' go to the shell.
pub fn bind_keys(server: &Server, cx: &mut App) {
    let overrides = load(server);
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
            Err(e) => log::warn!("could not bind {key} to {}: {e}", c.id),
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
    cx.bind_keys(crate::chat::text_area::bindings());
}

/// The pane's state: the page, the search, and the command whose key is being recorded.
pub struct ShortcutsState {
    page: &'static str,
    search: Option<Entity<TextArea>>,
    overrides: Option<Overrides>,
    pub recording: Option<&'static str>,
    pub recording_sigil: Option<crate::sigils::SigilKind>,
    pub refused: Option<String>,
    fixed_note: Option<&'static str>,
}

impl Default for ShortcutsState {
    fn default() -> Self {
        Self {
            page: PAGES[0],
            search: None,
            overrides: None,
            recording: None,
            recording_sigil: None,
            refused: None,
            fixed_note: None,
        }
    }
}

impl SettingsDialog {
    fn save_keys(&mut self, overrides: Overrides, cx: &mut Context<Self>) {
        let json = serde_json::to_string(&overrides).unwrap_or_default();
        if let Err(e) = self.server.store.set_state(STATE_KEY, &json) {
            self.error = Some(e);
        }
        self.shortcuts.overrides = Some(overrides);
        bind_keys(&self.server, cx);
        cx.notify();
    }

    fn set_key(&mut self, id: &str, key: Option<String>, cx: &mut Context<Self>) {
        let mut overrides = self.shortcuts.overrides.clone().unwrap_or_default();
        let default = DEFAULT_RULES
            .iter()
            .find(|(c, _, _)| *c == id)
            .map(|(_, k, _)| k.to_string());
        if key == default {
            overrides.remove(id);
        } else {
            overrides.insert(id.to_string(), key);
        }
        self.save_keys(overrides, cx);
    }

    /// Takes the key pressed while one is being recorded. True when it was used.
    pub fn record_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.shortcuts.recording else {
            return false;
        };
        let Some(key) = key_from_event(e) else {
            return true;
        };
        if key == "escape" {
            self.shortcuts.recording = None;
        } else if let Some(problem) = key_problem(id, &key) {
            self.shortcuts.refused = Some(problem);
        } else {
            self.shortcuts.recording = None;
            self.set_key(id, Some(key), cx);
        }
        cx.notify();
        true
    }
}

pub fn kbd(parts: &[String], t: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .children(parts.iter().map(|p| {
            div()
                .min_w(px(20.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .px(px(5.))
                .rounded(px(RADIUS_SM))
                .bg(t.bg_selected_muted)
                .text_size(px(FS_XS))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text_muted)
                .child(p.clone())
        }))
}

pub fn render(
    d: &mut SettingsDialog,
    window: &Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if d.shortcuts.overrides.is_none() {
        d.shortcuts.overrides = Some(load(&d.server));
    }
    let search = match &d.shortcuts.search {
        Some(search) => search.clone(),
        None => {
            let search = cx.new(|cx| {
                let mut area = TextArea::new("Search by name or key", "Search shortcuts", cx);
                area.single_line = true;
                area.text_color = t.text;
                area.placeholder_color = crate::theme::input_placeholder();
                area.selection_color = t.accent.opacity(0.3);
                area
            });
            cx.subscribe(&search, |_, _, _: &Changed, cx| cx.notify())
                .detach();
            d.shortcuts.search = Some(search.clone());
            search
        }
    };
    let query = search.read(cx).content.trim().to_lowercase();
    let searching = !query.is_empty();
    let search_focused = search
        .read(cx)
        .focus_handle(cx)
        .contains_focused(window, cx);
    let overrides = d.shortcuts.overrides.clone().unwrap_or_default();
    let page = d.shortcuts.page;
    let matches = |texts: &[&str]| texts.iter().any(|x| x.to_lowercase().contains(&query));

    let mut sections = Vec::new();
    for p in PAGES.iter().filter(|p| searching || **p == page) {
        let commands: Vec<&Command> = COMMANDS
            .iter()
            .filter(|c| c.page == *p && !c.fixed)
            .filter(|c| {
                let key = key_for(&overrides, c.id)
                    .map(|k| format_key(&k).join(" "))
                    .unwrap_or_default();
                !searching || matches(&[c.title, c.desc.unwrap_or_default(), p, &key])
            })
            .collect();
        let keys: Vec<_> = FIXED_KEYS
            .iter()
            .filter(|(fp, keys, action)| {
                *fp == *p && (!searching || matches(&[action, p, &keys.join(" ")]))
            })
            .collect();
        if !commands.is_empty() || !keys.is_empty() {
            sections.push((*p, commands, keys));
        }
    }
    let has_commands = sections.iter().any(|(_, c, _)| !c.is_empty());
    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);

    let sections: Vec<_> = sections
        .into_iter()
        .enumerate()
        .map(|(i, (p, commands, keys))| {
            let rows =
                commands.into_iter().map(|c| {
                    let key = key_for(&overrides, c.id);
                    let changed = overrides.contains_key(c.id);
                    let recording = d.shortcuts.recording == Some(c.id);
                    let clash = conflicts(&overrides, c.id);
                    let id = c.id;
                    let group = SharedString::from(format!("shortcut-{id}"));
                    let small = |name: &str, label: String, glyph: &str| {
                        button(SharedString::from(format!("{name}-{id}")), label)
                            .size(px(24.))
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon(glyph, 12.))
                    };
                    div()
                        .group(group.clone())
                        .border_b_1()
                        .border_color(t.border)
                        .py(px(6.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .child(div().text_color(t.text).child(c.title))
                                        .children(c.desc.map(|desc| {
                                            div().text_size(px(FS_SM)).text_color(muted).child(desc)
                                        })),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .justify_end()
                                        .when(key.is_some() && !recording, |r| {
                                            r.child(
                                                small(
                                                    "unbind",
                                                    format!("Remove shortcut for {}", c.title),
                                                    "x",
                                                )
                                                .opacity(0.)
                                                .group_hover(group.clone(), |s| s.opacity(1.))
                                                .focus(|s| s.opacity(1.))
                                                .tooltip(tooltip("Remove shortcut"))
                                                .on_click(cx.listener(move |d, _, _, cx| {
                                                    d.set_key(id, None, cx)
                                                })),
                                            )
                                        })
                                        .when(changed && !recording, |r| {
                                            r.child(
                                                small(
                                                    "reset",
                                                    format!("Reset shortcut for {}", c.title),
                                                    "arrow-counter-clockwise",
                                                )
                                                .tooltip(tooltip("Reset to default"))
                                                .on_click(cx.listener(move |d, _, _, cx| {
                                                    let mut overrides = d
                                                        .shortcuts
                                                        .overrides
                                                        .clone()
                                                        .unwrap_or_default();
                                                    overrides.remove(id);
                                                    d.save_keys(overrides, cx);
                                                })),
                                            )
                                        }),
                                )
                                .child(
                                    button(
                                        SharedString::from(format!("change-{id}")),
                                        format!("Change shortcut for {}", c.title),
                                    )
                                    .rounded(px(RADIUS_SM))
                                    .border_1()
                                    .px(px(8.))
                                    .py(px(4.))
                                    .map(|b| {
                                        if recording {
                                            b.border_color(t.accent).bg(t.bg_accent_soft)
                                        } else {
                                            b.border_color(gpui::transparent_black())
                                                .hover(move |s| s.bg(hover))
                                        }
                                    })
                                    .tooltip(tooltip("Change shortcut"))
                                    .map(|b| match (&key, recording) {
                                        (_, true) => b.child(
                                            div()
                                                .text_size(px(FS_SM))
                                                .text_color(t.accent_text)
                                                .child("Press keys…"),
                                        ),
                                        (Some(key), false) => b.child(kbd(&format_key(key), &t)),
                                        (None, false) => b.child(
                                            div()
                                                .text_size(px(FS_SM))
                                                .text_color(t.text_subtle)
                                                .child("Unbound"),
                                        ),
                                    })
                                    .on_click(cx.listener(
                                        move |d, _, window, cx| {
                                            d.shortcuts.refused = None;
                                            d.shortcuts.recording_sigil = None;
                                            d.shortcuts.recording =
                                                if d.shortcuts.recording == Some(id) {
                                                    None
                                                } else {
                                                    Some(id)
                                                };
                                            window.focus(&d.focus, cx);
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                        .when(recording, |r| {
                            r.children(d.shortcuts.refused.clone().map(|e| {
                                div()
                                    .mt(px(2.))
                                    .text_right()
                                    .text_size(px(FS_SM))
                                    .text_color(t.danger)
                                    .child(e)
                            }))
                        })
                        .when(!recording && !clash.is_empty(), |r| {
                            let names: Vec<String> = clash
                                .iter()
                                .map(|x| format!("{}: {}", x.page, x.title))
                                .collect();
                            r.child(
                                div()
                                    .mt(px(2.))
                                    .text_right()
                                    .text_size(px(FS_SM))
                                    .text_color(t.warning)
                                    .child(format!("Also used by {}", names.join(", "))),
                            )
                        })
                });
            let fixed_rows = keys.into_iter().map(|&(_, keys, action)| {
                let noted = d.shortcuts.fixed_note == Some(action);
                let parts: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
                div()
                    .border_b_1()
                    .border_color(t.border)
                    .py(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().min_w_0().flex_1().text_color(muted).child(action))
                            .child(
                                button(
                                    SharedString::from(format!("fixed-{action}")),
                                    format!("{action}: fixed shortcut"),
                                )
                                .rounded(px(RADIUS_SM))
                                .border_1()
                                .border_color(gpui::transparent_black())
                                .px(px(8.))
                                .py(px(4.))
                                .hover(move |s| s.bg(hover))
                                .tooltip(tooltip("Fixed: this shortcut cannot be changed"))
                                .child(kbd(&parts, &t))
                                .on_click(cx.listener(
                                    move |d, _, _, cx| {
                                        d.shortcuts.fixed_note =
                                            if d.shortcuts.fixed_note == Some(action) {
                                                None
                                            } else {
                                                Some(action)
                                            };
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .when(noted, |r| {
                        r.child(
                            div()
                                .mt(px(2.))
                                .text_right()
                                .text_size(px(FS_SM))
                                .text_color(t.text_subtle)
                                .child("Fixed: this shortcut cannot be changed"),
                        )
                    })
            });
            div()
                .mb(px(12.))
                .when(searching, |s| {
                    s.child(
                        div()
                            .mt(px(if i == 0 { 4. } else { 0. }))
                            .mb(px(2.))
                            .text_size(px(FS_SM))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.text_subtle)
                            .child(p),
                    )
                })
                .children(rows.collect::<Vec<_>>())
                .children(fixed_rows.collect::<Vec<_>>())
        })
        .collect();

    let sigil_rows = super::sigils::rows(d, page, &query, &t, cx);
    let has_sigils = !sigil_rows.is_empty();
    let nothing = sections.is_empty() && !has_sigils;
    // A heading's margin folds into the one above it, as the web page's block margins do.
    let sigils_first = sections.is_empty();
    let none_changed = overrides.is_empty() && !d.sigils(cx).changed();
    let tabs = PAGES.iter().chain([&SIGILS_PAGE]).map(|&p| {
        let selected = p == page;
        div()
            .id(SharedString::from(format!("page-{p}")))
            .role(Role::Tab)
            .aria_label(p)
            .aria_selected(selected)
            .tab_index(0)
            .focus_visible(crate::ui::focus_ring)
            .cursor_pointer()
            .flex()
            .h(px(28.))
            .items_center()
            .rounded(px(RADIUS_SM))
            .px(px(10.))
            .text_size(px(FS_SM))
            .map(|tab| {
                if selected {
                    tab.bg(t.bg_selected).text_color(fg)
                } else {
                    tab.text_color(muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                }
            })
            .child(p)
            .on_click(cx.listener(move |d, _, _, cx| {
                d.shortcuts.recording = None;
                d.shortcuts.recording_sigil = None;
                d.shortcuts.fixed_note = None;
                d.shortcuts.page = p;
                cx.notify();
            }))
    });
    let tabs: Vec<_> = tabs.collect();
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .mb(px(4.))
                .pb(px(4.))
                .child(
                    div()
                        .mb(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .rounded(px(RADIUS_SM))
                                .border_1()
                                .border_color(if search_focused {
                                    t.accent
                                } else {
                                    t.border_strong
                                })
                                .bg(t.bg_editor)
                                .px(px(8.))
                                .text_size(px(FS_MD))
                                .child(div().flex_1().min_w_0().child(search)),
                        )
                        .child(
                            btn(
                                "reset-all-shortcuts",
                                "Reset all",
                                BtnKind::Plain,
                                none_changed,
                                &t,
                            )
                            .child("Reset all")
                            .when(!none_changed, |b| {
                                b.on_click(cx.listener(|d, _, _, cx| {
                                    d.shortcuts.recording = None;
                                    d.shortcuts.recording_sigil = None;
                                    d.save_keys(Overrides::new(), cx);
                                    d.save_sigils(Default::default(), cx);
                                }))
                            }),
                        ),
                )
                .when(!searching, |s| {
                    s.child(
                        div()
                            .id("shortcut-pages")
                            .role(Role::TabList)
                            .aria_label("Shortcut pages")
                            .flex()
                            .gap(px(4.))
                            .children(tabs),
                    )
                }),
        )
        .when(has_commands, |s| {
            s.child(
                div()
                    .mb(px(4.))
                    .text_size(px(FS_SM))
                    .text_color(muted)
                    .child("Click a shortcut, then press the new keys. Esc cancels."),
            )
        })
        .when(has_sigils && !searching, |s| {
            s.child(
                div()
                    .mb(px(4.))
                    .text_size(px(FS_SM))
                    .text_color(muted)
                    .child("Click a sigil, then type the new one. Esc cancels."),
            )
        })
        .when(nothing, |s| {
            s.child(
                div()
                    .py(px(16.))
                    .text_color(muted)
                    .child("No shortcuts match."),
            )
        })
        .children(sections)
        .when(has_sigils, |s| {
            s.child(
                div()
                    .mb(px(12.))
                    .when(searching, |s| {
                        s.child(
                            div()
                                .mt(px(if sigils_first { 4. } else { 0. }))
                                .mb(px(2.))
                                .text_size(px(FS_SM))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.text_subtle)
                                .child(SIGILS_PAGE),
                        )
                    })
                    .children(sigil_rows),
            )
        })
        .into_any_element()
}
