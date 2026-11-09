//! The environment the user's own shell sets up, read from that shell once, as Patchwork starts.
//!
//! An app launched from the desktop gets the session's environment, which lacks what shell
//! startup files add: nvm, pnpm, cargo, mise, pyenv, Homebrew, and any `PATH=` line in
//! `.zshrc`, `.bashrc` or `config.fish`. So the user's login shell is started the way a terminal
//! starts it, interactive and as a login shell, and asked for its whole environment between
//! markers, since startup files may print too. Commands, agents and terminals then run in this
//! environment (`shell_command`), so they find the same tools a terminal does without re-reading
//! the startup files. A change to them reaches Patchwork when it starts again.

// Rust guideline compliant 2026-10-09

use std::ffi::OsString;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Set on a process Patchwork starts with the environment already read, such as its own agent,
/// so it uses the one it was given instead of starting the user's shell again.
pub const ENV_READ: &str = "PATCHWORK_LOGIN_ENV";

/// How long the user's shell gets to start and print its environment.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Variables that describe the probe's own shell rather than the user's environment.
const NOT_CARRIED: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];

/// Set for the probe, so a startup file does not stop to ask about updates or start tmux.
const PROBE_ENV: [(&str, &str); 3] = [
    ("DISABLE_AUTO_UPDATE", "true"),
    ("ZSH_TMUX_AUTOSTART", "false"),
    ("ZSH_TMUX_AUTOSTARTED", "true"),
];

/// The shells read, in turn, when the user's own can't be.
const FALLBACK_SHELLS: [&str; 3] = ["/bin/zsh", "/bin/bash", "/bin/sh"];

/// Variables Patchwork sets for the programs it starts, which the user's shell does not change.
const LAUNCHER_OWNED: [&str; 1] = ["PATCHWORK_"];

static LOGIN_ENV: OnceLock<Arc<Vec<(String, String)>>> = OnceLock::new();

/// The user's shell environment variables, read once and cached for the session.
///
/// The variables the user's shell sets up, read on first use, which [`crate::Server::open`] makes
/// at launch, and kept until Patchwork quits; a caller that comes while the read runs waits for
/// it. Empty when the shell can't be read, on Windows, whose apps get the PATH from the registry,
/// and in a process started with them already.
pub fn login_env() -> Arc<Vec<(String, String)>> {
    Arc::clone(LOGIN_ENV.get_or_init(|| {
        if cfg!(windows) || std::env::var_os(ENV_READ).is_some() {
            return Arc::default();
        }
        Arc::new(read_login_env().unwrap_or_default())
    }))
}

/// `name` as the user's shell sets it, else as Patchwork has it. None if unset or empty.
#[must_use]
pub fn login_var(name: &str) -> Option<String> {
    login_env()
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var(name).ok())
        .filter(|value| !value.is_empty())
}

/// The PATH to find and run programs with: the user's shell's, else the one Patchwork has.
pub fn search_path() -> Option<OsString> {
    login_env()
        .iter()
        .find(|(key, value)| key == "PATH" && !value.is_empty())
        .map(|(_, value)| OsString::from(value))
        .or_else(crate::system::path_var)
}

/// The program `name` on [`search_path`].
#[must_use]
pub fn find_program(name: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    which::which_in(name, search_path(), cwd).ok()
}

/// Reads the environment the user's login shell sets up, else that of the first of
/// [`FALLBACK_SHELLS`] that can be read. Nushell and others that can't run the probe are read
/// through bash.
fn read_login_env() -> Option<Vec<(String, String)>> {
    let login = crate::system::login_shell().or_else(|| std::env::var("SHELL").ok());
    let mut tried: Vec<String> = Vec::new();
    for shell in login.into_iter().chain(FALLBACK_SHELLS.map(String::from)) {
        let name = crate::system::shell_name(&shell);
        let shell = if crate::system::is_posix(&shell) || name == "fish" {
            shell
        } else {
            let cwd = std::env::current_dir().unwrap_or_default();
            // Not `find_shell`, which reads the environment this is reading.
            match which::which_in("bash", crate::system::path_var(), cwd) {
                Ok(bash) => bash.to_string_lossy().into_owned(),
                Err(_) => continue,
            }
        };
        if tried.contains(&shell) || !std::path::Path::new(&shell).is_file() {
            continue;
        }
        if let Some(vars) = read_shell(&shell) {
            return Some(vars);
        }
        tried.push(shell);
    }
    None
}

/// Starts `shell` as a terminal would and reads its environment. Bash as a login shell skips
/// `.bashrc` unless its profile sources it, while a terminal's bash reads only `.bashrc`, so bash
/// is read both ways and the two joined. A shell whose startup files hand over to another program,
/// such as `exec tmux`, is read as a plain login shell instead.
fn read_shell(shell: &str) -> Option<Vec<(String, String)>> {
    let mut flags: Vec<&[&str]> = vec![&["-l", "-i", "-c"]];
    if crate::system::shell_name(shell) == "bash" {
        flags.push(&["-i", "-c"]);
    }
    let mut readings = flags.iter().filter_map(|flags| probe(shell, flags));
    let Some(mut vars) = readings.next().or_else(|| probe(shell, &["-l", "-c"])) else {
        log::warn!(shell:%; "could not read the environment {{shell}} sets up");
        return None;
    };
    for extra in readings {
        join(&mut vars, extra);
    }
    log::info!(count = vars.len(), shell:%; "read the {{count}} variables {{shell}} sets up");
    Some(vars)
}

/// Adds the variables `extra` has that `vars` lacks, and the folders of its PATH after those of
/// `vars`.
fn join(vars: &mut Vec<(String, String)>, extra: Vec<(String, String)>) {
    for (key, value) in extra {
        match vars.iter_mut().find(|(k, _)| *k == key) {
            Some((_, have)) if key == "PATH" => {
                let mut dirs: Vec<PathBuf> = std::env::split_paths(have.as_str()).collect();
                for dir in std::env::split_paths(&value) {
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
                if let Ok(joined) = std::env::join_paths(dirs) {
                    *have = joined.to_string_lossy().into_owned();
                }
            }
            Some(_) => {}
            None => vars.push((key, value)),
        }
    }
}

/// Runs `shell` with `flags` and a script that prints its environment between two markers.
/// None if it doesn't print both in time.
fn probe(shell: &str, flags: &[&str]) -> Option<Vec<(String, String)>> {
    let marker = format!(
        "__PATCHWORK_ENV_{}__",
        crate::system::new_id().replace('-', "")
    );
    // `env -0` keeps values with new lines whole; BSD `env` lacks it, so plain `env` follows.
    // Every shell the probe runs reads this the same, fish included.
    let script = format!(
        "printf '%s' {marker}; command env -0 2>/dev/null || command env; printf '%s' {marker}"
    );
    let mut cmd = crate::system::host_command(shell);
    // From the home folder, as a terminal opens, so tools that read the folder they start in,
    // such as mise and direnv, don't set up the one Patchwork happened to start in.
    if let Some(home) = crate::system::fs::home_dir() {
        cmd.current_dir(home);
    }
    cmd.args(flags)
        .arg(&script)
        .envs(PROBE_ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A session of its own, away from the terminal Patchwork may have been started from: an
    // interactive shell in a background group that touches it is stopped until the time runs out.
    #[cfg(unix)]
    // SAFETY: setsid is async-signal-safe and touches no memory of this process.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        })
    };
    let mut child = cmd.spawn().ok()?;
    let mut pipe = child.stdout.take()?;
    let out = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let out = Arc::clone(&out);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 16 << 10];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                out.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..n]);
            }
        })
    };
    // Read once the closing marker is in: what a startup file started in the background can hold
    // the pipe open long after, and a logout file still runs as the shell exits.
    let printed = || {
        let out = out.lock().unwrap_or_else(PoisonError::into_inner);
        out.windows(marker.len())
            .filter(|w| *w == marker.as_bytes())
            .count()
            >= 2
    };
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let done = loop {
        if printed() {
            break Some(false);
        }
        match child.try_wait() {
            Ok(Some(_)) => break Some(true),
            // Short, so a fast shell returns at once instead of after a fixed wait.
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => break None,
        }
    };
    match done {
        // The shell finishes on its own, and is reaped once it has.
        Some(false) => {
            std::thread::spawn(move || child.wait());
        }
        Some(true) => {
            // Time for the reader to take the rest of the output after the shell exits.
            let grace = Instant::now() + Duration::from_secs(1);
            while !reader.is_finished() && Instant::now() < grace {
                // Short, so the reader is joined as soon as it finishes.
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        None => {
            #[cfg(unix)]
            // SAFETY: kill only sends a signal; the group is the one this child leads.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL)
            };
            let _ = child.kill();
            let _ = child.wait();
            log::warn!(
                shell:%, flags:% = flags.join(" ");
                "{{shell}} {{flags}} did not print its environment in time"
            );
            return None;
        }
    }
    let out = std::mem::take(&mut *out.lock().unwrap_or_else(PoisonError::into_inner));
    parse(&String::from_utf8_lossy(&out), &marker)
}

/// The variables printed between the two markers, as `env -0` or `env` prints them.
fn parse(out: &str, marker: &str) -> Option<Vec<(String, String)>> {
    let (_, rest) = out.split_once(marker)?;
    let (body, _) = rest.split_once(marker)?;
    let mut vars: Vec<(String, String)> = Vec::new();
    if body.contains('\0') {
        for entry in body.split('\0') {
            if let Some((key, value)) = entry.split_once('=') {
                vars.push((key.to_string(), value.to_string()));
            }
        }
    } else {
        // Without `-0` a value with a new line spans lines; a line that doesn't start a variable
        // belongs to the one before it.
        for line in body.lines() {
            match line.split_once('=').filter(|(key, _)| is_name(key)) {
                Some((key, value)) => vars.push((key.to_string(), value.to_string())),
                None => {
                    if let Some((_, value)) = vars.last_mut() {
                        value.push('\n');
                        value.push_str(line);
                    }
                }
            }
        }
    }
    vars.retain(|(key, _)| {
        is_name(key)
            && !NOT_CARRIED.contains(&key.as_str())
            && !PROBE_ENV.iter().any(|(k, _)| k == key)
            && !LAUNCHER_OWNED.iter().any(|prefix| key.starts_with(prefix))
    });
    Some(vars)
}

/// Whether `key` is a variable name a shell exports.
fn is_name(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with(|c: char| c.is_ascii_digit())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
