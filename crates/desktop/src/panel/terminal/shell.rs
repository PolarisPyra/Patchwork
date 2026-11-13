//! A shell this window shows: starting it, feeding its frames, sizing it and closing it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{Bounds, FontWeight, Pixels, SharedString, Task, Window, font, prelude::*, px};
use patchwork_server::settings::TerminalSettings;
use patchwork_server::terminal::{AttachedTerminal, FrameSink, TerminalFrame};

use crate::panel::Panel;
use crate::ui::theme::FONT_MONO;

/// The page this window's shells are attached as.
const PAGE: &str = "native";

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
    /// Set once the shell has ended with an error, while its last output stays readable.
    pub exit_code: Option<u32>,
    /// It runs one command rather than a shell, and has yet to say how the command ended.
    task: bool,
    pub(super) parser: vt100::Parser,
    /// What is typed, written to the shell in order.
    pub(super) input: UnboundedSender<String>,
    /// A drag over the screen: where it started and where it is, as (row, column).
    pub(super) selection: Option<((u16, u16), (u16, u16))>,
    /// Wheel movement short of a whole line.
    pub(super) scroll_rest: f32,
    _frames: Task<()>,
    _writer: Task<()>,
}

/// The size of a character cell, in the terminal's font.
#[derive(Clone)]
pub struct Metrics {
    pub(in crate::panel) family: SharedString,
    pub(in crate::panel) size: Pixels,
    pub(in crate::panel) weight: FontWeight,
    pub(in crate::panel) bold: FontWeight,
    pub(in crate::panel) cell_w: f32,
    pub(in crate::panel) line_h: f32,
}

impl Metrics {
    pub fn new(settings: &TerminalSettings, window: &Window) -> Self {
        let family: SharedString = settings
            .font_family
            .clone()
            .filter(|f| !f.trim().is_empty())
            .unwrap_or_else(|| FONT_MONO.to_string())
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

impl Panel {
    /// Starts a shell: `chosen`, else the one last picked while it is still installed, else the
    /// user's. `asked` is a "New terminal" that comes after the one starting now.
    pub(in crate::panel) fn start(
        &mut self,
        asked: bool,
        chosen: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.starting {
            if asked {
                self.start_next = Some(chosen);
            }
            return;
        }
        let Some((cols, rows)) = self.grid else {
            // The screen has no size yet; it starts once the panel is laid out.
            self.pending_start = Some((asked, chosen));
            return;
        };
        self.starting = true;
        self.error = None;
        let server = Arc::clone(&self.server);
        let cwd = self.cwd.clone();
        let host = self.host.clone();
        let saved = self.picked.clone();
        let (sink, rx) = frames();
        let task = cx.background_spawn(async move {
            let path = chosen.or_else(|| {
                let found = patchwork_server::terminal::shells();
                saved.filter(|s| found.iter().any(|f| &f.path == s))
            });
            let launch = patchwork_server::terminal::launch(
                &server.store.settings(),
                cwd.as_deref().map(std::path::Path::new),
                host.as_deref(),
                path.as_deref(),
            )?;
            server.terminals.spawn(launch, cols, rows, PAGE, sink)
        });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            this.update(cx, |this, cx| {
                this.starting = false;
                match started {
                    Ok(attached) => {
                        let id = attached.info.id;
                        this.add_shell(attached, rx, cx);
                        this.active = Some(id);
                        this.focus_terminal = true;
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                if let Some(next) = this.start_next.take() {
                    this.start(true, next, cx);
                }
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    /// Runs the command `launch` starts in a terminal tab of its own, without showing the
    /// terminal: the tab is there when the user opens it. It closes when the command succeeds and
    /// stays when it fails, as a shell does, and [`crate::panel::PanelEvent::RunFinished`] says
    /// how it ended.
    pub fn run(&mut self, launch: patchwork_server::terminal::Launch, cx: &mut Context<Self>) {
        self.restore(cx);
        // Before the terminal has been shown it has no size; it takes its own once it is.
        let (cols, rows) = self.grid.unwrap_or(HIDDEN_GRID);
        self.error = None;
        let server = Arc::clone(&self.server);
        let (sink, rx) = frames();
        let task =
            cx.background_spawn(
                async move { server.terminals.spawn(launch, cols, rows, PAGE, sink) },
            );
        cx.spawn(async move |this, cx| {
            let started = task.await;
            this.update(cx, |this, cx| {
                match started {
                    Ok(attached) => {
                        let id = attached.info.id;
                        this.add_shell(attached, rx, cx);
                        if let Some(shell) = this.shells.iter_mut().find(|s| s.id == id) {
                            shell.task = true;
                        }
                        this.active = Some(id);
                    }
                    Err(e) => cx.emit(crate::panel::PanelEvent::RunFinished {
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
    /// instead. Then starts one when there is none, or when one was asked for.
    pub(in crate::panel) fn restore_then_start(&mut self, asked: bool, cx: &mut Context<Self>) {
        self.restore(cx);
        if asked || self.shown().next().is_none() {
            self.start(asked, None, cx);
        }
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        if !self.restored {
            self.restored = true;
            for info in self.server.terminals.list() {
                if !info.running && info.exit_code.is_none_or(|code| code == 0) {
                    self.close_in_background(info.id, cx);
                    continue;
                }
                let (sink, rx) = frames();
                let Ok(attached) = self.server.terminals.attach(info.id, PAGE, sink) else {
                    continue;
                };
                self.add_shell(attached, rx, cx);
            }
        }
    }

    fn add_shell(
        &mut self,
        attached: AttachedTerminal,
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
        let server = Arc::clone(&self.server);
        let attachment = attached.attachment;
        let writer = cx.background_spawn(async move {
            while let Some(data) = typed.next().await {
                if let Err(e) = server.terminals.write(id, attachment, &data) {
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
            exit_code,
            task: false,
            parser,
            input,
            selection: None,
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
                cx.emit(crate::panel::PanelEvent::RunFinished {
                    code: *code,
                    output,
                });
            }
            match frame {
                TerminalFrame::Output { data } => shell.parser.process(data.as_bytes()),
                // A shell that failed stays, so its last output can be read; one that ended
                // cleanly closes.
                TerminalFrame::Exit { code: Some(code) }
                    if code != 0 && shell.exit_code.is_none() =>
                {
                    shell.exit_code = Some(code);
                    shell
                        .parser
                        .process(exit_text(&shell.name, code).as_bytes());
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
    pub(in crate::panel) fn kill(&mut self, id: u32, cx: &mut Context<Self>) {
        // A run whose tab is closed has ended without failing.
        if let Some(shell) = self.shells.iter_mut().find(|s| s.id == id)
            && std::mem::take(&mut shell.task)
        {
            cx.emit(crate::panel::PanelEvent::RunFinished {
                code: Some(0),
                output: String::new(),
            });
        }
        let Some(at) = self.shells.iter().position(|s| s.id == id) else {
            return;
        };
        let shown = self.shells.remove(at).host == self.host;
        if self.active == Some(id) {
            self.active = None;
        }
        self.close_in_background(id, cx);
        if shown && self.shown().next().is_none() && self.tab == crate::panel::PanelTab::Terminal {
            cx.emit(crate::panel::PanelEvent::Close);
        }
        cx.notify();
    }

    fn close_in_background(&self, id: u32, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        cx.background_spawn(async move {
            if let Err(e) = server.terminals.close(id) {
                log::warn!(id:%, error:% = e; "terminal {{id}}: could not close it: {{error}}");
            }
        })
        .detach();
    }

    /// The shells on the machine the terminal shows.
    pub(in crate::panel) fn shown(&self) -> impl Iterator<Item = &Shell> {
        self.shells.iter().filter(|s| s.host == self.host)
    }

    pub(in crate::panel) fn active_shell(&self) -> Option<&Shell> {
        self.shown()
            .find(|s| Some(s.id) == self.active)
            .or_else(|| self.shown().last())
    }

    pub(super) fn active_shell_mut(&mut self) -> Option<&mut Shell> {
        let id = self.active_shell().map(|s| s.id)?;
        self.shells.iter_mut().find(|s| s.id == id)
    }

    /// Follows the screen's laid-out size: every parser takes it now, and the shells once it
    /// settles.
    pub(super) fn fit(
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
        let server = Arc::clone(&self.server);
        self.resize_timer = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(RESIZE_SETTLE).await;
            cx.background_spawn(async move {
                for (id, attachment) in shells {
                    let _ = server.terminals.resize(id, attachment, cols, rows);
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
        let server = Arc::clone(&self.server);
        cx.background_spawn(async move {
            let _ = server.terminals.resize(id, attachment, cols, rows);
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
