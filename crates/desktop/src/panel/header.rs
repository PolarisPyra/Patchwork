//! The panel's header: its tabs, its tools, and the terminals' tabs.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, App, FontWeight, MouseButton, MouseDownEvent, Role, SharedString, div, prelude::*,
    px,
};
use crate::ui::menu::MenuItem;
use crate::ui::theme::{FS_SM, theme};
use crate::ui::{button, icon, icon_button, tooltip};
use super::state::{MenuFor, Panel, PanelTab};

impl Panel {
    pub(super) fn tab_button(
        &self,
        tab: PanelTab,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let on = self.tab == tab;
        let fg = t.text;
        button(SharedString::from(label), label)
            .role(Role::Tab)
            .aria_selected(on)
            .h(px(22.))
            .border_b_2()
            .text_size(px(FS_SM))
            .font_weight(FontWeight::MEDIUM)
            .map(|b| {
                if on {
                    b.border_color(t.accent).text_color(t.text)
                } else {
                    b.border_color(gpui::transparent_black())
                        .text_color(t.text_muted)
                        .hover(move |s| s.text_color(fg))
                }
            })
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.show(tab, cx)))
    }

    pub(super) fn header_tools(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let t = *theme(cx);
        let tool = |id: &'static str, label: &'static str, name: &'static str, cx: &App| {
            icon_button(id, label, name, 14., cx)
                .size(px(24.))
                .tooltip(tooltip(label))
        };
        match self.tab {
            PanelTab::Terminal => {
                let mut tools = Vec::new();
                if let [only] = self.shells.as_slice() {
                    tools.push(
                        div()
                            .px(px(4.))
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(only.name.clone())
                            .into_any_element(),
                    );
                }
                tools.push(
                    tool("terminal-new", "New terminal", "plus", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.start(true, None, cx)))
                        .into_any_element(),
                );
                tools.push(
                    tool(
                        "terminal-shells",
                        "New terminal in a shell",
                        "caret-down",
                        cx,
                    )
                    .aria_expanded(self.menu_open(&MenuFor::Shells))
                    .on_click(cx.listener(super::state::Panel::shells_menu))
                    .into_any_element(),
                );
                // With the tab list showing, each tab has its own kill button.
                if self.shells.len() < 2 {
                    let active = self.active_shell().map(|s| s.id);
                    tools.push(
                        tool("terminal-kill", "Kill terminal", "trash", cx)
                            .when(active.is_none(), |b| b.opacity(0.5).cursor_default())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(id) = active {
                                    this.kill(id, cx);
                                }
                            }))
                            .into_any_element(),
                    );
                }
                tools
            }
            PanelTab::Output => {
                let channel = self.output.channel().cloned();
                let name = channel.as_ref().map(|c| c.name.clone());
                let mut tools = vec![
                    crate::ui::dropdown_chip(
                        "output-log",
                        format!("Log: {}", name.as_deref().unwrap_or("none")),
                        name.clone().unwrap_or_else(|| "No logs yet".into()),
                        self.menu_open(&MenuFor::Logs),
                        &t,
                    )
                    .on_click(cx.listener(super::state::Panel::logs_menu))
                    .into_any_element(),
                ];
                if let Some(channel) = channel {
                    let level = *(channel.setting)(&mut self.logging.clone());
                    let current = crate::settings::LOG_LEVELS
                        .iter()
                        .find(|(l, _)| *l == level)
                        .map_or("", |(_, label)| label);
                    tools.push(
                        crate::ui::dropdown_chip(
                            "output-level",
                            format!("{} log level: {current}", channel.name),
                            current,
                            self.menu_open(&MenuFor::Level),
                            &t,
                        )
                        .on_click(cx.listener(super::state::Panel::level_menu))
                        .into_any_element(),
                    );
                }
                let logs = patchwork_server::system::dirs().logs.clone();
                tools.push(
                    tool("output-folder", "Open logs folder", "folder-open", cx)
                        .on_click(move |_, _, cx| {
                            let _ = std::fs::create_dir_all(&logs);
                            cx.open_with_system(&logs);
                        })
                        .into_any_element(),
                );
                tools
            }
            PanelTab::Screen => Vec::new(),
        }
    }

    /// The terminals, listed beside the active one once there is more than one.
    pub(super) fn shell_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *theme(cx);
        let active = self.active_shell().map(|s| s.id);
        let tabs = self.shells.iter().enumerate().map(|(i, shell)| {
            let id = shell.id;
            let on = active == Some(id);
            let label = if i == 0 {
                shell.name.clone()
            } else {
                format!("{} {}", shell.name, i + 1)
            };
            let (hover, fg) = (t.bg_hover, t.text);
            let group = SharedString::from(format!("shell-tab-{id}"));
            let kill = icon_button(
                ("shell-kill", id as usize),
                format!("Kill {label}"),
                "trash",
                12.,
                cx,
            )
            .size(px(18.))
            .tooltip(tooltip("Kill terminal"))
            .when(!on, |b| {
                b.opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .focus(|s| s.opacity(1.))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.kill(id, cx);
            }));
            div()
                .id(("shell-tab", id as usize))
                .group(group)
                .h(px(22.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(8.))
                .pr(px(2.))
                .text_size(px(FS_SM))
                .cursor_pointer()
                .map(|row| {
                    if on {
                        row.bg(t.bg_selected_muted).text_color(t.text)
                    } else {
                        row.text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                    }
                })
                .when_some(shell.exit_code, |row, code| {
                    row.tooltip(tooltip(format!("Exited with code {code}")))
                })
                .child(icon("terminal-window", 14.).flex_none())
                .child(
                    button(("shell-select", id as usize), label.clone())
                        .role(Role::Tab)
                        .aria_selected(on)
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(label))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.active = Some(id);
                            window.focus(&this.term_focus, cx);
                            cx.notify();
                        })),
                )
                .child(kill)
                // Middle-click kills the terminal; right-click opens its menu.
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, _, cx| this.kill(id, cx)),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                        let weak = cx.entity().downgrade();
                        let items = vec![
                            MenuItem::new("Kill terminal", move |_, cx| {
                                let _ = weak.update(cx, |p, cx| p.kill(id, cx));
                            })
                            .icon("trash")
                            .danger(),
                        ];
                        this.open_menu(
                            MenuFor::Shell(id),
                            "Terminal",
                            items,
                            e.position,
                            window,
                            cx,
                        );
                    }),
                )
        });
        div()
            .id("shell-tabs")
            .role(Role::TabList)
            .aria_label("Terminals")
            .aria_orientation(gpui::Orientation::Vertical)
            .w(px(120.))
            .flex_none()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(t.border)
            .py(px(2.))
            .children(tabs)
    }
}
