//! The composer's menus (`EffortPicker`, `CompactPicker`, `AccessPicker`, `AgentPicker`): what
//! each offers, and the keys that move through them. Each opens up from its chip.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, FocusHandle, KeyDownEvent, MouseDownEvent, Role, SharedString, Window, div,
    prelude::*, px,
};
use patchwork_server::agents::Agent;
use patchwork_server::provider::{AgentKind, ModelOption};
use patchwork_server::threads::AccessMode;

use super::{Composer, Picks};
use crate::ui::theme::{FS_SM, FS_XS, Theme};
use crate::ui::{button, format_tokens, icon, menu_item, popup};

/// The chips that open a menu.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Model,
    Effort,
    Compact,
    Access,
    Agent,
    /// Queue or steer, beside the send button while a turn runs.
    Send,
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
}

/// Context sizes to compact at, each twice the one before; 0 leaves it to the agent.
const WINDOWS: [u64; 4] = [128_000, 256_000, 512_000, 0];

const ACCESS_MODES: [(AccessMode, &str, &str, &str); 4] = [
    (
        AccessMode::Supervised,
        "Supervised",
        "Ask before commands and file changes.",
        "lock-simple",
    ),
    (
        AccessMode::AutoAcceptEdits,
        "Auto-accept edits",
        "Auto-approve edits, ask before other actions.",
        "pencil-simple",
    ),
    (
        AccessMode::Auto,
        "Auto",
        "Supported providers approve routine actions; others still ask.",
        "sparkle",
    ),
    (
        AccessMode::FullAccess,
        "Full access",
        "Allow commands and edits without prompts.",
        "lock-open",
    ),
];

/// The levels each CLI takes on every model it has, for a model whose own it does not report.
fn agent_efforts(agent: AgentKind) -> &'static [&'static str] {
    match agent {
        AgentKind::Claude | AgentKind::Antigravity => &["low", "medium", "high", "xhigh", "max"],
        AgentKind::Codex => &["low", "medium", "high", "xhigh"],
        AgentKind::OpenCode | AgentKind::Api => &["low", "medium", "high"],
    }
}

/// The levels the picked model takes: its own, else its agent's.
pub fn efforts(
    agent: AgentKind,
    model: Option<&str>,
    models: Option<&[ModelOption]>,
) -> Vec<String> {
    model
        .and_then(|id| models?.iter().find(|m| m.id.as_deref() == Some(id)))
        .and_then(|m| m.efforts.clone())
        .unwrap_or_else(|| {
            agent_efforts(agent)
                .iter()
                .map(std::string::ToString::to_string)
                .collect()
        })
}

pub fn effort_label(effort: &str) -> String {
    match effort {
        "low" => "Low".into(),
        "medium" => "Medium".into(),
        "high" => "High".into(),
        "xhigh" => "Extra high".into(),
        "max" => "Max".into(),
        "ultra" => "Ultra".into(),
        other => crate::ui::display_title(other),
    }
}

pub fn access_of(mode: AccessMode) -> (&'static str, &'static str) {
    ACCESS_MODES
        .iter()
        .find(|(m, ..)| *m == mode)
        .map_or(("Full access", "lock-open"), |(_, label, _, icon)| {
            (*label, *icon)
        })
}

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
}

impl Composer {
    /// The rows of the open menu, in order.
    pub(super) fn rows(&self, menu: Menu, picks: &Picks, can_compact: bool, t: &Theme) -> Vec<Row> {
        let text = |label: String| div().child(label).into_any_element();
        let row = |pick: Pick, label: String, checked: bool| Row {
            pick,
            body: text(label.clone()),
            label: label.into(),
            checked,
            ruled: false,
        };
        match menu {
            Menu::Model => Vec::new(),
            Menu::Effort => {
                let levels = efforts(
                    picks.agent,
                    picks.model.as_deref(),
                    self.models_of(picks.agent),
                );
                let current = picks.effort.as_ref().filter(|e| levels.contains(e));
                std::iter::once(row(Pick::Effort(None), "Default".into(), current.is_none()))
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
            Menu::Access => ACCESS_MODES
                .iter()
                .map(|&(mode, label, description, name)| Row {
                    pick: Pick::Access(mode),
                    body: div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(2.))
                        .min_w(px(284.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(icon(name, 14.))
                                .child(label),
                        )
                        .child(
                            div()
                                .pl(px(20.))
                                .text_size(px(FS_SM))
                                .text_color(t.text_muted)
                                .child(description),
                        )
                        .into_any_element(),
                    label: label.into(),
                    checked: picks.access == mode,
                    ruled: false,
                })
                .collect(),
            Menu::Send => {
                let queue_next = self.queues;
                let mode = |queue: bool, label: &'static str, description: &'static str| Row {
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
                                .text_size(px(FS_XS))
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
            Menu::Agent => {
                let mut rows = vec![row(
                    Pick::Role(None),
                    "No agent".into(),
                    picks.role.is_none(),
                )];
                if let Roles::Loaded(agents) = &self.roles {
                    rows.extend(agents.iter().map(|agent| {
                        let body = div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .flex_1()
                            .gap(px(2.))
                            .child(div().truncate().child(agent.name.clone()))
                            .when_some(agent.description.clone(), |col, description| {
                                col.child(
                                    div()
                                        .truncate()
                                        .text_size(px(FS_SM))
                                        .text_color(t.text_muted)
                                        .child(description),
                                )
                            });
                        Row {
                            pick: Pick::Role(Some(agent.name.clone())),
                            body: body.into_any_element(),
                            label: agent.name.clone().into(),
                            checked: picks.role.as_deref() == Some(&agent.name),
                            ruled: false,
                        }
                    }));
                }
                rows
            }
        }
    }

    /// The open menu's box, its rows, and for the agent menu its state and edit buttons.
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
        };
        let agents: Vec<Agent> = match &self.roles {
            Roles::Loaded(agents) if menu == Menu::Agent => agents.clone(),
            _ => Vec::new(),
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
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.highlight != i {
                    this.highlight = i;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.apply(pick.clone(), window, cx)));
            // A custom agent's row has its edit button beside it.
            let edit = (menu == Menu::Agent && i > 0)
                .then(|| agents.get(i - 1).cloned())
                .flatten()
                .map(|agent| {
                    let (fg, hover) = (t.text, t.text_muted);
                    button(
                        SharedString::from(format!("edit-agent-{i}")),
                        format!("Edit {}", agent.name),
                    )
                    .size(px(24.))
                    .mr(px(4.))
                    .flex_none()
                    .justify_center()
                    .text_color(hover)
                    .hover(move |s| s.text_color(fg))
                    .child(icon("pencil-simple", 14.))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.edit_agent(&agent, window, cx)),
                    )
                });
            div()
                .flex()
                .flex_col()
                .when(row.ruled, |d| {
                    d.child(div().my(px(4.)).h(px(1.)).bg(t.border))
                })
                .child(div().flex().items_center().child(item).children(edit))
        });
        let status = (menu == Menu::Agent && matches!(self.roles, Roles::Loading)).then(|| {
            div()
                .id("agents-status")
                .role(Role::Status)
                .px(px(8.))
                .py(px(6.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child("Loading agents…")
        });
        popup(t)
            .id("menu")
            .role(Role::Menu)
            .aria_label(label)
            .track_focus(focus)
            .occlude()
            .when(menu == Menu::Agent, |p| p.w(px(300.)))
            .on_key_down(cx.listener(Self::menu_key))
            // Tab leaves a menu the way Escape does, rather than moving on with it still open.
            .on_action(cx.listener(|this, _: &crate::FocusNext, window, cx| {
                this.close_menu(window, cx);
            }))
            .on_action(cx.listener(|this, _: &crate::FocusPrev, window, cx| {
                this.close_menu(window, cx);
            }))
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
                    .children(status)
                    .children(items),
            )
            .into_any_element()
    }

    /// Up and Down move the highlight, Enter or Space picks it, and Escape closes the menu.
    fn menu_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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

    fn edit_agent(&mut self, agent: &Agent, window: &mut Window, cx: &mut Context<Self>) {
        let path = std::path::PathBuf::from(&agent.path);
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let (dir, name) = (
            dir.to_string_lossy().into_owned(),
            name.to_string_lossy().into_owned(),
        );
        cx.background_spawn(async move {
            if let Err(e) = patchwork_server::workspace::open_in_editor(&dir, &name, None) {
                log::warn!(error:% = e; "could not open the agent: {{error}}");
            }
        })
        .detach();
        self.close_menu(window, cx);
    }
}
