//! The environment the user's own shell sets up, read from that shell once.
//!
//! An app launched from the desktop gets the session's environment, which lacks what shell
//! startup files add: nvm, pnpm, cargo, mise, pyenv, Homebrew, and any `PATH=` line in
//! `.zshrc`, `.bashrc` or `config.fish`. So the user's login shell is started the way a terminal
//! starts it, interactive and as a login shell, and asked for its whole environment between
//! markers, since startup files may print too. The result is kept until one of those files
//! changes. Commands then run without the startup files, in this environment
//! (`shell_command`), so they find the same tools a terminal does without re-reading them.

use std::ffi::OsString;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use crate::system::fs::home_dir;

/// Set on a process Patchwork starts with the environment already read, such as its own agent,
/// so it uses the one it was given instead of starting the user's shell again.
pub const ENV_READ: &str = "PATCHWORK_LOGIN_ENV";

/// How long the user's shell gets to start and print its environment.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Variables that describe the probe's own shell rather than the user's environment.
const NOT_CARRIED: [&str; 7] = ["PWD", "OLDPWD", "SHLVL", "_", "TERM", "COLUMNS", "LINES"];

/// Set for the probe, so a startup file does not stop to ask about updates or start tmux.
const PROBE_ENV: [(&str, &str); 2] = [
    ("DISABLE_AUTO_UPDATE", "true"),
    ("ZSH_TMUX_AUTOSTART", "false"),
];

/// What a read found, and the startup files' state it was read at.
struct Reading {
    key: Vec<Option<SystemTime>>,
    vars: Option<Arc<Vec<(String, String)>>>,
}

static READING: Mutex<Option<Reading>> = Mutex::new(None);

/// The variables the user's shell sets up, read again once a startup file changes. Empty when
/// the shell can't be read, on Windows, and in a process started with them already.
pub fn login_env() -> Arc<Vec<(String, String)>> {
    if cfg!(windows) || std::env::var_os(ENV_READ).is_some() {
        return Arc::default();
    }
    let key = startup_files_state();
    let mut reading = READING.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(r) = reading.as_ref()
        && r.key == key
    {
        return r.vars.clone().unwrap_or_default();
    }
    let vars = read_login_env().map(Arc::new);
    *reading = Some(Reading {
        key,
        vars: vars.clone(),
    });
    vars.unwrap_or_default()
}

/// `name` as the user's shell sets it, else as Patchwork has it. None if unset or empty.
pub fn login_var(name: &str) -> Option<String> {
    login_env()
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .or_else(|| std::env::var(name).ok())
        .filter(|value| !value.is_empty())
}

/// The PATH to find and run programs with: the user's shell's, then the one Patchwork has, each
/// folder once.
pub fn search_path() -> Option<OsString> {
    let env = login_env();
    let login = env
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, v)| OsString::from(v));
    let mut dirs: Vec<PathBuf> = Vec::new();
    for path in [login, crate::system::path_var()].into_iter().flatten() {
        for dir in std::env::split_paths(&path) {
            if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
    }
    std::env::join_paths(dirs).ok()
}

/// The program `name` on [`search_path`].
pub fn find_program(name: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    which::which_in(name, search_path(), cwd).ok()
}

/// The login shell and the modified times of the startup files it may read, so a change to any
/// of them reads the environment again.
fn startup_files_state() -> Vec<Option<SystemTime>> {
    let home = home_dir().unwrap_or_default();
    let mut files: Vec<PathBuf> = [
        ".zshenv",
        ".zprofile",
        ".zshrc",
        ".zlogin",
        ".bash_profile",
        ".bash_login",
        ".profile",
        ".bashrc",
        ".config/fish/config.fish",
        ".config/fish/conf.d",
    ]
    .iter()
    .map(|f| home.join(f))
    .collect();
    if let Some(zdotdir) = std::env::var_os("ZDOTDIR") {
        let dir = PathBuf::from(zdotdir);
        files.extend([".zshenv", ".zprofile", ".zshrc", ".zlogin"].map(|f| dir.join(f)));
    }
    let shell = crate::system::login_shell().map(PathBuf::from);
    files
        .iter()
        .chain(shell.as_ref())
        .map(|f| std::fs::metadata(f).and_then(|m| m.modified()).ok())
        .collect()
}

/// Starts the user's login shell as a terminal would and reads its environment. Bash as a login
/// shell skips `.bashrc` unless its profile sources it, while a terminal's bash reads only
/// `.bashrc`, so bash is read both ways and the two joined. A shell whose startup files hand over
/// to another program, such as `exec tmux`, is read as a plain login shell instead. Nushell and
/// others that can't run the probe are read through bash.
fn read_login_env() -> Option<Vec<(String, String)>> {
    let login = crate::system::login_shell().or_else(|| std::env::var("SHELL").ok())?;
    let name = crate::system::shell_name(&login);
    let shell = if crate::system::is_posix(&login) || name == "fish" {
        login
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        // Not `find_shell`, which reads the environment this is reading.
        which::which_in("bash", crate::system::path_var(), cwd)
            .ok()?
            .to_string_lossy()
            .into_owned()
    };
    let mut flags: Vec<&[&str]> = vec![&["-l", "-i", "-c"]];
    if crate::system::shell_name(&shell) == "bash" {
        flags.push(&["-i", "-c"]);
    }
    let readings: Vec<Vec<(String, String)>> = flags
        .iter()
        .filter_map(|flags| probe(&shell, flags))
        .collect();
    let mut readings = readings.into_iter();
    let Some(mut vars) = readings.next().or_else(|| probe(&shell, &["-l", "-c"])) else {
        log::warn!("could not read the environment {shell} sets up");
        return None;
    };
    for extra in readings {
        join(&mut vars, extra);
    }
    log::info!("read the {} variables {shell} sets up", vars.len());
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
        });
    }
    let mut child = cmd.spawn().ok()?;
    let mut pipe = child.stdout.take()?;
    let out = Arc::new(Mutex::new(Vec::new()));
    let reader = {
        let out = out.clone();
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
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let exited = loop {
        match child.try_wait() {
            Ok(Some(_)) => break true,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => break false,
        }
    };
    // Whatever a startup file left running, such as an agent it started, could hold the pipe open.
    #[cfg(unix)]
    // SAFETY: kill only sends a signal; the group is the one this child leads.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    if !exited {
        let _ = child.kill();
        let _ = child.wait();
        log::warn!(
            "{shell} {} did not print its environment in time",
            flags.join(" ")
        );
        return None;
    }
    let grace = Instant::now() + Duration::from_secs(1);
    while !reader.is_finished() && Instant::now() < grace {
        std::thread::sleep(Duration::from_millis(10));
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
            && !(PROBE_ENV.iter().any(|(k, _)| k == key) && std::env::var_os(key).is_none())
    });
    Some(vars)
}

/// Whether `key` is a variable name a shell exports.
fn is_name(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with(|c: char| c.is_ascii_digit())
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
