//! The composer's menus (`AddContext`, `EffortPicker`, `CompactPicker`, `AccessPicker`,
//! `AgentPicker`, `ProjectSelector`): what each offers, and the keys that move through them. Each
//! opens up from its button.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FocusHandle, FontWeight, KeyDownEvent, MouseDownEvent, Role, SharedString, Window,
    div, prelude::*, px,
};
use patchwork_backend::extensions::agents::Agent;
use patchwork_backend::provider::AgentKind;
use patchwork_backend::chats::AccessMode;

use super::{Composer, Picks};
use patchwork_ui_theme::{IconName, Theme, fs_xs};
use patchwork_ui_components::{menu_item, popup};
use patchwork_ui_markdown::{effort_label, format_tokens};
use patchwork_ui_theme::icon;

/// The chips that open a menu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Model,
    Effort,
    Compact,
    Access,
    Agent,
    /// The project the chats a request starts on the Orchestrator page go in.
    Project,
    /// Queue or steer, beside the send button while a turn runs.
    Send,
    /// The "+" in the footer: attaching files or a folder, starting a mention or an action, and
    /// the Markdown to put around the selection or at the start of the line.
    Attach,
}

/// What a menu row does when picked.
#[derive(Clone)]
pub enum Pick {
    Model(AgentKind, Option<String>),
    Effort(Option<String>),
    Compact(Option<u64>),
    CompactNow,
    Access(AccessMode),
    Role(Option<String>),
    /// Whether the next send while a turn runs queues.
    SendMode(bool),
    /// The project for the chats a request starts, or none.
    Project(Option<String>),
    /// Opens the file picker for files, or for a folder when true.
    Attach(bool),
    /// The marks to put before and after the selection; with nothing after, the mark starts the
    /// line.
    Format(&'static str, &'static str),
    /// Text to type at the caret, as the "@" that starts a mention.
    Insert(&'static str),
}

/// Context sizes to compact at, each twice the one before; 0 leaves it to the agent.
const WINDOWS: [u64; 5] = [128_000, 200_000, 256_000, 512_000, 0];

/// The "+" menu's attach rows: whether it picks a folder, its label, and its icon.
const ATTACH: [(bool, &str, IconName); 2] = [
    (false, "Files", IconName::Paperclip),
    (true, "Folder", IconName::Folder),
];

/// The "+" menu's rows that start a menu in the editor: the text they type, their label, and the
/// glyph drawn in the icon's place, as the web's `At` and `Command` icons are not in the set.
const SIGILS: [(&str, &str); 2] = [("@", "Mentions"), ("/", "Actions")];

/// The format rows: label, icon, the marks put before and after the selection, and the shortcut.
const FORMATS: [(&str, IconName, &str, &str, Option<&str>); 7] = [
    ("Bold", IconName::TextB, "**", "**", Some("Ctrl+B")),
    ("Italic", IconName::TextItalic, "*", "*", Some("Ctrl+I")),
    (
        "Strikethrough",
        IconName::TextStrikethrough,
        "~~",
        "~~",
        None,
    ),
    ("Inline code", IconName::Code, "`", "`", None),
    ("Code block", IconName::CodeBlock, "```\n", "\n```", None),
    ("Bulleted list", IconName::ListBullets, "- ", "", None),
    ("Numbered list", IconName::ListNumbers, "1. ", "", None),
];

pub fn compact_label(window: u64) -> String {
    if window == 0 {
        "When full".into()
    } else {
        format_tokens(window)
    }
}

/// The agents a chat can be in the role of, once read for the open menu.
pub enum Roles {
    Loading,
    Loaded(Vec<Agent>),
}

/// One row of an open menu, and what picking it does.
pub struct Row {
    pub pick: Pick,
    pub body: AnyElement,
    pub label: SharedString,
    pub checked: bool,
    /// A rule goes above it.
    pub ruled: bool,
    /// A small label goes above it (below the rule), naming the rows it starts.
    pub heading: Option<&'static str>,
    /// The shortcut that does the same, shown at the row's end.
    pub hint: Option<&'static str>,
}

impl Row {
    /// A row with no rule, heading or hint.
    pub fn new(
        pick: Pick,
        body: AnyElement,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self {
            pick,
            body,
            label: label.into(),
            checked,
            ruled: false,
            heading: None,
            hint: None,
        }
    }
}

/// A row's body: its icon, muted, before its label.
pub fn with_icon(name: IconName, label: impl Into<SharedString>, t: &Theme) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(icon(name, 14.).text_color(t.text_muted))
        .child(label.into())
        .into_any_element()
}

impl Composer {
    /// The rows of the open menu, in order.
    pub(super) fn rows(&self, menu: Menu, picks: &Picks, can_compact: bool, t: &Theme) -> Vec<Row> {
        let text = |label: String| div().child(label).into_any_element();
        let row = |pick: Pick, label: String, checked: bool| {
            Row::new(pick, text(label.clone()), label, checked)
        };
        match menu {
            Menu::Model | Menu::Project | Menu::Agent | Menu::Access => Vec::new(),
            Menu::Attach => {
                let glyph = |sigil: &'static str, label: &'static str| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .w(px(14.))
                                .flex()
                                .justify_center()
                                .text_color(t.text_muted)
                                .child(sigil),
                        )
                        .child(label)
                        .into_any_element()
                };
                let attach = ATTACH
                    .iter()
                    .enumerate()
                    .map(|(i, &(folder, label, name))| {
                        let mut row = Row::new(
                            Pick::Attach(folder),
                            with_icon(name, label, t),
                            label,
                            false,
                        );
                        if i == 0 {
                            row.heading = Some("Add context");
                            row.hint = Some("attach");
                        }
                        row
                    });
                let sigils = SIGILS.iter().map(|&(sigil, label)| {
                    Row::new(Pick::Insert(sigil), glyph(sigil, label), label, false)
                });
                let formats =
                    FORMATS
                        .iter()
                        .enumerate()
                        .map(|(i, &(label, name, before, after, hint))| {
                            let mut row = Row::new(
                                Pick::Format(before, after),
                                with_icon(name, label, t),
                                label,
                                false,
                            );
                            row.hint = hint;
                            if i == 0 {
                                row.ruled = true;
                                row.heading = Some("Format");
                            }
                            row
                        });
                attach.chain(sigils).chain(formats).collect()
            }
            Menu::Effort => {
                let levels = picks
                    .agent
                    .model_efforts(picks.model.as_deref(), self.models_of(picks.agent));
                let current = picks.effort.as_ref().filter(|e| levels.contains(e));
                std::iter::once(row(Pick::Effort(None), "Auto".into(), current.is_none()))
                    .chain(levels.iter().map(|e| {
                        row(
                            Pick::Effort(Some(e.clone())),
                            effort_label(e),
                            current == Some(e),
                        )
                    }))
                    .collect()
            }
            Menu::Compact => {
                let mut rows: Vec<Row> = std::iter::once(row(
                    Pick::Compact(None),
                    format!("Default ({})", compact_label(picks.fallback_window)),
                    picks.compact.is_none(),
                ))
                .chain(WINDOWS.iter().map(|&w| {
                    row(
                        Pick::Compact(Some(w)),
                        compact_label(w),
                        picks.compact == Some(w),
                    )
                }))
                .collect();
                if can_compact {
                    let mut now = row(Pick::CompactNow, "Compact now".into(), false);
                    now.ruled = true;
                    rows.push(now);
                }
                rows
            }
            Menu::Send => {
                let queue_next = self.queues;
                let mode = |queue: bool, label: &'static str, description: &'static str| Row {
                    heading: None,
                    hint: None,
                    pick: Pick::SendMode(queue),
                    body: div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(2.))
                        .min_w(px(192.))
                        .child(label)
                        .child(
                            div()
                                .text_size(px(fs_xs()))
                                .text_color(t.text_subtle)
                                .child(description),
                        )
                        .into_any_element(),
                    label: label.into(),
                    checked: queue == queue_next,
                    ruled: false,
                };
                vec![
                    mode(
                        false,
                        "Steer current turn",
                        "Send the next message into the active turn",
                    ),
                    mode(
                        true,
                        "Queue for next turn",
                        "Wait until the active turn finishes",
                    ),
                ]
            }
        }
    }

    /// The open menu's box and its rows.
    pub(super) fn menu_box(
        &self,
        menu: Menu,
        rows: Vec<Row>,
        focus: &FocusHandle,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = match menu {
            Menu::Model => "Choose model",
            Menu::Effort => "Effort",
            Menu::Compact => "Compact at",
            Menu::Access => "Access",
            Menu::Agent => "Agent",
            Menu::Send => "Send mode",
            Menu::Project => "Project",
            Menu::Attach => "Add context",
        };
        let items = rows.into_iter().enumerate().map(|(i, row)| {
            let pick = row.pick.clone();
            let item = menu_item(
                SharedString::from(format!("menu-{i}")),
                row.label,
                row.checked,
                self.highlight == i,
                t,
            )
            .flex_1()
            .min_w_0()
            .child(row.body)
            .when_some(row.hint, |item, hint| {
                item.child(div().flex_1()).child(
                    div()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .child(hint),
                )
            })
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.highlight != i {
                    this.highlight = i;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.apply(pick.clone(), window, cx)));
            div()
                .flex()
                .flex_col()
                .when(row.ruled, |d| {
                    d.child(div().my(px(4.)).h(px(1.)).bg(t.border))
                })
                .when_some(row.heading, |d, heading| {
                    d.child(
                        div()
                            .px(px(8.))
                            .pt(px(6.))
                            .pb(px(4.))
                            .text_size(px(fs_xs()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.text_muted)
                            .child(heading),
                    )
                })
                .child(item)
        });
        popup(t)
            .id("menu")
            .role(Role::Menu)
            .aria_label(label)
            .track_focus(focus)
            .occlude()
            .on_key_down(cx.listener(Self::menu_key))
            // Tab leaves a menu the way Escape does, rather than moving on with it still open.
            .on_action(
                cx.listener(|this, _: &patchwork_ui_components::FocusNext, window, cx| {
                    this.close_menu(window, cx);
                }),
            )
            .on_action(
                cx.listener(|this, _: &patchwork_ui_components::FocusPrev, window, cx| {
                    this.close_menu(window, cx);
                }),
            )
            .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
                this.dismissed = this.menu.take().map(|m| (m, e.position));
                this.restore_focus(window, cx);
                cx.notify();
            }))
            .child(
                div()
                    .id("menu-rows")
                    .flex()
                    .flex_col()
                    .max_h(px(384.))
                    .overflow_y_scroll()
                    .children(items),
            )
            .into_any_element()
    }

    /// Up and Down move the highlight, Enter or Space picks it, and Escape closes the menu.
    pub(super) fn menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.menu_picks.len();
        match event.keystroke.key.as_str() {
            "down" if count > 0 => self.highlight = (self.highlight + 1) % count,
            "up" if count > 0 => self.highlight = (self.highlight + count - 1) % count,
            "home" => self.highlight = 0,
            "end" => self.highlight = count.saturating_sub(1),
            "enter" | "space" => {
                if let Some(pick) = self.menu_picks.get(self.highlight).cloned() {
                    self.apply(pick, window, cx);
                }
            }
            "escape" => self.close_menu(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
