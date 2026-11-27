//! The hot-reload session loop: saves in, builds out, and the app swapped on each good build.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::Event;
use crate::app::App;
use crate::build::{Build, Note, Outcome};
use crate::config::Config;
use crate::keys::Key;
use crate::signals;
use crate::term::{Mark, Stream, Term};
use crate::watch::Kind;

/// How often the loop wakes while idle to advance the spinner and check child status.
const WAKE_EVERY: Duration = Duration::from_millis(80);

/// State file stored inside the target directory when handoff is enabled.
const STATE_FILE: &str = "oxidation-state.json";

/// Sink for session status messages, compiler diagnostics, and managed app output.
///
/// Implement this trait to embed Oxidation inside an IDE, custom CLI, or test harness.
pub trait Reporter {
    /// Whether ANSI color sequences are supported (also forwarded to Cargo diagnostics).
    fn color(&self) -> bool {
        false
    }

    /// Displays an active progress message (for example beside a spinner).
    fn spin(&mut self, text: String);

    /// Called periodically while idle to animate progress indicators.
    fn tick(&mut self) {}

    /// Clears any active progress indicator before writing a permanent line.
    fn settle(&mut self) {}

    /// Writes one status line tagged with `mark`.
    fn line(&mut self, mark: Mark, text: &str);

    /// Writes verbatim multi-line text such as rendered compiler errors.
    fn plain(&mut self, text: &str);

    /// Clears the output surface if interactive.
    fn clear(&mut self) {}

    /// Forwards one line of the managed app's stdout or stderr.
    fn output(&mut self, stream: Stream, line: &[u8]);

    /// Styles emphasized text when color is enabled.
    fn bold(&self, text: &str) -> String {
        text.to_owned()
    }

    /// Styles secondary text when color is enabled.
    fn dim(&self, text: &str) -> String {
        text.to_owned()
    }

    /// Styles warning text when color is enabled.
    fn warn(&self, text: &str) -> String {
        text.to_owned()
    }
}

/// No-op [`Reporter`] suitable for headless runs and unit tests.
#[derive(Debug, Default)]
pub struct NullReporter;

impl Reporter for NullReporter {
    fn spin(&mut self, _text: String) {}

    fn line(&mut self, _mark: Mark, _text: &str) {}

    fn plain(&mut self, _text: &str) {}

    fn output(&mut self, _stream: Stream, _line: &[u8]) {}
}

/// State machine driving builds, reloads, and process swaps for one hot-reload session.
#[derive(Debug)]
pub struct Session<'a, R: Reporter = Term> {
    reporter: &'a mut R,
    config: Config,
    root: PathBuf,
    app_args: Vec<String>,
    send: Sender<Event>,
    /// Whether interactive terminal keys are active.
    keys: bool,
    builds: u64,
    build: Option<Build>,
    /// Whether the active build restarts the app even when its binary is fresh.
    forced: bool,
    app: Option<App>,
    /// Most recently built binary path, reused for asset-triggered restarts.
    binary: Option<PathBuf>,
    /// Pending file saves waiting for the debounce window to settle.
    saved: Vec<(PathBuf, Kind)>,
    last_save: Option<Instant>,
}

impl<'a, R: Reporter> Session<'a, R> {
    /// Creates a session over `root`, launching the app with `app_args` appended to `config.run.args`.
    pub fn new(
        reporter: &'a mut R,
        config: Config,
        root: PathBuf,
        app_args: Vec<String>,
        send: Sender<Event>,
        keys: bool,
    ) -> Self {
        Self {
            reporter,
            config,
            root,
            app_args,
            send,
            keys,
            builds: 0,
            build: None,
            forced: false,
            app: None,
            binary: None,
            saved: Vec::new(),
            last_save: None,
        }
    }

    /// Builds and starts the app, then processes file changes and key commands until stopped.
    pub fn run(mut self, events: &Receiver<Event>) {
        self.intro();
        self.start_build(Vec::new());
        while !signals::stopped() {
            match events.recv_timeout(self.next_wake()) {
                Ok(Event::Changed(path, kind)) => {
                    if !self.saved.iter().any(|(p, _)| *p == path) {
                        self.saved.push((path, kind));
                    }
                    self.last_save = Some(Instant::now());
                }
                Ok(Event::Key(Key::Quit)) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Event::Key(Key::Clear)) => self.reporter.clear(),
                Ok(Event::Key(Key::Rebuild)) => {
                    self.forced = true;
                    self.rebuild(Vec::new());
                }
                Ok(Event::Build(id, note)) => self.on_build(id, note),
                Ok(Event::Output(stream, line)) => self.reporter.output(stream, &line),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self
                .last_save
                .is_some_and(|at| at.elapsed() >= self.settle())
            {
                self.last_save = None;
                self.act();
            }
            self.check_app();
            self.reporter.tick();
        }
        self.reporter.settle();
        if let Some(build) = self.build.take() {
            let _ = build.cancel();
        }
        if let Some(app) = self.app.take() {
            app.stop(self.stop_within());
        }
    }

    fn intro(&mut self) {
        let name = self
            .config
            .build
            .package
            .clone()
            .unwrap_or_else(|| "the app".into());
        let watched = self
            .config
            .watch
            .paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let keys = if self.keys {
            " · r rebuild · c clear · q quit"
        } else {
            ""
        };
        let intro = format!(
            "{} {}",
            self.reporter.bold(&name),
            self.reporter.dim(&format!("· watching {watched}{keys}"))
        );
        self.reporter.line(Mark::Note, &intro);
    }

    fn settle(&self) -> Duration {
        Duration::from_millis(self.config.watch.settle_ms)
    }

    fn stop_within(&self) -> Duration {
        Duration::from_millis(self.config.run.stop_within_ms)
    }

    fn state_file(&self) -> PathBuf {
        let target = self
            .config
            .build
            .target_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("target"));
        self.root.join(target).join(STATE_FILE)
    }

    fn next_wake(&self) -> Duration {
        self.last_save
            .map_or(WAKE_EVERY, |at| self.settle().saturating_sub(at.elapsed()))
            .min(WAKE_EVERY)
    }

    fn act(&mut self) {
        let mut sent = Vec::new();
        let mut restart = Vec::new();
        let mut build = Vec::new();
        for (path, kind) in std::mem::take(&mut self.saved) {
            if kind != Kind::Build && !path.exists() {
                continue;
            }
            match kind {
                Kind::Send if self.config.run.handoff => sent.push(path),
                Kind::Send | Kind::Restart => restart.push(path),
                Kind::Build => build.push(path),
            }
        }
        if !sent.is_empty() {
            self.send_files(&sent);
        }
        if build.is_empty() && self.build.is_none() {
            if !restart.is_empty() {
                self.restart(&restart);
            }
            return;
        }
        self.forced |= !restart.is_empty();
        build.extend(restart);
        self.rebuild(build);
    }

    fn rebuild(&mut self, saved: Vec<PathBuf>) {
        let mut changed = self.build.take().map(Build::cancel).unwrap_or_default();
        for path in saved {
            if !changed.contains(&path) {
                changed.push(path);
            }
        }
        self.start_build(changed);
    }

    fn send_files(&mut self, paths: &[PathBuf]) {
        let lead = describe(&self.root, paths).unwrap_or_default();
        let told = match self.app.as_mut() {
            Some(app) => paths.iter().all(|path| app.reload(path)),
            None => false,
        };
        if told {
            let line = format!("{} · sent to the app", self.reporter.bold(&lead));
            self.reporter.line(Mark::Sent, &line);
        } else {
            let line = self
                .reporter
                .dim(&format!("{lead} · read when the app starts"));
            self.reporter.line(Mark::Same, &line);
        }
    }

    fn restart(&mut self, paths: &[PathBuf]) {
        let lead = describe(&self.root, paths).unwrap_or_default();
        let Some(binary) = self.binary.clone() else {
            let line = self.reporter.dim(&format!("{lead} · no build to run yet"));
            self.reporter.line(Mark::Same, &line);
            return;
        };
        let restarting = self.app.is_some();
        match self.launch(&binary) {
            Ok(stop) => {
                let done = if restarting {
                    format!("restarted in {}ms", stop.as_millis())
                } else {
                    "started".to_owned()
                };
                let line = format!("{} · {done}", self.reporter.bold(&lead));
                self.reporter.line(Mark::Built, &line);
            }
            Err(e) => self.reporter.line(Mark::Failed, &format!("{e:#}")),
        }
    }

    fn launch(&mut self, binary: &Path) -> anyhow::Result<Duration> {
        let stopped = Instant::now();
        let held = self
            .app
            .take()
            .map(|app| app.restart(self.stop_within()))
            .unwrap_or_default();
        let stop = stopped.elapsed();
        let state = self.state_file();
        self.app = Some(App::start(
            binary,
            &self.root,
            &self.config.run,
            &self.app_args,
            &state,
            held,
            &self.send,
        )?);
        Ok(stop)
    }

    fn start_build(&mut self, changed: Vec<PathBuf>) {
        self.builds += 1;
        let color = self.reporter.color();
        match Build::start(
            self.builds,
            &self.root,
            &self.config.build,
            color,
            changed,
            &self.send,
        ) {
            Ok(build) => {
                self.build = Some(build);
                self.reporter.spin("building".into());
            }
            Err(e) => self.reporter.line(Mark::Failed, &format!("{e:#}")),
        }
    }

    fn on_build(&mut self, id: u64, note: Note) {
        let Some(build) = self.build.as_mut().filter(|b| b.id == id) else {
            return;
        };
        if matches!(note, Note::Ended) {
            if let Some(build) = self.build.take() {
                self.finish(build.finish());
            }
            return;
        }
        build.note(note);
        let status = if build.waiting {
            "waiting for another cargo to finish".to_owned()
        } else {
            match &build.compiling {
                Some(name) => format!("building {name}"),
                None => "building".to_owned(),
            }
        };
        let status = match describe(&self.root, build.changed()) {
            Some(what) => format!("{what} · {status}"),
            None => status,
        };
        self.reporter.spin(status);
    }

    fn finish(&mut self, outcome: Outcome) {
        self.reporter.settle();
        let forced = std::mem::take(&mut self.forced);
        let what = describe(&self.root, &outcome.changed);
        let took = format!("{:.1}s", outcome.took.as_secs_f64());
        let warnings = match outcome.warnings {
            0 => String::new(),
            1 => format!(" · {}", self.reporter.warn("1 warning")),
            n => format!(" · {}", self.reporter.warn(&format!("{n} warnings"))),
        };
        let lead = what.map_or_else(String::new, |w| format!("{} · ", self.reporter.bold(&w)));
        if !outcome.ok {
            let shown = if outcome.errors.is_empty() {
                outcome.said.join("\n")
            } else {
                outcome.errors.join("\n")
            };
            self.reporter.plain(&shown);
            if let Some(app) = self.app.as_mut() {
                app.report(&shown);
            }
            let count = match outcome.errors.len() {
                0 => "build failed".to_owned(),
                1 => "1 error".to_owned(),
                n => format!("{n} errors"),
            };
            self.reporter
                .line(Mark::Failed, &format!("{lead}{count} · {took}"));
            return;
        }
        let Some((binary, fresh)) = outcome.binary else {
            self.reporter
                .line(Mark::Failed, &format!("{lead}built, but no binary to run"));
            return;
        };
        self.binary = Some(binary.clone());
        if fresh
            && !forced
            && let Some(app) = self.app.as_mut()
        {
            app.fixed();
            self.reporter
                .line(Mark::Same, &self.reporter.dim(&format!("{lead}no change")));
            return;
        }
        let restarting = self.app.is_some();
        match self.launch(&binary) {
            Ok(stop) => {
                let done = if restarting {
                    format!("restarted in {}ms", stop.as_millis())
                } else {
                    "started".to_owned()
                };
                let line = format!("{lead}built in {took} · {done}{warnings}");
                self.reporter.line(Mark::Built, &line);
            }
            Err(e) => self.reporter.line(Mark::Failed, &format!("{e:#}")),
        }
    }

    fn check_app(&mut self) {
        let Some(status) = self.app.as_mut().and_then(App::quit) else {
            return;
        };
        self.app = None;
        let how = match status.code() {
            Some(0) => "the app quit".to_owned(),
            Some(code) => format!("the app quit with code {code}"),
            None => "the app was killed".to_owned(),
        };
        let line = format!(
            "{how} {}",
            self.reporter.dim("· save or press r to start it again")
        );
        self.reporter.line(Mark::Quit, &line);
    }
}

/// Summarizes changed files as the first file's folder/name plus a count of remaining files.
fn describe(root: &Path, changed: &[PathBuf]) -> Option<String> {
    let first = changed.first()?;
    let relative = first.strip_prefix(root).unwrap_or(first);
    let mut parts: Vec<_> = relative.components().rev().take(2).collect();
    parts.reverse();
    let short: PathBuf = parts.iter().collect();
    let more = changed.len() - 1;
    Some(if more == 0 {
        short.display().to_string()
    } else {
        format!("{} +{more}", short.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_saves_by_folder_and_name() {
        let root = Path::new("/repo");
        let one = [PathBuf::from("/repo/crates/ui/src/lib.rs")];
        assert_eq!(describe(root, &one).as_deref(), Some("src/lib.rs"));
        let two = [one[0].clone(), PathBuf::from("/repo/Cargo.toml")];
        assert_eq!(describe(root, &two).as_deref(), Some("src/lib.rs +1"));
        assert_eq!(describe(root, &[]), None);
    }
}
