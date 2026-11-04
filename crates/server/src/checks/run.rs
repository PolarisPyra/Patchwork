//! Running a project's check command: through the shell an agent's commands run in, in the
//! project's folder, with a time limit, keeping the end of what it printed.

use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::KEEP;

/// What a check command came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// It exited zero.
    pub passed: bool,
    /// It was still running at the limit and was stopped, so whether it would have passed is
    /// unknown. Never a pass.
    pub timed_out: bool,
    /// Its output, the end of it, with no colour codes.
    pub output: String,
    pub duration: Duration,
}

/// Runs `command` in `root`, stopping it after `limit`. Output is kept to its last [`KEEP`]
/// bytes; anything after that is read and dropped, so a chatty command still finishes.
pub fn run(root: &str, command: &str, limit: Duration) -> Result<Outcome, String> {
    let mut cmd = crate::system::shell_command(command)
        .ok_or_else(|| "No shell was found to run the check in.".to_string())?;
    cmd.current_dir(Path::new(root))
        .env("CI", "1")
        // Colour codes make the output unreadable in the card, and hide what the parsers look
        // for; every runner on this list respects one of these.
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("CLICOLOR", "0");
    let started = Instant::now();
    let (mut child, range) = crate::system::contained::spawn(cmd, Stdio::null())
        .map_err(|e| format!("The check command didn't start: {e}"))?;
    let stdout = keep_tail(child.stdout.take(), KEEP);
    let stderr = keep_tail(child.stderr.take(), KEEP);
    let deadline = started + limit;
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Whatever it left running, such as a server a test started and never stopped,
                // still holds the output pipes open, and reading them would wait on it forever.
                range.stop();
                break (Some(status), false);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                // All of it, so a runner that started a server or a watcher takes it along.
                range.stop();
                let _ = child.wait();
                break (None, true);
            }
            Err(e) => return Err(format!("The check command could not be waited for: {e}")),
        }
    };
    let duration = started.elapsed();
    let mut output = stdout.join().unwrap_or_default();
    let errors = stderr.join().unwrap_or_default();
    if !errors.trim().is_empty() {
        // Test runners split themselves across the two: cargo prints failures to stdout, most
        // JavaScript and Python runners to stderr.
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&errors);
    }
    Ok(Outcome {
        passed: status.is_some_and(|s| s.success()),
        timed_out,
        output,
        duration,
    })
}

/// Reads a pipe to its end, keeping only its last `keep` bytes. On its own thread, so a command
/// that fills both pipes cannot hold up the wait.
fn keep_tail<R: Read + Send + 'static>(
    pipe: Option<R>,
    keep: usize,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return String::new();
        };
        let mut kept: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 16 << 10];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    kept.extend_from_slice(&chunk[..n]);
                    if kept.len() > keep {
                        let cut = kept.len() - keep;
                        kept.drain(..cut);
                    }
                }
            }
        }
        // A cut in the middle of a character is replaced rather than dropped, since the text is
        // shown to a person.
        String::from_utf8_lossy(&kept).into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(crate::system::new_id());
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_command_that_exits_zero_passed() {
        let root = dir();
        let outcome = run(
            &root.to_string_lossy(),
            "echo hello",
            Duration::from_secs(30),
        )
        .expect("the shell runs");
        assert!(outcome.passed);
        assert!(!outcome.timed_out);
        assert!(outcome.output.contains("hello"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_failing_command_keeps_what_it_printed() {
        let root = dir();
        let outcome = run(
            &root.to_string_lossy(),
            "echo 'AssertionError: nope' >&2; exit 1",
            Duration::from_secs(30),
        )
        .expect("the shell runs");
        assert!(!outcome.passed);
        assert!(outcome.output.contains("AssertionError"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_command_past_its_limit_is_stopped_and_is_not_a_pass() {
        let root = dir();
        let outcome = run(
            &root.to_string_lossy(),
            "sleep 30",
            Duration::from_millis(300),
        )
        .expect("the shell runs");
        assert!(outcome.timed_out);
        assert!(!outcome.passed);
        assert!(outcome.duration < Duration::from_secs(10));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_output_keeps_its_end_and_not_its_beginning() {
        let root = dir();
        let outcome = run(
            &root.to_string_lossy(),
            "for i in $(seq 1 4000); do echo \"line $i padded padded padded padded\"; done",
            Duration::from_secs(60),
        )
        .expect("the shell runs");
        assert!(
            outcome.output.len() <= KEEP + 1,
            "kept {} bytes",
            outcome.output.len()
        );
        assert!(outcome.output.contains("line 4000"));
        assert!(!outcome.output.contains("line 1 padded"));
        std::fs::remove_dir_all(root).ok();
    }
}
