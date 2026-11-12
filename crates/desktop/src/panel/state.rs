//! The panel's state: its tabs, its terminals, and showing or hiding it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{App, Entity, EventEmitter, FocusHandle, Pixels, Point, Task, Window, prelude::*};
use patchwork_server::Server;
use patchwork_server::settings::{LogLevel, LoggingSettings, Settings, TerminalSettings};
use patchwork_server::terminal::TerminalShell;
use crate::ui::menu::Menu;
use super::output::OutputState;
use super::screen::ScreenState;
use super::terminal::Shell;

/// Where the shell last picked for new terminals is kept.
pub(super) const SHELL_KEY: &str = "patchwork-terminal-shell";

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
    /// A command run in a terminal of its own ended: its exit code, zero too when its tab was
    /// closed, None when it did not start or was killed, and the end of what it printed.
    RunFinished { code: Option<u32>, output: String },
}

/// Which of the panel's menus is open.
#[derive(PartialEq)]
pub(super) enum MenuFor {
    Shells,
    Logs,
    Level,
    Shell(u32),
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct Panel {
    pub(super) server: Arc<Server>,
    pub tab: PanelTab,
    /// Whether the window shows the panel now.
    pub(super) visible: bool,
    pub(super) focus: FocusHandle,
    pub(super) term_focus: FocusHandle,
    pub(super) settings: TerminalSettings,
    pub(super) logging: LoggingSettings,
    pub(super) computer_use: bool,
    /// The folder new terminals start in: the open project's.
    pub(super) cwd: Option<String>,
    pub(super) shells: Vec<Shell>,
    pub(super) active: Option<u32>,
    pub(super) starting: bool,
    /// A "New terminal" asked for while a shell starts, opened once that one is up.
    #[expect(
        clippy::option_option,
        reason = "None when none was asked for, else the shell asked for, which may be the default"
    )]
    pub(super) start_next: Option<Option<String>>,
    /// A shell asked for before the screen had a size.
    pub(super) pending_start: Option<(bool, Option<String>)>,
    pub(super) restored: bool,
    /// The screen's columns and rows, and where it sits in the window.
    pub(super) grid: Option<(u16, u16)>,
    pub(super) origin: Point<Pixels>,
    pub(super) resize_timer: Option<Task<()>>,
    /// A drag selecting on the screen is under way.
    pub(super) selecting: bool,
    /// The button held down over a program that hears the mouse, and the cell last reported.
    pub(super) mouse_held: Option<u8>,
    pub(super) mouse_cell: Option<(u16, u16)>,
    /// When the cursor's blink last started over, as typing shows it at once.
    pub(super) blink_from: std::time::Instant,
    pub(super) blink_timer: Option<Task<()>>,
    /// Focus moves into the terminal at the next draw, once the shell asked for is up.
    pub(super) focus_terminal: bool,
    pub(super) error: Option<String>,
    pub(super) shell_choices: Vec<TerminalShell>,
    pub(super) picked: Option<String>,
    pub(super) menu: Option<(MenuFor, Entity<Menu>, Point<Pixels>)>,
    pub(super) menu_dismissed: Option<Point<Pixels>>,
    pub(super) output: OutputState,
    pub(super) screen: ScreenState,
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
        #[expect(
            clippy::float_cmp,
            reason = "compares the saved settings, not computed values"
        )]
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

    pub(super) fn find_shells(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn pick_shell(&mut self, path: String, cx: &mut Context<Self>) {
        if let Err(e) = self.server.store.set_state(SHELL_KEY, &path) {
            log::warn!(error:% = e; "could not keep the terminal's shell: {{error}}");
        }
        self.picked = Some(path.clone());
        self.start(true, Some(path), cx);
    }

    pub(super) fn set_level(
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
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}
