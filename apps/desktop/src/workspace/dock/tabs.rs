//! The dock's header: its tabs, its tools, and the terminals' tabs.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use gpui::{
    AnyElement, App, ClickEvent, MouseButton, MouseDownEvent, Role, SharedString, Window, div,
    prelude::*, px,
};
use patchwork_ui_components::menu::MenuItem;
use patchwork_ui_theme::{IconName, fs_md, fs_xs, radius_sm, theme};
use patchwork_ui_components::{button, icon_button, tooltip};
use patchwork_ui_theme::icon;
use super::open_tabs::{MenuFor, Dock, DockTab};

/// The size of the header's icon buttons, as the web panel's `icon-xs`.
pub(crate) const TOOL: f32 = 24.;

impl Dock {
    /// A view's tab in the header, as VS Code's panel title draws one: its name, with a line under
    /// the one that shows.
    pub(crate) fn tab_button(
        &self,
        tab: DockTab,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme(cx);
        let on = self.tab == tab;
        let fg = t.text;
        button(SharedString::from(label), label)
            .role(Role::Tab)
            .aria_selected(on)
            .relative()
            .h_full()
            .px(px(12.))
            .text_size(px(fs_md()))
            .map(|b| {
                if on {
                    b.text_color(t.text)
                } else {
                    b.text_color(t.text_muted).hover(move |s| s.text_color(fg))
                }
            })
            .child(label)
            .when(on, |b| {
                b.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.))
                        .bg(t.accent),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| this.show(tab, cx)))
    }

    /// The tools at the header's right for the view that shows.
    pub(crate) fn header_tools(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = *theme(cx);
        let tool = |id: &'static str, label: &'static str, name: IconName, cx: &App| {
            icon_button(id, label, name, 14., cx)
                .size(px(TOOL))
                .tooltip(tooltip(label))
        };
        match self.tab {
            DockTab::Terminal => {
                let mut tools: Vec<AnyElement> = self.find_bar(window, cx).into_iter().collect();
                let current = self.active_shell().map(|s| (s.id, s.name.clone()));
                let name = match &current {
                    Some((_, name)) => name.clone(),
                    None if self.starting || self.pending_start.is_some() => "Starting…".into(),
                    None => "Terminal".into(),
                };
                let (hover, fg) = (t.bg_hover, t.text);
                // The terminal chooser: which terminals there are, and which is in view.
                tools.push(
                    button("terminal-chooser", "Open terminals")
                        .aria_expanded(self.menu_open(&MenuFor::Terminals))
                        .h(px(TOOL))
                        .gap(px(6.))
                        .px(px(6.))
                        .rounded(px(radius_sm()))
                        .text_size(px(fs_md()))
                        .text_color(t.text_muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .tooltip(tooltip("Open terminals"))
                        .child(icon(IconName::TerminalWindow, 14.))
                        .child(div().max_w(px(140.)).truncate().child(name))
                        .child(icon(IconName::CaretDown, 11.))
                        .on_click(cx.listener(Dock::terminals_menu))
                        .into_any_element(),
                );
                // New terminal, with its shell beside it, as VS Code's `+ ⌄` is.
                let mut new = div().flex().items_center().child(
                    icon_button("terminal-new", "New terminal", IconName::Plus, 15., cx)
                        .size(px(TOOL))
                        .tooltip(tooltip("New terminal (Ctrl+Shift+`)"))
                        .on_click(cx.listener(|this, _, _, cx| this.start(true, None, cx))),
                );
                // An SSH host starts the user's login shell there; the shells listed are this
                // computer's.
                if self.host.is_none() && self.shell_choices.len() > 1 {
                    new = new.child(
                        button("terminal-shells", "Choose a shell")
                            .aria_expanded(self.menu_open(&MenuFor::Shells))
                            .h(px(TOOL))
                            .w(px(16.))
                            .justify_center()
                            .rounded(px(radius_sm()))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .tooltip(tooltip("New terminal in a shell"))
                            .child(icon(IconName::CaretDown, 11.))
                            .on_click(cx.listener(Dock::shells_menu)),
                    );
                }
                tools.push(new.into_any_element());
                let active = current.map(|(id, _)| id);
                tools.push(
                    tool("terminal-kill", "Kill the terminal", IconName::Trash, cx)
                        .when(active.is_none(), |b| b.opacity(0.5).cursor_default())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(id) = active {
                                this.kill(id, cx);
                            }
                        }))
                        .into_any_element(),
                );
                tools.push(
                    icon_button(
                        "terminal-more",
                        "More terminal actions",
                        IconName::DotsThree,
                        16.,
                        cx,
                    )
                    .size(px(TOOL))
                    .tooltip(tooltip("More"))
                    .aria_expanded(self.menu_open(&MenuFor::More))
                    .on_click(cx.listener(Dock::more_menu))
                    .into_any_element(),
                );
                tools
            }
            DockTab::Output => {
                let channel = self.output.channel().cloned();
                let name = channel.as_ref().map(|c| c.name.clone());
                let mut tools = vec![
                    patchwork_ui_components::dropdown_chip(
                        "output-log",
                        format!("Log: {}", name.as_deref().unwrap_or("none")),
                        name.unwrap_or_else(|| "No logs yet".into()),
                        self.menu_open(&MenuFor::Logs),
                        &t,
                    )
                    .on_click(cx.listener(Dock::logs_menu))
                    .into_any_element(),
                ];
                if let Some(channel) = channel {
                    let level = *(channel.setting)(&mut self.logging.clone());
                    let current = patchwork_desktop_settings::settings::LOG_LEVELS
                        .iter()
                        .find(|(l, _)| *l == level)
                        .map_or("", |(_, label)| label);
                    tools.push(
                        patchwork_ui_components::dropdown_chip(
                            "output-level",
                            format!("{} log level: {current}", channel.name),
                            current,
                            self.menu_open(&MenuFor::Level),
                            &t,
                        )
                        .on_click(cx.listener(Dock::level_menu))
                        .into_any_element(),
                    );
                }
                tools.push(
                    tool(
                        "output-folder",
                        "Open logs folder",
                        IconName::FolderOpen,
                        cx,
                    )
                    .on_click(|_, _, cx| match patchwork_backend::logging::logs_folder() {
                        Ok(logs) => cx.open_with_system(&logs),
                        Err(e) => {
                            log::warn!(error:% = e; "could not open the logs folder: {{error}}");
                        }
                    })
                    .into_any_element(),
                );
                tools
            }
            DockTab::Screen => Vec::new(),
        }
    }

    /// The terminals' tabs, in a strip over the active one once there is more than one.
    pub(crate) fn shell_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *theme(cx);
        let active = self.active_shell().map(|s| s.id);
        // A name shared by several terminals is numbered from the second on: zsh, zsh 2.
        let mut seen: HashMap<String, usize> = HashMap::new();
        let tabs = self.shown().map(|shell| {
            let id = shell.id;
            let on = active == Some(id);
            let n = seen.entry(shell.name.clone()).or_default();
            *n += 1;
            let label = if *n == 1 {
                shell.name.clone()
            } else {
                format!("{} {n}", shell.name)
            };
            let exited = shell.exit_code.is_some();
            let (hover, fg) = (t.bg_hover, t.text);
            let group = SharedString::from(format!("shell-tab-{id}"));
            let close = button(("shell-kill", id as usize), format!("Close {label}"))
                .flex_none()
                .size(px(18.))
                .justify_center()
                .rounded(px(radius_sm()))
                .text_color(t.text_muted)
                .hover(move |s| s.bg(hover).text_color(fg))
                .tooltip(tooltip("Kill terminal"))
                .child(icon(IconName::X, 11.))
                .opacity(0.)
                .group_hover(group.clone(), |s| s.opacity(1.))
                .focus(|s| s.opacity(1.))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.kill(id, cx);
                }));
            div()
                .id(("shell-tab", id as usize))
                .group(group)
                .h_full()
                .max_w(px(200.))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .pl(px(10.))
                .pr(px(4.))
                .text_size(px(fs_xs()))
                .cursor_pointer()
                .map(|row| {
                    if on {
                        row.bg(t.bg_editor).text_color(t.text)
                    } else {
                        row.text_color(t.text_muted)
                            .hover(move |s| s.bg(hover.opacity(0.6)).text_color(fg))
                    }
                })
                .when_some(shell.exit_code, |row, code| {
                    row.tooltip(tooltip(format!("Exited with code {code}")))
                })
                .child(
                    button(("shell-select", id as usize), label.clone())
                        .role(Role::Tab)
                        .aria_selected(on)
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .when(exited, |d| d.line_through().opacity(0.6))
                                .child(label),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.active = Some(id);
                            window.focus(&this.term_focus, cx);
                            cx.notify();
                        })),
                )
                .child(close)
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
                            .icon(IconName::Trash)
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
            .h(px(28.))
            .flex_none()
            .flex()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .children(tabs)
    }

    /// Opens the terminal chooser: the terminals, numbered, with the one in view checked, then
    /// find and kill.
    pub(crate) fn terminals_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let active = self.active_shell().map(|s| s.id);
        let mut items: Vec<MenuItem> = self
            .shown()
            .enumerate()
            .map(|(i, shell)| {
                let (id, this) = (shell.id, this.clone());
                let label = if shell.exit_code.is_some() {
                    format!("{}: {} (exited)", i + 1, shell.name)
                } else {
                    format!("{}: {}", i + 1, shell.name)
                };
                MenuItem::new(label, move |window, cx| {
                    let _ = this.update(cx, |p, cx| {
                        p.active = Some(id);
                        window.focus(&p.term_focus, cx);
                        cx.notify();
                    });
                })
                .radio(active == Some(id))
            })
            .collect();
        let find = this.clone();
        items.push(
            MenuItem::new("Find…", move |window, cx| {
                let _ = find.update(cx, |p, cx| p.open_find(window, cx));
            })
            .icon(IconName::MagnifyingGlass)
            .separated(),
        );
        items.push(
            MenuItem::new("Kill the terminal", move |_, cx| {
                let _ = this.update(cx, |p, cx| {
                    if let Some(id) = active {
                        p.kill(id, cx);
                    }
                });
            })
            .icon(IconName::Trash),
        );
        self.toggle_menu(MenuFor::Terminals, "Terminals", items, e, window, cx);
    }

    /// Opens the terminal's other actions: find, clearing the screen, and maximizing the panel.
    pub(crate) fn more_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let (find, clear) = (this.clone(), this.clone());
        let items = vec![
            MenuItem::new("Find…", move |window, cx| {
                let _ = find.update(cx, |p, cx| p.open_find(window, cx));
            })
            .icon(IconName::MagnifyingGlass),
            MenuItem::new("Clear the screen", move |_, cx| {
                let _ = clear.update(cx, Dock::clear_screen);
            }),
            MenuItem::new(
                if self.maximized {
                    "Restore panel size"
                } else {
                    "Maximize panel"
                },
                move |_, cx| {
                    let _ = this.update(cx, |p, cx| p.set_maximized(!p.maximized, cx));
                },
            )
            .separated(),
        ];
        self.toggle_menu(MenuFor::More, "More", items, e, window, cx);
    }
}
