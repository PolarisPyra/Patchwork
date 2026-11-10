//! The integrated terminal's shells. Each is a shell the server
//! runs in a pseudo-terminal; this window keeps its screen in a `vt100` parser fed by the shell's
//! frames, and draws it as rows of styled text. Keys go to the shell as the bytes a terminal
//! sends, except the app's own panel, search and settings keys.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, FontWeight, KeyDownEvent, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Role, ScrollEvent, SharedString,
    Task, Window, canvas, div, font, prelude::*, px,
};
use patchwork_server::settings::TerminalSettings;
use patchwork_server::terminal::{AttachedTerminal, FrameSink, TerminalFrame};

use super::Panel;
use super::cells::{draw_screen, line_at, ordered, word_at};
use super::input::{Mouse, key_bytes, mouse_bytes};
use crate::ui::theme::{FONT_MONO, theme};

/// The page this window's shells are attached as.
const PAGE: &str = "native";

/// How long the size waits to settle before the shell is told: each resize makes it redraw its
/// prompt, so a drag would leave a trail of stale prompts behind.
const RESIZE_SETTLE: Duration = Duration::from_millis(100);

/// How long the cursor shows, then hides, while it blinks.
const BLINK: Duration = Duration::from_millis(600);

/// A shell this window shows.
pub struct Shell {
    pub id: u32,
    attachment: u32,
    /// The shell's file name, such as "zsh".
    pub name: String,
    /// Set once the shell has ended with an error, while its last output stays readable.
    pub exit_code: Option<u32>,
    parser: vt100::Parser,
    /// What is typed, written to the shell in order.
    input: UnboundedSender<String>,
    /// A drag over the screen: where it started and where it is, as (row, column).
    selection: Option<((u16, u16), (u16, u16))>,
    /// Wheel movement short of a whole line.
    scroll_rest: f32,
    _frames: Task<()>,
    _writer: Task<()>,
}

/// The size of a character cell, in the terminal's font.
#[derive(Clone)]
pub struct Metrics {
    pub(super) family: SharedString,
    pub(super) size: Pixels,
    pub(super) weight: FontWeight,
    pub(super) bold: FontWeight,
    pub(super) cell_w: f32,
    pub(super) line_h: f32,
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
    pub(super) fn start(&mut self, asked: bool, chosen: Option<String>, cx: &mut Context<Self>) {
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

    /// Attaches to the shells the app already has, once: a shell that ended cleanly is closed
    /// instead. Then starts one when there is none, or when one was asked for.
    pub(super) fn restore_then_start(&mut self, asked: bool, cx: &mut Context<Self>) {
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
        if asked || self.shells.is_empty() {
            self.start(asked, None, cx);
        }
    }

    fn add_shell(
        &mut self,
        attached: AttachedTerminal,
        mut rx: futures::channel::mpsc::UnboundedReceiver<TerminalFrame>,
        cx: &mut Context<Self>,
    ) {
        let id = attached.info.id;
        let (cols, rows) = self.grid.unwrap_or((80, 24));
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
            exit_code,
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
    pub(super) fn kill(&mut self, id: u32, cx: &mut Context<Self>) {
        let before = self.shells.len();
        self.shells.retain(|s| s.id != id);
        if self.shells.len() == before {
            return;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.close_in_background(id, cx);
        if self.shells.is_empty() && self.tab == super::PanelTab::Terminal {
            cx.emit(super::PanelEvent::Close);
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

    pub(super) fn active_shell(&self) -> Option<&Shell> {
        self.shells
            .iter()
            .find(|s| Some(s.id) == self.active)
            .or(self.shells.last())
    }

    fn active_shell_mut(&mut self) -> Option<&mut Shell> {
        let id = self.active_shell().map(|s| s.id)?;
        self.shells.iter_mut().find(|s| s.id == id)
    }

    /// Follows the screen's laid-out size: every parser takes it now, and the shells once it
    /// settles.
    fn fit(&mut self, bounds: Bounds<Pixels>, metrics: &Metrics, cx: &mut Context<Self>) {
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

    fn type_text(&mut self, data: String) {
        if let Some(shell) = self.active_shell_mut()
            && shell.exit_code.is_none()
        {
            // Typing brings the view back to the prompt.
            shell.parser.screen_mut().set_scrollback(0);
            shell.selection = None;
            let _ = shell.input.unbounded_send(data);
            self.blink_from = Instant::now();
        }
    }

    /// Tells the program on the screen what the mouse did, when it asked to hear of it and Shift,
    /// which keeps the mouse for selecting, is up. Returns whether it was told.
    fn report_mouse(
        &mut self,
        event: Mouse,
        position: Point<Pixels>,
        m: Modifiers,
        metrics: &Metrics,
    ) -> bool {
        if m.shift {
            return false;
        }
        let cell = self.cell_under(position, metrics);
        let moved = matches!(event, Mouse::Drag(_) | Mouse::Move);
        let Some(shell) = self.active_shell() else {
            return false;
        };
        let Some(bytes) = mouse_bytes(shell.parser.screen(), event, cell, m) else {
            return false;
        };
        // A move within the cell last reported says nothing new.
        if !(moved && self.mouse_cell == Some(cell)) {
            let _ = shell.input.unbounded_send(bytes);
        }
        self.mouse_cell = Some(cell);
        true
    }

    /// `altClickMovesCursor`: Alt+click on the prompt sends the arrow keys that walk the cursor
    /// there, counting cells across wrapped lines. Returns whether it did.
    fn alt_click(&mut self, position: Point<Pixels>, metrics: &Metrics) -> bool {
        let (row, col) = self.cell_under(position, metrics);
        let Some(shell) = self.active_shell() else {
            return false;
        };
        let screen = shell.parser.screen();
        if screen.alternate_screen() || screen.scrollback() > 0 {
            return false;
        }
        let (cur_row, cur_col) = screen.cursor_position();
        let cols = i32::from(screen.size().1);
        let cells =
            (i32::from(row) - i32::from(cur_row)) * cols + i32::from(col) - i32::from(cur_col);
        let key = match (cells > 0, screen.application_cursor()) {
            (true, true) => "\x1bOC",
            (true, false) => "\x1b[C",
            (false, true) => "\x1bOD",
            (false, false) => "\x1b[D",
        };
        self.type_text(key.repeat(cells.unsigned_abs() as usize));
        true
    }

    fn copy_selection(&self, cx: &mut App) -> bool {
        let Some(text) = self.active_shell().and_then(selected_text) else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    fn paste(&mut self, cx: &mut App) {
        let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) else {
            return;
        };
        let bracketed = self
            .active_shell()
            .is_some_and(|s| s.parser.screen().bracketed_paste());
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        self.type_text(if bracketed {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text
        });
    }

    pub(super) fn on_terminal_key(
        &mut self,
        e: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ks = &e.keystroke;
        let m = &ks.modifiers;
        // Ctrl+Shift+C and V copy and paste, as Ctrl+C and V belong to the shell.
        let copy_paste = if cfg!(target_os = "macos") {
            m.platform && !m.shift
        } else {
            m.control && m.shift
        };
        if copy_paste && ks.key == "c" {
            if self.copy_selection(cx) {
                cx.stop_propagation();
            }
            return;
        }
        if copy_paste && ks.key == "v" {
            self.paste(cx);
            cx.stop_propagation();
            return;
        }
        let app_cursor = self
            .active_shell()
            .is_some_and(|s| s.parser.screen().application_cursor());
        let Some(bytes) = key_bytes(ks, app_cursor) else {
            return;
        };
        self.type_text(bytes);
        cx.stop_propagation();
        cx.notify();
    }

    fn on_terminal_scroll(&mut self, e: &ScrollEvent, metrics: &Metrics, cx: &mut Context<Self>) {
        let speed = if e.modifiers.alt {
            self.settings.fast_scroll_sensitivity
        } else {
            self.settings.mouse_wheel_scroll_sensitivity
        };
        let line_h = px(metrics.line_h);
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let lines =
            f32::from(e.delta.pixel_delta(line_h).y) / metrics.line_h * speed + shell.scroll_rest;
        let whole = lines.trunc();
        shell.scroll_rest = lines - whole;
        if whole == 0. {
            return;
        }
        let wheel = if whole > 0. {
            Mouse::WheelUp
        } else {
            Mouse::WheelDown
        };
        // A program that hears the mouse gets one wheel report a turn, however far it went, as in xterm.
        if self.report_mouse(wheel, e.position, e.modifiers, metrics) {
            cx.stop_propagation();
            return;
        }
        let Some(shell) = self.active_shell_mut() else {
            return;
        };
        let screen = shell.parser.screen();
        if screen.alternate_screen() {
            // Full-screen programs scroll with the arrow keys.
            let key = match (whole > 0., screen.application_cursor()) {
                (true, true) => "\x1bOA",
                (true, false) => "\x1b[A",
                (false, true) => "\x1bOB",
                (false, false) => "\x1b[B",
            };
            let _ = shell.input.unbounded_send(key.repeat(whole.abs() as usize));
        } else {
            let back = shell.parser.screen().scrollback() as f32 + whole;
            shell
                .parser
                .screen_mut()
                .set_scrollback(back.max(0.) as usize);
            shell.selection = None;
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The cell under `position`, as (row, column).
    fn cell_under(&self, position: Point<Pixels>, metrics: &Metrics) -> (u16, u16) {
        let (cols, rows) = self.grid.unwrap_or((1, 1));
        let x = f32::from(position.x - self.origin.x) / metrics.cell_w;
        let y = f32::from(position.y - self.origin.y) / metrics.line_h;
        (
            (y.max(0.) as u16).min(rows.saturating_sub(1)),
            (x.max(0.) as u16).min(cols.saturating_sub(1)),
        )
    }

    /// The edge between cells nearest `position`, as (row, column), where a selection starts or ends.
    fn cell_at(&self, position: Point<Pixels>, metrics: &Metrics) -> (u16, u16) {
        let (cols, rows) = self.grid.unwrap_or((1, 1));
        let x = f32::from(position.x - self.origin.x) / metrics.cell_w;
        let y = f32::from(position.y - self.origin.y) / metrics.line_h;
        (
            (y.max(0.) as u16).min(rows.saturating_sub(1)),
            (x.max(0.).round() as u16).min(cols),
        )
    }

    /// The active shell's screen, sized to the panel, or nothing while none is open.
    pub(super) fn terminal_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *theme(cx);
        let metrics = Metrics::new(&self.settings, window);
        let focused = self.term_focus.is_focused(window);
        let measure = {
            let this = cx.entity().downgrade();
            let metrics = metrics.clone();
            canvas(
                move |bounds, _, cx| {
                    let _ = this.update(cx, |this, cx| this.fit(bounds, &metrics, cx));
                },
                |_, (), _, _| {},
            )
            .absolute()
            .size_full()
        };
        // The cursor blinks only while the terminal has the keys, as xterm's does.
        let blinking = self.settings.cursor_blinking && focused;
        let elapsed = self.blink_from.elapsed();
        let shown = !blinking || (elapsed.as_millis() / BLINK.as_millis()).is_multiple_of(2);
        if blinking && self.blink_timer.is_none() {
            let wait = BLINK.saturating_sub(Duration::from_millis(
                (elapsed.as_millis() % BLINK.as_millis()) as u64,
            ));
            self.blink_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    this.blink_timer = None;
                    cx.notify();
                });
            }));
        }
        let screen = self.active_shell().map(|shell| {
            let screen = shell.parser.screen();
            draw_screen(
                screen,
                shell.selection,
                &metrics,
                &self.settings,
                focused,
                shown,
                &t,
            )
        });
        div()
            .id("terminal")
            .role(Role::Group)
            .aria_label("Terminal")
            .key_context("Terminal")
            .track_focus(&self.term_focus)
            .relative()
            .size_full()
            .min_w_0()
            .overflow_hidden()
            .cursor_text()
            .bg(t.bg_editor)
            .on_key_down(cx.listener(Self::on_terminal_key))
            .on_scroll(cx.listener({
                let metrics = metrics.clone();
                move |this, e: &ScrollEvent, _, cx| this.on_terminal_scroll(e, &metrics, cx)
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseDownEvent, window, cx| {
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(0), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(0);
                            return;
                        }
                        if e.modifiers.alt
                            && this.settings.alt_click_moves_cursor
                            && this.alt_click(e.position, &metrics)
                        {
                            cx.notify();
                            return;
                        }
                        let cell = this.cell_at(e.position, &metrics);
                        let under = this.cell_under(e.position, &metrics);
                        if let Some(shell) = this.active_shell_mut() {
                            // A double click takes the word, a triple click the line, as in xterm.
                            let screen = shell.parser.screen();
                            shell.selection = Some(match e.click_count {
                                0 | 1 => (cell, cell),
                                2 => word_at(screen, under),
                                _ => line_at(screen, under.0),
                            });
                        }
                        this.selecting = e.click_count < 2;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener({
                let metrics = metrics.clone();
                move |this, e: &MouseMoveEvent, _, cx| {
                    let event = this.mouse_held.map_or(Mouse::Move, Mouse::Drag);
                    if !this.selecting
                        && this.report_mouse(event, e.position, e.modifiers, &metrics)
                    {
                        return;
                    }
                    if !this.selecting || !e.dragging() {
                        return;
                    }
                    let cell = this.cell_at(e.position, &metrics);
                    if let Some(shell) = this.active_shell_mut()
                        && let Some((_, end)) = &mut shell.selection
                    {
                        *end = cell;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, cx| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(0), e.position, e.modifiers, &metrics);
                            return;
                        }
                        this.selecting = false;
                        let copy = this.settings.copy_on_selection;
                        if let Some(shell) = this.active_shell_mut()
                            && shell.selection.is_some_and(|(a, b)| a == b)
                        {
                            shell.selection = None;
                        }
                        if copy {
                            this.copy_selection(cx);
                        }
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseDownEvent, window, cx| {
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(1), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(1);
                        }
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, _| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(1), e.position, e.modifiers, &metrics);
                        }
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseUpEvent, _, _| {
                        if this.mouse_held.take().is_some() {
                            this.report_mouse(Mouse::Release(2), e.position, e.modifiers, &metrics);
                        }
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener({
                    let metrics = metrics.clone();
                    move |this, e: &MouseDownEvent, window, cx| {
                        use patchwork_server::settings::RightClickBehavior as Behavior;
                        window.focus(&this.term_focus, cx);
                        if this.report_mouse(Mouse::Press(2), e.position, e.modifiers, &metrics) {
                            this.mouse_held = Some(2);
                            return;
                        }
                        match this.settings.right_click_behavior {
                            Behavior::CopyPaste if this.copy_selection(cx) => {
                                if let Some(shell) = this.active_shell_mut() {
                                    shell.selection = None;
                                }
                            }
                            Behavior::CopyPaste | Behavior::Paste => this.paste(cx),
                            _ => {}
                        }
                        cx.notify();
                    }
                }),
            )
            .child(measure)
            .children(screen)
            .into_any_element()
    }
}

fn exit_text(name: &str, code: u32) -> String {
    format!(
        "\r\n\x1b[90mThe terminal process \"{name}\" terminated with exit code: {code}.\x1b[0m\r\n"
    )
}

fn selected_text(shell: &Shell) -> Option<String> {
    let (a, b) = ordered(shell.selection?);
    if a == b {
        return None;
    }
    let text = shell.parser.screen().contents_between(a.0, a.1, b.0, b.1);
    (!text.is_empty()).then_some(text)
}
