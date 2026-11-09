//! The panel under the chat (`features/panel/Panel.tsx`): the integrated terminal, the Output view
//! with this launch's logs, and with computer use on, the Screen view. It stays alive while
//! closed, so its shells keep running and it keeps its tab.

mod cells;
mod input;
mod output;
mod screen;
mod terminal;

use std::sync::Arc;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, FontWeight,
    MouseButton, MouseDownEvent, Pixels, Point, Role, SharedString, Task, Window, div, prelude::*,
    px,
};
use patchwork_server::Server;
use patchwork_server::settings::{LogLevel, LoggingSettings, Settings, TerminalSettings};
use patchwork_server::terminal::TerminalShell;

use crate::menu::{Dismissed, Menu, MenuItem};
use crate::theme::{FS_SM, theme};
use crate::ui::{button, icon, icon_button, tooltip};

use output::OutputState;
use screen::ScreenState;
use terminal::Shell;

/// Where the shell last picked for new terminals is kept.
const SHELL_KEY: &str = "patchwork-terminal-shell";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    Terminal,
    Output,
    Screen,
}

pub enum PanelEvent {
    /// The panel asks to close: its close button, or its last shell ended.
    Close,
    /// A log's level changed here.
    SettingsChanged,
}

/// Which of the panel's menus is open.
#[derive(PartialEq)]
enum MenuFor {
    Shells,
    Logs,
    Level,
    Shell(u32),
}

pub struct Panel {
    server: Arc<Server>,
    pub tab: PanelTab,
    /// Whether the window shows the panel now.
    visible: bool,
    focus: FocusHandle,
    term_focus: FocusHandle,
    settings: TerminalSettings,
    logging: LoggingSettings,
    computer_use: bool,
    /// The folder new terminals start in: the open project's.
    cwd: Option<String>,
    shells: Vec<Shell>,
    active: Option<u32>,
    starting: bool,
    /// A "New terminal" asked for while a shell starts, opened once that one is up.
    start_next: Option<Option<String>>,
    /// A shell asked for before the screen had a size.
    pending_start: Option<(bool, Option<String>)>,
    restored: bool,
    /// The screen's columns and rows, and where it sits in the window.
    grid: Option<(u16, u16)>,
    origin: Point<Pixels>,
    resize_timer: Option<Task<()>>,
    /// A drag selecting on the screen is under way.
    selecting: bool,
    /// The button held down over a program that hears the mouse, and the cell last reported.
    mouse_held: Option<u8>,
    mouse_cell: Option<(u16, u16)>,
    /// When the cursor's blink last started over, as typing shows it at once.
    blink_from: std::time::Instant,
    blink_timer: Option<Task<()>>,
    /// Focus moves into the terminal at the next draw, once the shell asked for is up.
    focus_terminal: bool,
    error: Option<String>,
    shell_choices: Vec<TerminalShell>,
    picked: Option<String>,
    menu: Option<(MenuFor, Entity<Menu>, Point<Pixels>)>,
    menu_dismissed: Option<Point<Pixels>>,
    output: OutputState,
    screen: ScreenState,
}

impl EventEmitter<PanelEvent> for Panel {}

impl Panel {
    pub fn new(server: Arc<Server>, settings: &Settings, cx: &mut Context<Self>) -> Self {
        let picked = server.store.state(SHELL_KEY);
        Self {
            server,
            tab: PanelTab::Terminal,
            visible: false,
            focus: cx.focus_handle(),
            term_focus: cx.focus_handle(),
            settings: settings.terminal.clone(),
            logging: settings.logging.clone(),
            computer_use: settings.computer_use,
            cwd: None,
            shells: Vec::new(),
            active: None,
            starting: false,
            start_next: None,
            pending_start: None,
            restored: false,
            grid: None,
            origin: Point::default(),
            resize_timer: None,
            selecting: false,
            mouse_held: None,
            mouse_cell: None,
            blink_from: std::time::Instant::now(),
            blink_timer: None,
            focus_terminal: false,
            error: None,
            shell_choices: Vec::new(),
            picked,
            menu: None,
            menu_dismissed: None,
            output: OutputState::default(),
            screen: ScreenState::default(),
        }
    }

    /// Takes the settings and the open project's folder as they are now.
    pub fn sync(&mut self, settings: &Settings, cwd: Option<String>, cx: &mut Context<Self>) {
        let resized = self.settings.font_size != settings.terminal.font_size
            || self.settings.line_height != settings.terminal.line_height
            || self.settings.font_family != settings.terminal.font_family;
        self.settings = settings.terminal.clone();
        self.logging = settings.logging.clone();
        self.computer_use = settings.computer_use;
        self.cwd = cwd;
        if resized {
            self.grid = None;
        }
        if !self.computer_use && self.tab == PanelTab::Screen {
            self.tab = PanelTab::Terminal;
        }
        cx.notify();
    }

    /// Shows the panel on `tab`, starting what that view needs: a shell when the terminal has
    /// none, the logs, or a picture of the screen.
    pub fn show(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        let tab = if tab == PanelTab::Screen && !self.computer_use {
            PanelTab::Terminal
        } else {
            tab
        };
        self.tab = tab;
        self.visible = true;
        self.follow_output(tab == PanelTab::Output, cx);
        match tab {
            PanelTab::Terminal => {
                self.find_shells(cx);
                self.restore_then_start(false, cx);
            }
            PanelTab::Screen if self.screen.capture.is_none() => self.capture(cx),
            _ => {}
        }
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        self.menu = None;
        self.follow_output(false, cx);
        cx.notify();
    }

    /// Starts another terminal, in the shell picked last.
    pub fn new_terminal(&mut self, cx: &mut Context<Self>) {
        self.show(PanelTab::Terminal, cx);
        self.start(true, None, cx);
    }

    /// Moves focus into the terminal, now or once its shell is up.
    pub fn focus_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shells.is_empty() {
            self.focus_terminal = true;
        } else {
            window.focus(&self.term_focus, cx);
        }
    }

    pub fn terminal_focused(&self, window: &Window) -> bool {
        self.term_focus.is_focused(window)
    }

    /// Whether focus is anywhere in the panel.
    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    fn find_shells(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async { patchwork_server::terminal::shells() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |this, cx| {
                this.shell_choices = found;
                cx.notify();
            })
        })
        .detach();
    }

    fn pick_shell(&mut self, path: String, cx: &mut Context<Self>) {
        if let Err(e) = self.server.store.set_state(SHELL_KEY, &path) {
            log::warn!("could not keep the terminal's shell: {e}");
        }
        self.picked = Some(path.clone());
        self.start(true, Some(path), cx);
    }

    fn set_level(
        &mut self,
        setting: fn(&mut LoggingSettings) -> &mut LogLevel,
        level: LogLevel,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.server.store.settings();
        *setting(&mut next.logging) = level;
        match self.server.save_settings(next) {
            Ok(()) => {
                *setting(&mut self.logging) = level;
                cx.emit(PanelEvent::SettingsChanged);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Opens menu `kind` at the pointer, or closes it when it is open. The press that closed it
    /// does not open it again.
    fn toggle_menu(
        &mut self,
        kind: MenuFor,
        label: &str,
        items: Vec<MenuItem>,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing_press = matches!(e, ClickEvent::Mouse(click)
            if self.menu_dismissed.take() == Some(click.down.position));
        if closing_press || self.menu.as_ref().is_some_and(|(k, _, _)| *k == kind) {
            self.menu = None;
            return cx.notify();
        }
        self.open_menu(kind, label, items, e.position(), window, cx);
    }

    fn open_menu(
        &mut self,
        kind: MenuFor,
        label: &str,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = Menu::open(label.to_string(), items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, window, cx| {
            this.menu_dismissed = e.0;
            this.menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        })
        .detach();
        self.menu = Some((kind, menu, at));
        cx.notify();
    }

    fn menu_open(&self, kind: &MenuFor) -> bool {
        self.menu.as_ref().is_some_and(|(k, _, _)| k == kind)
    }

    fn shells_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.menu_open(&MenuFor::Shells) {
            self.find_shells(cx);
        }
        let this = cx.entity().downgrade();
        let picked = self
            .shell_choices
            .iter()
            .find(|s| Some(&s.path) == self.picked.as_ref())
            .or(self.shell_choices.first())
            .map(|s| s.path.clone());
        let items = self
            .shell_choices
            .iter()
            .map(|s| {
                let (path, this) = (s.path.clone(), this.clone());
                MenuItem::new(s.name.clone(), move |_, cx| {
                    let path = path.clone();
                    let _ = this.update(cx, |p, cx| p.pick_shell(path, cx));
                })
                .radio(picked.as_ref() == Some(&s.path))
            })
            .collect();
        self.toggle_menu(MenuFor::Shells, "Shells", items, e, window, cx);
    }

    fn logs_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let current = self.output.channel().map(|c| c.id.clone());
        let items = self
            .output
            .channels
            .iter()
            .map(|c| {
                let (id, this) = (c.id.clone(), this.clone());
                MenuItem::new(c.name.clone(), move |_, cx| {
                    let id = id.clone();
                    let _ = this.update(cx, |p, cx| p.pick_channel(id, cx));
                })
                .radio(current.as_ref() == Some(&c.id))
            })
            .collect();
        self.toggle_menu(MenuFor::Logs, "Logs", items, e, window, cx);
    }

    fn level_menu(&mut self, e: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.output.channel().cloned() else {
            return;
        };
        let this = cx.entity().downgrade();
        let current = *(channel.setting)(&mut self.logging.clone());
        let items = crate::settings::LOG_LEVELS
            .iter()
            .map(|&(level, label)| {
                let this = this.clone();
                let setting = channel.setting;
                MenuItem::new(label, move |_, cx| {
                    let _ = this.update(cx, |p, cx| p.set_level(setting, level, cx));
                })
                .radio(level == current)
            })
            .collect();
        let label = format!("{} log level", channel.name);
        self.toggle_menu(MenuFor::Level, &label, items, e, window, cx);
    }

    fn tab_button(
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

    fn header_tools(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                    .on_click(cx.listener(|this, e, window, cx| this.shells_menu(e, window, cx)))
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
                    crate::settings::dropdown_chip(
                        "output-log",
                        format!("Log: {}", name.as_deref().unwrap_or("none")),
                        name.clone().unwrap_or_else(|| "No logs yet".into()),
                        self.menu_open(&MenuFor::Logs),
                        &t,
                    )
                    .on_click(cx.listener(|this, e, window, cx| this.logs_menu(e, window, cx)))
                    .into_any_element(),
                ];
                if let Some(channel) = channel {
                    let level = *(channel.setting)(&mut self.logging.clone());
                    let current = crate::settings::LOG_LEVELS
                        .iter()
                        .find(|(l, _)| *l == level)
                        .map_or("", |(_, label)| label);
                    tools.push(
                        crate::settings::dropdown_chip(
                            "output-level",
                            format!("{} log level: {current}", channel.name),
                            current,
                            self.menu_open(&MenuFor::Level),
                            &t,
                        )
                        .on_click(cx.listener(|this, e, window, cx| this.level_menu(e, window, cx)))
                        .into_any_element(),
                    );
                }
                let logs = patchwork_server::system::dirs().logs.clone();
                tools.push(
                    tool("output-folder", "Open logs folder", "folder-open", cx)
                        .on_click(move |_, _, cx| {
                            let _ = std::fs::create_dir_all(&logs);
                            cx.open_with_system(&logs)
                        })
                        .into_any_element(),
                );
                tools
            }
            PanelTab::Screen => Vec::new(),
        }
    }

    /// The terminals, listed beside the active one once there is more than one.
    fn shell_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
                b.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
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

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        if self.focus_terminal && !self.shells.is_empty() {
            self.focus_terminal = false;
            window.focus(&self.term_focus, cx);
        }
        let tools = self.header_tools(cx);
        let body = match self.tab {
            PanelTab::Terminal => {
                let tabs = (self.shells.len() > 1).then(|| self.shell_tabs(cx));
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .pl(px(12.))
                            .bg(t.bg_editor)
                            .child(self.terminal_view(window, cx)),
                    )
                    .children(tabs)
                    .into_any_element()
            }
            PanelTab::Output => div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.output_view(window, cx))
                .into_any_element(),
            PanelTab::Screen => div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.screen_view(window, cx))
                .into_any_element(),
        };
        div()
            .id("panel")
            .role(Role::Region)
            .aria_label("Panel")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(t.border)
            .bg(t.bg_editor)
            .child(
                div()
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .child(
                        div()
                            .id("panel-tabs")
                            .role(Role::TabList)
                            .aria_label("Panel views")
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(self.tab_button(PanelTab::Terminal, "Terminal", cx))
                            .child(self.tab_button(PanelTab::Output, "Output", cx))
                            .when(self.computer_use, |tabs| {
                                tabs.child(self.tab_button(PanelTab::Screen, "Screen", cx))
                            }),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .children(tools)
                            .child(
                                icon_button("panel-close", "Close panel", "x", 14., cx)
                                    .size(px(24.))
                                    .tooltip(tooltip("Close panel"))
                                    .on_click(
                                        cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
                                    ),
                            ),
                    ),
            )
            .children(self.error.clone().map(|e| {
                div()
                    .id("panel-error")
                    .role(Role::Alert)
                    .flex_none()
                    .px(px(12.))
                    .pb(px(4.))
                    .text_size(px(FS_SM))
                    .text_color(t.danger)
                    .child(e)
            }))
            .child(body)
            .children(
                self.menu
                    .as_ref()
                    .map(|(_, menu, at)| Menu::view(menu, *at)),
            )
    }
}
