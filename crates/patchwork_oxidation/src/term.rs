//! The terminal reporter: one line for each rebuild, and a spinner on the last line while building.
//!
//! The spinner is redrawn in place on its own line, never clearing the whole screen, so nothing
//! flickers. The managed app's output passes through above it. Without a TTY there is no spinner
//! and no ANSI color.

// Rust guideline compliant 2026-10-09

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use crate::session::Reporter;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const FRAME_EVERY: Duration = Duration::from_millis(80);

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const CYAN: &str = "\x1b[36m";
const RESET: &str = "\x1b[0m";

/// Which of the app's streams a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    /// Standard output.
    Out,
    /// Standard error.
    Err,
}

/// What a status line reports, shown by its mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// The app was built and started.
    Built,
    /// A save changed nothing the app is built from.
    Same,
    /// The build failed.
    Failed,
    /// A saved file was sent to the app.
    Sent,
    /// The app quit by itself.
    Quit,
    /// Something about Oxidation itself.
    Note,
}

/// Writes Oxidation's status lines and the managed app's output to the terminal.
#[derive(Debug)]
pub struct Term {
    tty: bool,
    color: bool,
    live: Option<String>,
    frame: usize,
    drawn: Option<Instant>,
}

impl Default for Term {
    fn default() -> Self {
        Self::new()
    }
}

impl Term {
    /// Writes to standard error, using ANSI color when stderr is a terminal and `NO_COLOR` is unset.
    #[must_use]
    pub fn new() -> Self {
        let tty = std::io::stderr().is_terminal();
        Self {
            tty,
            color: tty && std::env::var_os("NO_COLOR").is_none(),
            live: None,
            frame: 0,
            drawn: None,
        }
    }

    /// Whether the terminal accepts ANSI color.
    #[must_use]
    pub fn color(&self) -> bool {
        self.color
    }

    /// Shows `text` beside the spinner.
    pub fn spin(&mut self, text: String) {
        if self.live.as_ref() != Some(&text) {
            self.live = Some(text);
            self.draw();
        }
    }

    /// Advances the spinner frame if active and due.
    pub fn tick(&mut self) {
        if self.live.is_some() && self.drawn.is_none_or(|at| at.elapsed() >= FRAME_EVERY) {
            self.frame = (self.frame + 1) % FRAMES.len();
            self.draw();
        }
    }

    /// Clears the active spinner line.
    pub fn settle(&mut self) {
        if self.live.take().is_some() {
            self.erase();
        }
    }

    /// Writes one timestamped status line.
    pub fn line(&mut self, mark: Mark, text: &str) {
        let (symbol, tint) = match mark {
            Mark::Built => ("✓", GREEN),
            Mark::Same => ("·", DIM),
            Mark::Failed => ("✗", RED),
            Mark::Sent => ("↻", CYAN),
            Mark::Quit => ("■", YELLOW),
            Mark::Note => ("◆", CYAN),
        };
        let time = chrono::Local::now().format("%H:%M:%S");
        let line = if self.color {
            format!("{DIM}{time}{RESET} {tint}{symbol}{RESET} {text}")
        } else {
            format!("{time} {symbol} {text}")
        };
        self.above(|err| writeln!(err, "{line}"));
    }

    /// Writes verbatim text, such as rendered compiler errors or CLI usage.
    pub fn plain(&mut self, text: &str) {
        self.above(|err| writeln!(err, "{}", text.trim_end()));
    }

    /// Clears the terminal and its scrollback, preserving the spinner.
    pub fn clear(&mut self) {
        if self.tty {
            self.above(|err| write!(err, "\x1b[H\x1b[2J\x1b[3J"));
        }
    }

    /// Writes a fatal error line before exiting.
    pub fn fail(&mut self, text: &str) {
        self.settle();
        self.line(Mark::Failed, text);
    }

    /// Forwards one line written by the managed app.
    pub fn output(&mut self, stream: Stream, line: &[u8]) {
        self.erase();
        let _ = match stream {
            Stream::Out => {
                let mut out = std::io::stdout().lock();
                out.write_all(line)
                    .and_then(|()| out.write_all(b"\n"))
                    .and_then(|()| out.flush())
            }
            Stream::Err => {
                let mut err = std::io::stderr().lock();
                err.write_all(line).and_then(|()| err.write_all(b"\n"))
            }
        };
        self.draw();
    }

    /// Formats `text` bold when color is enabled.
    #[must_use]
    pub fn bold(&self, text: &str) -> String {
        self.tint(BOLD, text)
    }

    /// Formats `text` dim when color is enabled.
    #[must_use]
    pub fn dim(&self, text: &str) -> String {
        self.tint(DIM, text)
    }

    /// Formats `text` yellow when color is enabled.
    #[must_use]
    pub fn warn(&self, text: &str) -> String {
        self.tint(YELLOW, text)
    }

    fn tint(&self, tint: &str, text: &str) -> String {
        if self.color {
            format!("{tint}{text}{RESET}")
        } else {
            text.to_owned()
        }
    }

    fn above(&mut self, write: impl FnOnce(&mut std::io::StderrLock<'_>) -> std::io::Result<()>) {
        self.erase();
        let _ = write(&mut std::io::stderr().lock());
        self.draw();
    }

    fn draw(&mut self) {
        let Some(text) = self.live.as_ref().filter(|_| self.tty) else {
            return;
        };
        let frame = FRAMES[self.frame];
        let line = if self.color {
            format!("\r\x1b[2K{CYAN}{frame}{RESET} {DIM}{text}{RESET}")
        } else {
            format!("\r\x1b[2K{frame} {text}")
        };
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(line.as_bytes()).and_then(|()| err.flush());
        self.drawn = Some(Instant::now());
    }

    fn erase(&mut self) {
        if self.tty && self.drawn.take().is_some() {
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(b"\r\x1b[2K").and_then(|()| err.flush());
        }
    }
}

impl Reporter for Term {
    fn color(&self) -> bool {
        Self::color(self)
    }

    fn spin(&mut self, text: String) {
        Self::spin(self, text);
    }

    fn tick(&mut self) {
        Self::tick(self);
    }

    fn settle(&mut self) {
        Self::settle(self);
    }

    fn line(&mut self, mark: Mark, text: &str) {
        Self::line(self, mark, text);
    }

    fn plain(&mut self, text: &str) {
        Self::plain(self, text);
    }

    fn clear(&mut self) {
        Self::clear(self);
    }

    fn output(&mut self, stream: Stream, line: &[u8]) {
        Self::output(self, stream, line);
    }

    fn bold(&self, text: &str) -> String {
        Self::bold(self, text)
    }

    fn dim(&self, text: &str) -> String {
        Self::dim(self, text)
    }

    fn warn(&self, text: &str) -> String {
        Self::warn(self, text)
    }
}
