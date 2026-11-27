//! One `cargo build`: starting it, reading what cargo reports, and its outcome.
//!
//! Cargo's JSON messages on stdout carry the errors, the warnings, and the built binary; its
//! progress lines on stderr name the crate being compiled, which the reporter displays.

// Rust guideline compliant 2026-10-09

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::Context;
use serde_json::Value;

use crate::Event;
use crate::config;
use crate::signals;

/// Something cargo reported.
#[derive(Debug)]
pub enum Note {
    /// It started compiling this crate.
    Compiling(String),
    /// It is waiting for another cargo to let go of the build folder.
    Waiting,
    /// A compile error, rendered.
    Error(String),
    /// A compile warning.
    Warning,
    /// It built the binary.
    Binary {
        /// Where the binary is.
        path: PathBuf,
        /// Whether it was already up to date.
        fresh: bool,
    },
    /// A line of its own output, kept to show if the build fails without a compile error.
    Said(String),
    /// It has finished and closed its output.
    Ended,
}

/// How a build went.
#[derive(Debug)]
pub struct Outcome {
    /// Whether it succeeded.
    pub ok: bool,
    /// The binary, and whether it was already up to date.
    pub binary: Option<(PathBuf, bool)>,
    /// The compile errors, rendered.
    pub errors: Vec<String>,
    /// How many warnings there were.
    pub warnings: usize,
    /// Cargo's last lines, for a failure without a compile error.
    pub said: Vec<String>,
    /// How long it took.
    pub took: Duration,
    /// The saves that started it.
    pub changed: Vec<PathBuf>,
}

/// A running `cargo build`.
#[derive(Debug)]
pub struct Build {
    /// Which build this is; notes from older ones are dropped.
    pub id: u64,
    child: Child,
    started: Instant,
    /// The crate cargo is compiling.
    pub compiling: Option<String>,
    /// Whether cargo is waiting for another one's lock.
    pub waiting: bool,
    outcome: Outcome,
}

impl Build {
    /// Starts `cargo build` in `root`, reporting to `send` under `id`.
    ///
    /// # Errors
    ///
    /// Returns an error if cargo cannot be spawned.
    pub fn start(
        id: u64,
        root: &Path,
        build: &config::Build,
        color: bool,
        changed: Vec<PathBuf>,
        send: &Sender<Event>,
    ) -> anyhow::Result<Self> {
        let mut command = match build.effective_toolchain() {
            // Rustup's `cargo` runs the toolchain the variable names.
            Some(toolchain) => {
                let mut command = Command::new("cargo");
                command.env("RUSTUP_TOOLCHAIN", toolchain);
                command
            }
            None => Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
        };
        command.arg("build").arg(if color {
            "--message-format=json-diagnostic-rendered-ansi"
        } else {
            "--message-format=json"
        });
        if let Some(package) = &build.package {
            command.args(["--package", package]);
        }
        if let Some(bin) = &build.bin {
            command.args(["--bin", bin]);
        }
        if let Some(dir) = &build.target_dir {
            command.arg("--target-dir").arg(root.join(dir));
        }
        command
            .args(build.effective_args())
            .current_dir(root)
            .env("CARGO_TERM_COLOR", "never")
            .env("CARGO_TERM_PROGRESS_WHEN", "never")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = signals::own_group(&mut command)
            .spawn()
            .context("could not start cargo")?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            anyhow::bail!("cargo's output was not piped");
        };
        let bin = build.bin.clone();
        let (to_err, to_out) = (send.clone(), send.clone());
        std::thread::spawn(move || {
            let progress = std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Some(note) = progress_note(&line) {
                        let _ = to_err.send(Event::Build(id, note));
                    }
                }
            });
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(note) = message_note(&line, bin.as_deref()) {
                    let _ = to_out.send(Event::Build(id, note));
                }
            }
            let _ = progress.join();
            let _ = to_out.send(Event::Build(id, Note::Ended));
        });
        Ok(Self {
            id,
            child,
            started: Instant::now(),
            compiling: None,
            waiting: false,
            outcome: Outcome {
                ok: false,
                binary: None,
                errors: Vec::new(),
                warnings: 0,
                said: Vec::new(),
                took: Duration::ZERO,
                changed,
            },
        })
    }

    /// Takes in a note from cargo, other than [`Note::Ended`].
    pub fn note(&mut self, note: Note) {
        match note {
            Note::Compiling(name) => {
                self.waiting = false;
                self.compiling = Some(name);
            }
            Note::Waiting => self.waiting = true,
            Note::Error(text) => self.outcome.errors.push(text),
            Note::Warning => self.outcome.warnings += 1,
            Note::Binary { path, fresh } => self.outcome.binary = Some((path, fresh)),
            Note::Said(line) => {
                if self.outcome.said.len() == 20 {
                    self.outcome.said.remove(0);
                }
                self.outcome.said.push(line);
            }
            Note::Ended => {}
        }
    }

    /// The saves that started this build.
    #[must_use]
    pub fn changed(&self) -> &[PathBuf] {
        &self.outcome.changed
    }

    /// Waits for cargo to exit once it has closed its output streams.
    #[must_use]
    pub fn finish(mut self) -> Outcome {
        let status = self.child.wait();
        self.outcome.ok = status.is_ok_and(|s| s.success());
        self.outcome.took = self.started.elapsed();
        self.outcome
    }

    /// Stops cargo and its child compilers, returning the saves that triggered it.
    #[must_use]
    pub fn cancel(mut self) -> Vec<PathBuf> {
        signals::stop(&mut self.child, Duration::from_millis(500));
        self.outcome.changed
    }
}

fn progress_note(line: &str) -> Option<Note> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("Compiling ") {
        let name = rest.split_whitespace().next()?;
        return Some(Note::Compiling(name.to_owned()));
    }
    if trimmed.starts_with("Blocking waiting for file lock") {
        return Some(Note::Waiting);
    }
    let routine = [
        "Finished ",
        "Fresh ",
        "Checking ",
        "Running ",
        "Updating ",
        "Locking ",
    ];
    if routine.iter().any(|r| trimmed.starts_with(r)) || trimmed.is_empty() {
        return None;
    }
    Some(Note::Said(line.to_owned()))
}

fn message_note(line: &str, bin: Option<&str>) -> Option<Note> {
    let message: Value = serde_json::from_str(line).ok()?;
    match message["reason"].as_str()? {
        "compiler-message" => {
            let diagnostic = &message["message"];
            match diagnostic["level"].as_str()? {
                "error" | "error: internal compiler error" => {
                    let text = diagnostic["message"].as_str().unwrap_or_default();
                    if text.starts_with("aborting due to") {
                        return None;
                    }
                    Some(Note::Error(diagnostic["rendered"].as_str()?.to_owned()))
                }
                "warning" => Some(Note::Warning),
                _ => None,
            }
        }
        "compiler-artifact" => {
            let target = &message["target"];
            let is_bin = target["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "bin"));
            let named = bin.is_none_or(|bin| target["name"] == bin);
            let path = message["executable"].as_str()?;
            (is_bin && named).then(|| Note::Binary {
                path: PathBuf::from(path),
                fresh: message["fresh"].as_bool().unwrap_or(false),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress() {
        let note = progress_note("   Compiling patchwork-ui v26.11.162 (/repo/crates/ui)");
        assert!(matches!(note, Some(Note::Compiling(name)) if name == "patchwork-ui"));
        assert!(matches!(
            progress_note("    Blocking waiting for file lock on build directory"),
            Some(Note::Waiting)
        ));
        assert!(progress_note("    Finished `dev` profile in 2.1s").is_none());
        assert!(matches!(
            progress_note("error: could not compile `x`"),
            Some(Note::Said(_))
        ));
    }

    #[test]
    fn reads_errors_but_not_the_closing_count() {
        let error = r#"{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","rendered":"error[E0308]: mismatched types"}}"#;
        assert!(
            matches!(message_note(error, None), Some(Note::Error(text)) if text.starts_with("error[E0308]"))
        );
        let closing = r#"{"reason":"compiler-message","message":{"level":"error","message":"aborting due to 1 previous error","rendered":"x"}}"#;
        assert!(message_note(closing, None).is_none());
    }

    #[test]
    fn finds_the_named_binary() {
        let artifact = r#"{"reason":"compiler-artifact","target":{"kind":["bin"],"name":"app"},"executable":"/t/app","fresh":true}"#;
        assert!(matches!(
            message_note(artifact, Some("app")),
            Some(Note::Binary { fresh: true, .. })
        ));
        assert!(message_note(artifact, Some("other")).is_none());
        let library = r#"{"reason":"compiler-artifact","target":{"kind":["lib"],"name":"x"},"executable":null,"fresh":false}"#;
        assert!(message_note(library, None).is_none());
    }
}
