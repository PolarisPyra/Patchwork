//! Setup's first step: which agent CLIs are installed and signed in, which to
//! connect, and which one new tasks start with.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, Div, FontWeight, Role, SharedString, Toggled, div, prelude::*, px};
use patchwork_server::provider::AgentKind;

use super::{AGENT_KINDS, Onboarding, Probe};
use super::rows::{choice_row, heading, key_id, picked, row_end, tick};
use crate::ui::theme::{FS_SM, RADIUS, Theme};
use crate::ui::{BtnKind, agent_label, agent_logo, badge, btn, button, icon, spinner};

impl Onboarding {
    /// Detects every agent at once; run again, it shows a CLI installed during setup.
    pub(super) fn check(&mut self, cx: &mut Context<Self>) {
        for kind in AGENT_KINDS {
            self.detect(kind, cx);
        }
    }

    /// Detects one agent.
    fn detect(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.probes.entry(kind).or_default().checking = true;
        let task =
            cx.background_spawn(async move { patchwork_server::provider::detect_agent(kind) });
        cx.spawn(async move |this, cx| {
            let status = task.await;
            let _ = this.update(cx, |this, cx| {
                this.probes.insert(
                    kind,
                    Probe {
                        checking: false,
                        status: Some(status),
                    },
                );
                this.sync_agents();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Downloads `kind`'s CLI, then detects it again. A CLI that installs is connected once it is
    /// ready.
    fn install(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.installing.insert(kind);
        self.install_errors.remove(&kind);
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { server.install_cli(kind) });
        cx.spawn(async move |this, cx| {
            let installed = task.await;
            let _ = this.update(cx, |this, cx| {
                this.installing.remove(&kind);
                match installed {
                    Ok(()) => {
                        if let Some(agents) = this.agents.as_mut()
                            && !agents.contains(&kind)
                        {
                            agents.push(kind);
                        }
                        this.detect(kind, cx);
                    }
                    Err(e) => {
                        log::warn!(
                            agent:? = kind, error:% = e;
                            "could not install {{agent}}: {{error}}"
                        );
                        this.install_errors.insert(kind, e.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn checking(&self) -> bool {
        AGENT_KINDS
            .iter()
            .any(|k| self.probes.get(k).is_none_or(|p| p.checking))
    }

    /// Only ready agents can be picked. Starts from the saved agents (or every ready agent the
    /// first time), then keeps the user's choices while they stay ready.
    pub(super) fn sync_agents(&mut self) {
        if self.checking() {
            return;
        }
        let ready: Vec<AgentKind> = AGENT_KINDS
            .into_iter()
            .filter(|k| self.probes.get(k).is_some_and(Probe::ready))
            .collect();
        let saved: Vec<AgentKind> = self
            .settings
            .agents
            .iter()
            .copied()
            .filter(|k| ready.contains(k))
            .collect();
        let base = match self.agents.take() {
            Some(current) => current,
            None if !saved.is_empty() => saved,
            None => ready.clone(),
        };
        self.agents = Some(ready.into_iter().filter(|k| base.contains(k)).collect());
    }

    pub(super) fn toggle_agent(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        let mut set = self.agents.take().unwrap_or_default();
        if set.contains(&kind) {
            set.retain(|k| *k != kind);
        } else {
            set.push(kind);
        }
        self.agents = Some(
            AGENT_KINDS
                .into_iter()
                .filter(|k| set.contains(k))
                .collect(),
        );
        cx.notify();
    }

    pub(super) fn default_agent(&self) -> Option<AgentKind> {
        let agents = self.agents.as_ref()?;
        match self.default_kind {
            Some(kind) if agents.contains(&kind) => Some(kind),
            _ => agents.first().copied(),
        }
    }

    /// The connected agents with the default first.
    pub(super) fn ordered_agents(&self) -> Vec<AgentKind> {
        let default = self.default_agent();
        default
            .into_iter()
            .chain(
                self.agents
                    .iter()
                    .flatten()
                    .copied()
                    .filter(|k| Some(*k) != default),
            )
            .collect()
    }
}

/// Every badge is as wide as the longest usual one, so they line up from row to row.
fn status_badge(probe: Option<&Probe>, t: &Theme) -> AnyElement {
    let wide = |b: Div| b.min_w(px(112.)).justify_center().gap(px(4.));
    let Some(probe) = probe.filter(|p| !p.checking) else {
        return wide(badge("", t))
            .child(spinner(12., t.text_muted))
            .child("Checking…")
            .into_any_element();
    };
    let b = match &probe.status {
        None => wide(badge("Check failed", t)),
        Some(s) if !s.installed => wide(badge("Not installed", t)),
        Some(s) if !s.signed_in => wide(badge("Not signed in", t))
            .bg(t.danger_soft)
            .text_color(t.danger),
        Some(s) => wide(badge(
            s.version
                .as_ref()
                .map_or_else(|| "Ready".to_string(), |v| format!("Ready · {v}")),
            t,
        ))
        .bg(t.success.opacity(0.14))
        .text_color(t.success),
    };
    b.into_any_element()
}

pub fn agent_step(this: &Onboarding, t: &Theme, cx: &mut Context<Onboarding>) -> AnyElement {
    let checking = this.checking();
    let accent = t.accent_text;
    let recheck = button("setup-recheck", "Check again")
        .gap(px(4.))
        .text_color(accent)
        .child(if checking {
            spinner(14., accent).into_any_element()
        } else {
            icon("arrow-clockwise", 14.).into_any_element()
        })
        .child("Check again");
    let recheck = if checking {
        recheck.opacity(0.5).cursor_default()
    } else {
        recheck.on_click(cx.listener(|this, _, _, cx| this.check(cx)))
    };
    let selected = this.agents.clone().unwrap_or_default();
    let rows = AGENT_KINDS.into_iter().map(|kind| {
        let probe = this.probes.get(&kind);
        let ready = probe.is_some_and(Probe::ready);
        let checked = selected.contains(&kind);
        let detail = probe
            .and_then(|p| p.status.as_ref())
            .map(|s| s.detail.clone())
            .unwrap_or_default();
        let missing = probe
            .filter(|p| !p.checking)
            .and_then(|p| p.status.as_ref())
            .is_some_and(|s| !s.installed);
        let mut end = vec![status_badge(probe, t)];
        if missing {
            end.push(download_button(this, kind, t, cx));
        }
        if ready {
            end.push(tick(checked, t));
        }
        let row = choice_row(
            key_id("setup-agent", kind),
            agent_label(kind),
            detail,
            agent_logo(kind, None, 26.),
            checked,
            ready,
            t,
        )
        .child(row_end(end));
        let row = if ready {
            row.on_click(cx.listener(move |this, _, _, cx| this.toggle_agent(kind, cx)))
        } else {
            row
        };
        div().flex().flex_col().gap(px(6.)).child(row).children(
            this.install_errors
                .get(&kind)
                .map(|e| install_error(kind, e, t)),
        )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(heading("Available coding agents"))
                .child(recheck),
        )
        .child(
            div()
                .id("setup-agents")
                .role(Role::Group)
                .aria_label("Coding agents")
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(rows),
        )
        .when(!selected.is_empty(), |s| {
            s.child(default_agent(this, &selected, t, cx))
        })
        .into_any_element()
}

/// Downloads a CLI that is not installed. While it runs, it is disabled and shows a spinner.
fn download_button(
    this: &Onboarding,
    kind: AgentKind,
    t: &Theme,
    cx: &mut Context<Onboarding>,
) -> AnyElement {
    let installing = this.installing.contains(&kind);
    let name = agent_label(kind);
    btn(
        key_id("setup-download", kind),
        if installing {
            format!("Installing {name}")
        } else {
            format!("Download {name}")
        },
        BtnKind::Plain,
        installing,
        t,
    )
    .when(installing, |b| {
        b.role(Role::Status)
            .child(spinner(12., t.text_muted))
            .child("Installing…")
    })
    .when(!installing, |b| {
        b.child("Download")
            .on_click(cx.listener(move |this, _, _, cx| this.install(kind, cx)))
    })
    .into_any_element()
}

/// Why `kind`'s last download failed.
fn install_error(kind: AgentKind, error: &str, t: &Theme) -> impl IntoElement {
    div()
        .id(key_id("setup-download-error", kind))
        .role(Role::Alert)
        .px(px(16.))
        .text_size(px(FS_SM))
        .text_color(t.danger)
        .child(SharedString::from(error.to_string()))
}

/// Picks which connected agent new tasks start with.
fn default_agent(
    this: &Onboarding,
    selected: &[AgentKind],
    t: &Theme,
    cx: &mut Context<Onboarding>,
) -> impl IntoElement {
    let value = this.default_agent();
    let hover = t.bg_hover;
    let fg = t.text;
    let tiles = selected.iter().map(|&kind| {
        let checked = Some(kind) == value;
        let tile = div()
            .id(key_id("setup-default", kind))
            .role(Role::RadioButton)
            .aria_label(agent_label(kind))
            .aria_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            })
            .when_some(
                this.default_tiles.get(&kind),
                gpui::InteractiveElement::track_focus,
            )
            .when(checked, |d| d.tab_index(0))
            .flex_1()
            .min_w(px(150.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .rounded(px(RADIUS))
            .border_1()
            .px(px(12.))
            .pt(px(16.))
            .pb(px(12.))
            .text_size(px(FS_SM))
            .cursor_pointer()
            .child(
                div()
                    .size(px(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS))
                    .bg(t.bg_selected_muted)
                    .child(agent_logo(kind, None, 22.)),
            )
            .child(agent_label(kind))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.default_kind = Some(kind);
                cx.notify();
            }));
        if checked {
            picked(tile, t)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
        } else {
            tile.border_color(t.border)
                .text_color(t.text_muted)
                .hover(move |s| s.bg(hover).text_color(fg))
        }
    });
    let order = selected.to_vec();
    div()
        .mt(px(8.))
        .flex()
        .flex_col()
        .gap(px(16.))
        .child(heading("Default agent"))
        .child(
            div()
                .id("setup-default-agents")
                .role(Role::RadioGroup)
                .aria_label("Default agent")
                .flex()
                .flex_wrap()
                .gap(px(8.))
                // Arrow keys pick the next or previous agent, as in any radio group.
                .on_key_down(
                    cx.listener(move |this, e: &gpui::KeyDownEvent, window, cx| {
                        let step: isize = match e.keystroke.key.as_str() {
                            "left" | "up" => -1,
                            "right" | "down" => 1,
                            _ => return,
                        };
                        cx.stop_propagation();
                        let at = this
                            .default_agent()
                            .and_then(|k| order.iter().position(|o| *o == k))
                            .unwrap_or(0) as isize;
                        let len = order.len() as isize;
                        let next = order[((at + step + len) % len) as usize];
                        this.default_kind = Some(next);
                        if let Some(focus) = this.default_tiles.get(&next) {
                            window.focus(focus, cx);
                        }
                        cx.notify();
                    }),
                )
                .children(tiles),
        )
}
