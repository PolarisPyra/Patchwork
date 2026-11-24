//! A shell this window shows: starting it, feeding its frames, sizing it and closing it.

// Rust guideline compliant 2026-10-09

use crate::workspace::dock::MenuFor;
use gpui::ClickEvent;
use patchwork_ui::controls::menu::MenuItem;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{Bounds, FontWeight, Pixels, SharedString, Task, Window, font, prelude::*, px};
use patchwork_core::settings::TerminalSettings;
use patchwork_core::terminal::{AttachedTerminal, FrameSink, TerminalFrame};

use crate::workspace::dock::Dock;
use patchwork_ui::theme::font_mono;

/// How long the size waits to settle before the shell is told: each resize makes it redraw its
/// prompt, so a drag would leave a trail of stale prompts behind.
const RESIZE_SETTLE: Duration = Duration::from_millis(100);

/// The columns and rows a command run before the terminal was ever shown starts with.
const HIDDEN_GRID: (u16, u16) = (120, 40);

/// A shell this window shows.
pub struct Shell {
    pub id: u32,
    attachment: u32,
    /// The shell's file name, such as "zsh".
    pub name: String,
    /// The SSH host it runs on, or None for this computer.
    pub host: Option<String>,
    /// The folder it started in.
    cwd: String,
    /// The project or worktree folder it was opened for, when one was active.
    workspace: Option<String>,
    /// The app opened it for that folder rather than the user asking for a terminal, so opening
    /// another project's folder closes it and takes its place.
    auto: bool,
    /// Set once the shell has ended with an error, while its last output stays readable.
    pub exit_code: Option<u32>,
    /// It runs one command rather than a shell, and has yet to say how the command ended.
    task: bool,
    pub(crate) parser: vt100::Parser,
    /// What is typed, written to the shell in order.
    pub(crate) input: UnboundedSender<String>,
    /// What is selected: where it started and where it is, on the lines kept.
    pub(crate) selection: Option<super::select::Span>,
    /// The selection is a block of columns, as Alt+drag makes.
    pub(crate) block: bool,
    /// Wheel movement short of a whole line.
    pub(crate) scroll_rest: f32,
    _frames: Task<()>,
    _writer: Task<()>,
}

/// The size of a character cell, in the terminal's font.
#[derive(Clone)]
pub struct Metrics {
    pub(crate) family: SharedString,
    pub(crate) size: Pixels,
    pub(crate) weight: FontWeight,
    pub(crate) bold: FontWeight,
    pub(crate) cell_w: f32,
    pub(crate) line_h: f32,
}

impl Metrics {
    pub fn new(settings: &TerminalSettings, window: &Window) -> Self {
        let family: SharedString = settings
            .font_family
            .clone()
            .filter(|f| !f.trim().is_empty())
            .unwrap_or_else(|| font_mono().to_string())
            .into();
        let size = px(settings.font_size);
        let text = window.text_system();
        let id = text.resolve_font(&font(family.clone()));
        let cell_w = text
            .advance(id, size, 'm')
            .map_or(settings.font_size * 0.6, |a| f32::from(a.width));
        Self {
            family,
            size,
            weight: weight(&settings.font_weight, FontWeight::NORMAL),
            bold: weight(&settings.font_weight_bold, FontWeight::BOLD),
            cell_w,
            line_h: (settings.font_size * 1.2 * settings.line_height).round(),
        }
    }

    /// The columns and rows that fit in `bounds`.
    fn grid(&self, bounds: Bounds<Pixels>) -> (u16, u16) {
        let cols = (f32::from(bounds.size.width) / self.cell_w).floor().max(2.);
        let rows = (f32::from(bounds.size.height) / self.line_h)
            .floor()
            .max(1.);
        (cols.min(500.) as u16, rows.min(300.) as u16)
    }
}

/// A font weight as the settings write it: "normal", "bold", or a number.
fn weight(name: &str, fallback: FontWeight) -> FontWeight {
    match name {
        "normal" => FontWeight::NORMAL,
        "bold" => FontWeight::BOLD,
        n => n.parse::<f32>().map_or(fallback, FontWeight),
    }
}

/// The frames of a shell starting, kept until its id is known, then fed to it.
fn frames() -> (
    FrameSink,
    futures::channel::mpsc::UnboundedReceiver<TerminalFrame>,
) {
    let (tx, rx) = unbounded();
    (
        Box::new(move |frame| tx.unbounded_send(frame.clone()).is_ok()),
        rx,
    )
}

/// Whether `a` and `b` name the same local or remote folder.
pub(crate) fn same_folder(a: &str, b: &str) -> bool {
    if patchwork_core::system::ssh::is_remote(a) || patchwork_core::system::ssh::is_remote(b) {
        a.trim_end_matches('/') == b.trim_end_matches('/')
    } else {
        std::path::Path::new(a) == std::path::Path::new(b)
    }
}

impl Shell {
    /// Whether this shell was opened for `folder`.
    pub(crate) fn matches_folder(&self, folder: &str) -> bool {
        same_folder(self.workspace.as_deref().unwrap_or(&self.cwd), folder)
    }
}

impl Dock {
    /// Starts a shell: `chosen`, else the one last picked while it is still installed, else the
    /// user's. `asked` is a "New terminal" that comes after the one starting now.
    pub(crate) fn start(&mut self, asked: bool, chosen: Option<String>, cx: &mut Context<Self>) {
        if self.starting {
            if asked {
                self.start_next = Some(chosen);
            }
            return;
        }
        let Some((cols, rows)) = self.grid else {
            // The screen has no size yet; it starts once the panel is laid out.
            if asked || self.pending_start.is_none() {
                self.pending_start = Some((asked, chosen));
            }
            return;
        };
        self.starting = true;
        self.error = None;
        let core = Arc::clone(&self.core);
        let cwd = self.cwd.clone();
        let workspace = cwd.clone();
        let host = self.host.clone();
        let saved = self.picked.clone();
        let (sink, rx) = frames();
        patchwork_ui::off_thread(
            cx,
            move || {
                let path = chosen.or_else(|| {
                    let found = patchwork_core::terminal::shells();
                    saved.filter(|s| found.iter().any(|f| &f.path == s))
                });
                let launch = patchwork_core::terminal::launch(
                    &core.settings(),
                    cwd.as_deref().map(std::path::Path::new),
                    host.as_deref(),
                    path.as_deref(),
                )?;
                core.terminals.spawn(launch, cols, rows, sink)
            },
            move |this, started, cx| {
                this.starting = false;
                let ok = match started {
                    Ok(attached) => {
                        let id = attached.info.id;
                        let same_host = attached.info.host == this.host;
                        let matches_current = match (&workspace, &this.cwd) {
                            (Some(w), Some(c)) => same_folder(w, c),
                            (None, None) => true,
                            _ => false,
                        };
                        this.add_shell(attached, workspace, !asked, rx, cx);
                        if same_host && (this.active.is_none() || matches_current) {
                            this.active = Some(id);
                        }
                        true
                    }
                    Err(e) => {
                        this.error = Some(e.to_string());
                        false
                    }
                };
                if let Some(next) = this.start_next.take() {
                    this.start(true, next, cx);
                } else if ok
                    && this.visible
                    && this.tab == crate::workspace::dock::DockTab::Terminal
                    && let Some(folder) = this.cwd.clone()
                {
                    this.switch_to_shell_in(&folder, cx);
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Types `text` into the terminal of the newest command [`Self::run`] started that still
    /// runs. Nothing when none does.
    pub fn type_in_task(&self, text: &str) {
        if let Some(shell) = self.shells.iter().rev().find(|shell| shell.task) {
            let _ = shell.input.unbounded_send(text.to_string());
        }
    }

    /// Runs the command `launch` starts in a terminal tab of its own, without showing the
    /// terminal: the tab is there when the user opens it. It closes when the command succeeds and
    /// stays when it fails, as a shell does, and [`crate::workspace::dock::DockEvent::RunFinished`] says
    /// how it ended.
    pub fn run(&mut self, launch: patchwork_core::terminal::Launch, cx: &mut Context<Self>) {
        self.run_named(launch, None, cx);
    }

    /// Runs the command `launch` starts as [`Self::run`] does, its tab called `name` rather than
    /// after the program's file.
    pub fn run_named(
        &mut self,
        launch: patchwork_core::terminal::Launch,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.restore(cx);
        // Before the terminal has been shown it has no size; it takes its own once it is.
        let (cols, rows) = self.grid.unwrap_or(HIDDEN_GRID);
        self.error = None;
        let core = Arc::clone(&self.core);
        let (sink, rx) = frames();
        let task =
            cx.background_spawn(async move { core.terminals.spawn(launch, cols, rows, sink) });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            this.update(cx, |this, cx| {
                match started {
                    Ok(attached) => {
                        let id = attached.info.id;
                        this.add_shell(attached, None, false, rx, cx);
                        if let Some(shell) = this.shells.iter_mut().find(|s| s.id == id) {
                            shell.task = true;
                            if let Some(name) = name {
                                shell.name = name;
                            }
                        }
                        this.active = Some(id);
                    }
                    Err(e) => cx.emit(crate::workspace::dock::DockEvent::RunFinished {
                        code: None,
                        output: e.to_string(),
                    }),
                }
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Attaches to the shells the app already has, once: a shell that ended cleanly is closed
    /// instead. Then switches to a shell in the active project's folder, or starts one when there
    /// is none or one was asked for.
    pub(crate) fn restore_then_start(&mut self, asked: bool, cx: &mut Context<Self>) {
        self.restore(cx);
        if asked {
            self.start(true, None, cx);
        } else if let Some(folder) = self.cwd.clone() {
            self.switch_to_shell_in(&folder, cx);
        } else if self.shown().next().is_none() {
            self.start(false, None, cx);
        }
    }

    /// Switches to a shell that started in `folder`, starting one there when none did. A shell the
    /// app opened for another project's folder is closed first, so moving between projects leaves
    /// the one terminal it replaced rather than a tab per project.
    pub fn switch_to_shell_in(&mut self, folder: &str, cx: &mut Context<Self>) {
        if self
            .active_shell()
            .is_some_and(|s| s.matches_folder(folder))
        {
            return;
        }
        let found = self
            .shown()
            .rev()
            .find(|s| !s.task && s.matches_folder(folder))
            .map(|s| s.id);
        if let Some(id) = found {
            self.active = Some(id);
            cx.notify();
            return;
        }
        let left: Vec<u32> = self
            .shown()
            .filter(|s| s.auto && !s.matches_folder(folder))
            .map(|s| s.id)
            .collect();
        for id in left {
            self.forget(id, cx);
        }
        self.start(false, None, cx);
    }

    pub(crate) fn restore(&mut self, cx: &mut Context<Self>) {
        if !self.restored {
            self.restored = true;
            for info in self.core.terminals.list() {
                if !info.running && info.exit_code.is_none_or(|code| code == 0) {
                    self.close_in_background(info.id, cx);
                    continue;
                }
                let (sink, rx) = frames();
                let Ok(attached) = self.core.terminals.attach(info.id, sink) else {
                    continue;
                };
                self.add_shell(attached, None, false, rx, cx);
            }
        }
    }

    fn add_shell(
        &mut self,
        attached: AttachedTerminal,
        workspace: Option<String>,
        auto: bool,
        mut rx: futures::channel::mpsc::UnboundedReceiver<TerminalFrame>,
        cx: &mut Context<Self>,
    ) {
        let id = attached.info.id;
        let (cols, rows) = self.grid.unwrap_or(HIDDEN_GRID);
        let mut parser = vt100::Parser::new(rows, cols, self.settings.scrollback as usize);
        parser.process(attached.screen.as_bytes());
        let mut exit_code = None;
        if !attached.info.running
            && let Some(code) = attached.info.exit_code.filter(|&c| c != 0)
        {
            exit_code = Some(code);
            parser.process(exit_text(&attached.info.name, code).as_bytes());
        }
        let frames = cx.spawn(async move |this, cx| {
            while let Some(frame) = rx.next().await {
                // A busy shell sends many frames at once; they are drawn together.
                let mut batch = vec![frame];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                }
                if this
                    .update(cx, |this, cx| this.on_frames(id, batch, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let (input, mut typed) = unbounded::<String>();
        let core = Arc::clone(&self.core);
        let attachment = attached.attachment;
        let writer = cx.background_spawn(async move {
            while let Some(data) = typed.next().await {
                if let Err(e) = core.terminals.write(id, attachment, &data) {
                    log::warn!(
                        id:%, error:% = e;
                        "terminal {{id}}: could not type into it: {{error}}"
                    );
                }
            }
        });
        if self.grid.is_some_and(|g| g != (cols, rows)) || attached.screen.is_empty() {
            self.tell_size(id, attachment, cols, rows, cx);
        }
        self.shells.push(Shell {
            id,
            attachment,
            name: attached.info.name,
            host: attached.info.host,
            cwd: attached.info.cwd,
            workspace,
            auto,
            exit_code,
            task: false,
            parser,
            input,
            selection: None,
            block: false,
            scroll_rest: 0.,
            _frames: frames,
            _writer: writer,
        });
    }

    fn on_frames(&mut self, id: u32, frames: Vec<TerminalFrame>, cx: &mut Context<Self>) {
        let Some(shell) = self.shells.iter_mut().find(|s| s.id == id) else {
            return;
        };
        for frame in frames {
            // A run says how it ended, once, then closes or stays as any shell does.
            if let TerminalFrame::Exit { code } = &frame
                && std::mem::take(&mut shell.task)
            {
                let output = last_lines(&shell.parser.screen().contents());
                cx.emit(crate::workspace::dock::DockEvent::RunFinished {
                    code: *code,
                    output,
                });
            }
            match frame {
                TerminalFrame::Output { data } => shell.process(data.as_bytes()),
                // A shell that failed stays, so its last output can be read; one that ended
                // cleanly closes.
                TerminalFrame::Exit { code: Some(code) }
                    if code != 0 && shell.exit_code.is_none() =>
                {
                    shell.exit_code = Some(code);
                    let text = exit_text(&shell.name, code);
                    shell.process(text.as_bytes());
                }
                TerminalFrame::Exit { .. } => {
                    self.kill(id, cx);
                    return;
                }
            }
        }
        cx.notify();
    }

    /// Takes the shell's tab away now, and ends its processes behind it. The last one going
    /// closes the panel.
    pub(crate) fn kill(&mut self, id: u32, cx: &mut Context<Self>) {
        let shown = self.forget(id, cx);
        if shown
            && self.shown().next().is_none()
            && self.tab == crate::workspace::dock::DockTab::Terminal
        {
            cx.emit(crate::workspace::dock::DockEvent::Close);
        }
        cx.notify();
    }

    /// Takes the shell's tab away and ends its processes behind it, without closing the panel
    /// when it was the last one: another shell takes its place. Says whether its tab was one of
    /// those the terminal shows.
    fn forget(&mut self, id: u32, cx: &mut Context<Self>) -> bool {
        // A run whose tab is closed has ended without failing.
        if let Some(shell) = self.shells.iter_mut().find(|s| s.id == id)
            && std::mem::take(&mut shell.task)
        {
            cx.emit(crate::workspace::dock::DockEvent::RunFinished {
                code: Some(0),
                output: String::new(),
            });
        }
        let Some(at) = self.shells.iter().position(|s| s.id == id) else {
            return false;
        };
        let shown = self.shells.remove(at).host == self.host;
        if self.active == Some(id) {
            self.active = None;
        }
        self.close_in_background(id, cx);
        shown
    }

    fn close_in_background(&self, id: u32, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.terminals.close(id) {
                log::warn!(id:%, error:% = e; "terminal {{id}}: could not close it: {{error}}");
            }
        })
        .detach();
    }

    /// The shells on the machine the terminal shows.
    pub(crate) fn shown(&self) -> impl DoubleEndedIterator<Item = &Shell> {
        self.shells.iter().filter(|s| s.host == self.host)
    }

    pub(crate) fn active_shell(&self) -> Option<&Shell> {
        self.shown()
            .find(|s| Some(s.id) == self.active)
            .or_else(|| self.shown().last())
    }

    pub(crate) fn active_shell_mut(&mut self) -> Option<&mut Shell> {
        let id = self.active_shell().map(|s| s.id)?;
        self.shells.iter_mut().find(|s| s.id == id)
    }

    /// Follows the screen's laid-out size: every parser takes it now, and the shells once it
    /// settles.
    pub(crate) fn fit(
        &mut self,
        bounds: Bounds<Pixels>,
        metrics: &Metrics,
        cx: &mut Context<Self>,
    ) {
        self.origin = bounds.origin;
        let grid = metrics.grid(bounds);
        if self.grid == Some(grid) {
            return;
        }
        self.grid = Some(grid);
        let (cols, rows) = grid;
        for shell in &mut self.shells {
            shell.parser.screen_mut().set_size(rows, cols);
            shell.selection = None;
        }
        let shells: Vec<(u32, u32)> = self.shells.iter().map(|s| (s.id, s.attachment)).collect();
        let core = Arc::clone(&self.core);
        self.resize_timer = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(RESIZE_SETTLE).await;
            cx.background_spawn(async move {
                for (id, attachment) in shells {
                    if let Err(e) = core.terminals.resize(id, attachment, cols, rows) {
                        log::warn!(error:% = e; "could not resize a terminal: {{error}}");
                    }
                }
            })
            .await;
        }));
        if let Some((asked, chosen)) = self.pending_start.take() {
            self.start(asked, chosen, cx);
        }
        cx.notify();
    }

    fn tell_size(&self, id: u32, attachment: u32, cols: u16, rows: u16, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.terminals.resize(id, attachment, cols, rows) {
                log::warn!(error:% = e; "could not resize a terminal: {{error}}");
            }
        })
        .detach();
    }
}

/// The end of what a screen shows, without its blank lines: where a build says what failed.
fn last_lines(screen: &str) -> String {
    /// Enough to hold a compiler error and the task that failed.
    const LINES: usize = 40;
    let lines: Vec<&str> = screen
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(LINES)..].join("\n")
}

fn exit_text(name: &str, code: u32) -> String {
    format!(
        "\r\n\x1b[90mThe terminal process \"{name}\" terminated with exit code: {code}.\x1b[0m\r\n"
    )
}

impl Dock {
    /// Opens the menu of shells a new terminal can start.
    pub(crate) fn shells_menu(
        &mut self,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
}
