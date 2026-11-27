//! The dock's state: its tabs, its terminals, and showing or hiding it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{App, Entity, EventEmitter, FocusHandle, Pixels, Point, Task, Window, prelude::*};
use patchwork_backend::Core;
use patchwork_backend::settings::{LogLevel, LoggingSettings, Settings, TerminalSettings};
use patchwork_backend::terminal::TerminalShell;
use patchwork_ui_components::menu::Menu;
use crate::logs::OutputState;
use crate::screen::ScreenState;
use crate::terminal::Shell;

/// Where the shell last picked for new terminals is kept.
pub(crate) const SHELL_KEY: &str = "patchwork-terminal-shell";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockTab {
    Terminal,
    Output,
    Screen,
}

#[derive(Debug)]
pub enum DockEvent {
    /// The panel asks to close: its close button, or its last shell ended.
    Close,
    /// A log's level changed here.
    SettingsChanged,
    /// The panel was maximized or given back its own height.
    Maximized,
    /// A command run in a terminal of its own ended: its exit code, zero too when its tab was
    /// closed, None when it did not start or was killed, and the end of what it printed.
    RunFinished { code: Option<u32>, output: String },
}

/// Which of the panel's menus is open.
#[derive(PartialEq)]
pub(crate) enum MenuFor {
    /// The terminal chooser: the open terminals, find and kill.
    Terminals,
    Shells,
    /// The terminal's other actions: find, clear and maximize.
    More,
    Logs,
    Level,
    Shell(u32),
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct Dock {
    pub(crate) core: Arc<Core>,
    pub tab: DockTab,
    /// Whether the window shows the panel now.
    pub(crate) visible: bool,
    /// The panel's height as the user left it, and where a drag of its handle started: the
    /// pointer's y and the height then.
    pub(crate) height: f32,
    pub(crate) resizing: Option<(f32, f32)>,
    pub(crate) focus: FocusHandle,
    pub(crate) term_focus: FocusHandle,
    pub(crate) settings: TerminalSettings,
    pub(crate) logging: LoggingSettings,
    pub(crate) computer_use: bool,
    /// The machine whose terminals show: an SSH host's alias, or None for this computer.
    pub(crate) host: Option<String>,
    /// The folder new terminals start in: the open project's, when it is on that machine.
    pub(crate) cwd: Option<String>,
    pub(crate) shells: Vec<Shell>,
    pub(crate) active: Option<u32>,
    pub(crate) starting: bool,
    /// A "New terminal" asked for while a shell starts, opened once that one is up.
    #[expect(
        clippy::option_option,
        reason = "None when none was asked for, else the shell asked for, which may be the default"
    )]
    pub(crate) start_next: Option<Option<String>>,
    /// A shell asked for before the screen had a size.
    pub(crate) pending_start: Option<(bool, Option<String>)>,
    pub(crate) restored: bool,
    /// The columns and rows the shells have now, and where the first drawn row sits in the
    /// window.
    pub(crate) grid: Option<(u16, u16)>,
    pub(crate) origin: Point<Pixels>,
    /// The columns and rows the panel has room for, which `grid` takes once they hold still.
    pub(crate) target: Option<(u16, u16)>,
    pub(crate) resize_timer: Option<Task<()>>,
    /// The panel takes all the room the chat column can give it.
    pub(crate) maximized: bool,
    pub(crate) find: crate::terminal::FindState,
    /// A drag selecting on the screen is under way, and how it goes.
    pub(crate) selecting: bool,
    pub(crate) drag: crate::terminal::Drag,
    /// The button held down over a program that hears the mouse, and the cell last reported.
    pub(crate) mouse_held: Option<u8>,
    pub(crate) mouse_cell: Option<(u16, u16)>,
    /// When the cursor's blink last started over, as typing shows it at once.
    pub(crate) blink_from: std::time::Instant,
    pub(crate) blink_timer: Option<Task<()>>,
    /// Focus moves into the terminal at the next draw, once the shell asked for is up.
    pub(crate) focus_terminal: bool,
    pub(crate) error: Option<String>,
    pub(crate) shell_choices: Vec<TerminalShell>,
    pub(crate) picked: Option<String>,
    pub(crate) menu: Option<(MenuFor, Entity<Menu>, Point<Pixels>)>,
    pub(crate) menu_dismissed: Option<Point<Pixels>>,
    pub(crate) output: OutputState,
    pub(crate) screen: ScreenState,
}

impl std::fmt::Debug for Dock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panel")
            .field("tab", &self.tab)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<DockEvent> for Dock {}

impl Dock {
    pub fn new(core: Arc<Core>, settings: &Settings, cx: &mut Context<Self>) -> Self {
        let picked = core.state(SHELL_KEY);
        // A selection made elsewhere after the terminal's takes its place.
        cx.observe_global::<patchwork_ui_markdown::selection::Owner>(|this: &mut Self, cx| {
            if patchwork_ui_markdown::selection::owner(cx)
                == patchwork_ui_markdown::selection::Owner::Terminal
            {
                return;
            }
            for shell in &mut this.shells {
                if shell.selection.take().is_some() {
                    cx.notify();
                }
            }
        })
        .detach();
        let height = super::place::kept_height(&core);
        Self {
            core,
            tab: DockTab::Terminal,
            visible: false,
            height,
            resizing: None,
            focus: cx.focus_handle(),
            term_focus: cx.focus_handle(),
            settings: settings.terminal.clone(),
            logging: settings.logging.clone(),
            computer_use: settings.computer_use,
            host: None,
            cwd: None,
            shells: Vec::new(),
            active: None,
            starting: false,
            start_next: None,
            pending_start: None,
            restored: false,
            grid: None,
            origin: Point::default(),
            target: None,
            resize_timer: None,
            maximized: false,
            find: crate::terminal::FindState::default(),
            selecting: false,
            drag: crate::terminal::Drag::default(),
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

    /// Takes the settings, the machine and the open project's folder as they are now. A machine
    /// or folder change while the terminal shows brings up its own terminals, starting one when it
    /// has none.
    pub fn sync(
        &mut self,
        settings: &Settings,
        host: Option<String>,
        cwd: Option<String>,
        cx: &mut Context<Self>,
    ) {
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
        let cwd_changed = match (self.cwd.as_deref(), cwd.as_deref()) {
            (Some(a), Some(b)) => !crate::terminal::same_folder(a, b),
            (None, None) => false,
            _ => true,
        };
        self.cwd = cwd;
        if resized {
            self.grid = None;
            self.target = None;
        }
        if self.host != host {
            self.host = host;
            self.active = None;
            self.menu = None;
            if self.visible && self.tab == DockTab::Terminal {
                self.restore_then_start(false, cx);
                self.focus_terminal = true;
            }
        } else if cwd_changed
            && self.visible
            && self.tab == DockTab::Terminal
            && let Some(folder) = self.cwd.clone()
        {
            self.switch_to_shell_in(&folder, cx);
        }
        if !self.computer_use && self.tab == DockTab::Screen {
            self.tab = DockTab::Terminal;
        }
        cx.notify();
    }

    /// Shows the terminal tab without starting a default shell, when the caller is about to start
    /// its own shell or command.
    pub(crate) fn open_terminal_tab(&mut self, cx: &mut Context<Self>) {
        self.tab = DockTab::Terminal;
        self.visible = true;
        self.follow_output(false, cx);
        self.find_shells(cx);
        self.restore(cx);
        cx.notify();
    }

    /// Shows the panel on `tab`, starting what that view needs: a shell when the terminal has
    /// none, the logs, or a picture of the screen.
    pub fn show(&mut self, tab: DockTab, cx: &mut Context<Self>) {
        let tab = if tab == DockTab::Screen && !self.computer_use {
            DockTab::Terminal
        } else {
            tab
        };
        self.tab = tab;
        self.visible = true;
        self.follow_output(tab == DockTab::Output, cx);
        match tab {
            DockTab::Terminal => {
                self.find_shells(cx);
                self.restore_then_start(false, cx);
            }
            DockTab::Screen if self.screen.capture.is_none() => self.capture(cx),
            _ => {}
        }
        cx.notify();
    }

    pub fn hide(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        self.menu = None;
        // Focus asked for while the shell starts is dropped with the panel, so it does not take
        // the keys from the composer later.
        self.focus_terminal = false;
        self.follow_output(false, cx);
        cx.notify();
    }

    /// Starts another terminal, in the shell picked last.
    pub fn new_terminal(&mut self, cx: &mut Context<Self>) {
        self.open_terminal_tab(cx);
        self.start(true, None, cx);
    }

    /// Starts another terminal in the folder `cwd`, in the shell picked last.
    pub fn new_terminal_in(&mut self, cwd: String, cx: &mut Context<Self>) {
        self.cwd = Some(cwd);
        self.new_terminal(cx);
    }

    /// Moves focus into the terminal, now or once its shell is up.
    pub fn focus_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown().next().is_none() {
            self.focus_terminal = true;
        } else {
            window.focus(&self.term_focus, cx);
        }
    }

    #[must_use]
    pub fn terminal_focused(&self, window: &Window) -> bool {
        self.term_focus.is_focused(window)
    }

    /// Whether focus is anywhere in the panel.
    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    pub(crate) fn find_shells(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async { patchwork_backend::terminal::shells() });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            this.update(cx, |this, cx| {
                this.shell_choices = found;
                cx.notify();
            })
        })
        .detach();
    }

    pub(crate) fn pick_shell(&mut self, path: String, cx: &mut Context<Self>) {
        if let Err(e) = self.core.set_state(SHELL_KEY, &path) {
            log::warn!(error:% = e; "could not keep the terminal's shell: {{error}}");
        }
        self.picked = Some(path.clone());
        self.start(true, Some(path), cx);
    }

    /// Has the panel take all the room it may, or its own height again.
    pub(crate) fn set_maximized(&mut self, maximized: bool, cx: &mut Context<Self>) {
        self.maximized = maximized;
        self.menu = None;
        cx.emit(DockEvent::Maximized);
        cx.notify();
    }

    pub(crate) fn set_level(
        &mut self,
        setting: fn(&mut LoggingSettings) -> &mut LogLevel,
        level: LogLevel,
        cx: &mut Context<Self>,
    ) {
        let mut next = self.core.settings();
        *setting(&mut next.logging) = level;
        match self.core.update_settings(|s| *s = next) {
            Ok(()) => {
                *setting(&mut self.logging) = level;
                cx.emit(DockEvent::SettingsChanged);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}
