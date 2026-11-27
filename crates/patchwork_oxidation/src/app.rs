//! The managed application process: launching, forwarding output, sending handoff commands,
//! and stopping or restarting with file descriptor handover.

// Rust guideline compliant 2026-10-09

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command as ProcessCommand, ExitStatus, Stdio};
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::Context;
use patchwork_oxidation_client::fds::HeldFds;
use patchwork_oxidation_client::protocol::{Command, STATE_VAR, encode_command};

use crate::Event;
use crate::config::Run;
use crate::signals;
use crate::term::Stream;

/// A running child application managed by Oxidation.
#[derive(Debug)]
pub struct App {
    child: Child,
    /// Stdin command pipe when `run.handoff` is enabled.
    commands: Option<ChildStdin>,
    /// Host end of the Unix socket used for descriptor handover when `run.handoff` is enabled.
    #[cfg(unix)]
    channel: Option<patchwork_oxidation_client::fds::FdChannel>,
    /// Channel for reporting Oxidation's own operational warnings.
    send: Sender<Event>,
}

impl App {
    /// Starts `binary` in `root` with the arguments and environment from `run`, then `args`.
    ///
    /// With `run.handoff` enabled, passes `OXIDATION_STATE` pointing at `state` and delivers
    /// `held` descriptors from the prior run over `OXIDATION_FDS`.
    ///
    /// # Errors
    ///
    /// Returns an error if the child binary cannot be spawned.
    pub fn start(
        binary: &Path,
        root: &Path,
        run: &Run,
        args: &[String],
        state: &Path,
        held: HeldFds,
        send: &Sender<Event>,
    ) -> anyhow::Result<Self> {
        let mut command = ProcessCommand::new(binary);
        command
            .args(&run.args)
            .args(args)
            .envs(&run.env)
            .current_dir(root)
            .stdin(if run.handoff {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if run.handoff {
            command.env(STATE_VAR, state);
        }
        #[cfg(unix)]
        let channel = if run.handoff {
            patchwork_oxidation_client::fds::pair(&mut command)
                .map_err(|e| warn(send, &format!("could not make a socket for the app: {e}")))
                .ok()
        } else {
            None
        };
        let mut child = signals::own_group(&mut command)
            .spawn()
            .with_context(|| format!("could not start {}", binary.display()))?;
        #[cfg(unix)]
        let channel = channel.map(|(channel, theirs)| {
            drop(theirs);
            if let Err(e) = channel.give(&held) {
                warn(
                    send,
                    &format!("could not pass the held descriptors on: {e}"),
                );
            }
            channel
        });
        // The child now owns its duplicate descriptors; close the host's copies.
        drop(held);
        if let Some(out) = child.stdout.take() {
            pass_through(out, Stream::Out, send.clone());
        }
        if let Some(err) = child.stderr.take() {
            pass_through(err, Stream::Err, send.clone());
        }
        let commands = child.stdin.take();
        Ok(Self {
            child,
            commands,
            #[cfg(unix)]
            channel,
            send: send.clone(),
        })
    }

    /// Tells the app `path` was saved if handoff is active. Returns whether the command was sent.
    pub fn reload(&mut self, path: &Path) -> bool {
        self.send_command(&Command::Reload(path.to_path_buf()))
    }

    /// Sends plain-text compiler errors from the last failed build to the running app.
    pub fn report(&mut self, errors: &str) {
        self.send_command(&Command::Errors(errors.to_owned()));
    }

    /// Tells the running app that a subsequent build succeeded and cleared prior errors.
    pub fn fixed(&mut self) {
        self.send_command(&Command::Fixed);
    }

    /// Writes one [`Command`] frame to the child's stdin if handoff is active.
    pub fn send_command(&mut self, command: &Command) -> bool {
        let Some(commands) = self.commands.as_mut() else {
            return false;
        };
        let line = encode_command(command);
        commands
            .write_all(line.as_bytes())
            .and_then(|()| commands.flush())
            .is_ok()
    }

    /// Returns the exit status if the child has already exited.
    pub fn quit(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// Requests a graceful quit via `Command::Quit` (when handoff is active), then terminates
    /// the process group if it does not exit within `within`.
    pub fn stop(mut self, within: Duration) {
        if self.send_command(&Command::Quit) && signals::wait(&mut self.child, within) {
            signals::end_group(&self.child);
            return;
        }
        signals::stop(&mut self.child, within);
    }

    /// Requests a restart via `Command::Restart`, collects any handed-over file descriptors on
    /// Unix, and terminates the process group if it does not exit within `within`.
    #[must_use]
    pub fn restart(mut self, within: Duration) -> HeldFds {
        if self.send_command(&Command::Restart) && signals::wait(&mut self.child, within) {
            signals::end_group(&self.child);
        } else {
            signals::stop(&mut self.child, within);
        }
        #[cfg(unix)]
        if let Some(channel) = &self.channel {
            return channel.take().unwrap_or_else(|e| {
                warn(
                    &self.send,
                    &format!("could not take what the app handed over: {e}"),
                );
                HeldFds::default()
            });
        }
        HeldFds::default()
    }
}

fn warn(send: &Sender<Event>, problem: &str) {
    let line = format!("oxidation: {problem}").into_bytes();
    let _ = send.send(Event::Output(Stream::Err, line));
}

fn pass_through(stream: impl Read + Send + 'static, kind: Stream, send: Sender<Event>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if send
                .send(Event::Output(kind, std::mem::take(&mut line)))
                .is_err()
            {
                return;
            }
        }
    });
}
