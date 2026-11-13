//! What Patchwork's helper does on an SSH host: run a command for Patchwork, open a shell, and
//! unpack the files Patchwork copies there.
//!
//! The helper is a small build of Patchwork's backend for Linux, macOS and Windows hosts.
//! Patchwork starts it over SSH with one of the arguments here; the hooks and proxies an agent
//! starts on the host run it with the same arguments they would give Patchwork's own binary. Each
//! argument after these is a plain word, so the host's shell, whichever it is, passes it as is.

// Rust guideline compliant 2026-10-09

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use super::command::Spec;
use super::upload::Kind;

/// The argument that runs the command a staged spec describes, named by its id, until the
/// connection ends.
pub const EXEC_ARG: &str = "--remote-exec";

/// The argument that runs the command the hex spec after it describes, as [`EXEC_ARG`] does.
pub const RUN_ARG: &str = "--remote-run";

/// The argument that opens a shell in the folder whose path follows in hex, on Windows hosts.
pub const SHELL_ARG: &str = "--remote-shell";

/// The argument that unpacks the files on stdin into the helper's mirror folder.
pub const UNPACK_ARG: &str = "--remote-unpack";

#[doc(inline)]
pub use super::answer::answer;
#[doc(inline)]
pub use super::op::OP_ARG;

/// Runs the command a staged spec describes, named after [`EXEC_ARG`], and returns its exit
/// code.
///
/// The command gets the host's login environment, and runs with everything it starts in a group
/// of its own. When the connection that started the helper ends, its output closes, and the
/// helper ends the group.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "ssh carries the helper's stderr back to Patchwork; it has no log of its own"
)]
pub fn exec() -> i32 {
    let id = std::env::args().nth(2).unwrap_or_default();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        eprintln!("Usage: {EXEC_ARG} <id>");
        return 2;
    }
    let Some(file) = super::answer::base().map(|b| b.join("run").join(format!("{id}.json"))) else {
        eprintln!("The helper cannot find its folder");
        return 2;
    };
    let read = std::fs::read(&file);
    let _ = std::fs::remove_file(&file);
    match read {
        Ok(bytes) => start(&bytes),
        Err(e) => {
            eprintln!("Cannot read {}: {e}", file.display());
            2
        }
    }
}

/// Runs the command the hex spec after [`RUN_ARG`] describes, and returns its exit code.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "ssh carries the helper's stderr back to Patchwork; it has no log of its own"
)]
pub fn run_inline() -> i32 {
    if let Some(bytes) = std::env::args().nth(2).as_deref().and_then(unhex) {
        start(&bytes)
    } else {
        eprintln!("Usage: {RUN_ARG} <hex spec>");
        2
    }
}

#[expect(
    clippy::print_stderr,
    reason = "ssh carries the helper's stderr back to Patchwork; it has no log of its own"
)]
fn start(bytes: &[u8]) -> i32 {
    let spec: Spec = match serde_json::from_slice(bytes) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("Unreadable command: {e}");
            return 2;
        }
    };
    let program = if spec.program.contains(['/', '\\']) {
        PathBuf::from(&spec.program)
    } else if let Some(found) = crate::system::find_program(&spec.program) {
        found
    } else {
        eprintln!("{} is not installed on this machine", spec.program);
        return 127;
    };
    let mut cmd = Command::new(program);
    cmd.current_dir(&spec.cwd);
    args(&mut cmd, &spec.args);
    for (key, value) in crate::system::login_env().iter() {
        cmd.env(key, value);
    }
    cmd.envs(spec.env.iter().map(|(k, v)| (k, v)));
    run(cmd)
}

/// Gives `cmd` its arguments. On Windows, `cmd /d /s /c <line>` gets its line as written, since
/// cmd reads its own command line rather than the arguments Rust would quote.
fn args(cmd: &mut Command, args: &[String]) {
    #[cfg(windows)]
    if let [d, s, c, line] = args
        && [d, s, c].iter().all(|a| a.starts_with('/'))
        && c.eq_ignore_ascii_case("/c")
    {
        use std::os::windows::process::CommandExt;
        cmd.args([d, s, c]).raw_arg(format!("\"{line}\""));
        return;
    }
    cmd.args(args);
}

/// Opens a shell in the folder whose hex path follows [`SHELL_ARG`], on the terminal ssh gave
/// the helper, and returns its exit code. PowerShell when there is one, else cmd.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "ssh carries the helper's stderr back to Patchwork; it has no log of its own"
)]
pub fn shell() -> i32 {
    let dir = std::env::args()
        .nth(2)
        .as_deref()
        .and_then(unhex)
        .and_then(|b| String::from_utf8(b).ok());
    let program = ["pwsh", "powershell"]
        .into_iter()
        .find_map(crate::system::find_program)
        .or_else(|| std::env::var_os("COMSPEC").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("cmd"));
    let mut cmd = Command::new(program);
    if let Some(dir) = dir.filter(|d| Path::new(d).is_dir()) {
        cmd.current_dir(dir);
    } else {
        eprintln!("The project's folder is not there; the shell opens in the home folder");
    }
    run(cmd)
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(unix)]
#[expect(
    clippy::print_stderr,
    reason = "the helper's stderr goes back to Patchwork over ssh"
)]
fn run(mut cmd: Command) -> i32 {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Cannot start {}: {e}", cmd.get_program().to_string_lossy());
            return 126;
        }
    };
    let Ok(group) = i32::try_from(child.id()) else {
        return 1;
    };
    std::thread::spawn(move || {
        watch_output();
        stop(group);
    });
    let code = match child.wait() {
        Ok(status) => status
            .code()
            .or_else(|| status.signal().map(|s| 128 + s))
            .unwrap_or(1),
        Err(_) => 1,
    };
    // What the command started and left running goes with it, as a local turn's scope ends.
    // SAFETY: killpg only sends a signal to the process group the command was started in.
    unsafe {
        libc::killpg(group, libc::SIGTERM);
    };
    code
}

#[cfg(windows)]
#[expect(
    clippy::print_stderr,
    reason = "the helper's stderr goes back to Patchwork over ssh"
)]
fn run(mut cmd: Command) -> i32 {
    // The helper joins a job of its own first, so the command and all it starts are in it, and
    // the job ends them all when the helper exits, however it does.
    if let Some(job) = crate::system::contained::windows::Job::new()
        && job.assign_self()
    {
        // Closed by the helper's exit, not before.
        std::mem::forget(job);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Cannot start {}: {e}", cmd.get_program().to_string_lossy());
            return 126;
        }
    };
    // Windows' OpenSSH server leaves a command running when the connection ends, so the helper
    // watches for it and exits, which ends the job.
    std::thread::spawn(|| {
        watch_output();
        std::process::exit(1);
    });
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

#[cfg(not(any(unix, windows)))]
#[expect(
    clippy::print_stderr,
    reason = "the helper's stderr goes back to Patchwork over ssh"
)]
fn run(_cmd: Command) -> i32 {
    eprintln!("The helper does not run commands on this system");
    2
}

/// Returns once the reader of the helper's output has gone, which is when Patchwork stops the
/// command or the connection drops.
#[cfg(unix)]
fn watch_output() {
    loop {
        let mut fd = libc::pollfd {
            fd: 1,
            events: 0,
            revents: 0,
        };
        // SAFETY: `fd` is one valid pollfd, and the count given is 1.
        let ready = unsafe { libc::poll(&raw mut fd, 1, 1000) };
        if ready > 0 && fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return;
        }
    }
}

/// Returns once the reader of the helper's output has gone. A pipe whose reader has closed
/// refuses even an empty write, so the helper tries one each second.
#[cfg(windows)]
fn watch_output() {
    use windows_sys::Win32::Foundation::{
        ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED, GetLastError,
    };
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut written = 0;
        // SAFETY: the standard output handle stays open while the helper runs, and an empty
        // write reads nothing from the buffer.
        let gone = unsafe {
            let out = GetStdHandle(STD_OUTPUT_HANDLE);
            WriteFile(
                out,
                [0u8].as_ptr(),
                0,
                &raw mut written,
                std::ptr::null_mut(),
            ) == 0
                && matches!(
                    GetLastError(),
                    ERROR_BROKEN_PIPE | ERROR_NO_DATA | ERROR_PIPE_NOT_CONNECTED
                )
        };
        if gone {
            return;
        }
    }
}

/// Ends the process group `group`: asks first, then forces what is still there.
#[cfg(unix)]
fn stop(group: i32) {
    // SAFETY: killpg only sends a signal to the process group the command was started in.
    unsafe {
        libc::killpg(group, libc::SIGTERM);
    };
    std::thread::sleep(std::time::Duration::from_secs(3));
    // SAFETY: as above.
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
}

/// Unpacks the files on stdin into the helper's mirror folder, and returns the exit code: 0 when
/// every file was written.
#[must_use]
#[expect(
    clippy::print_stderr,
    reason = "ssh carries the helper's stderr back to Patchwork; it has no log of its own"
)]
pub fn unpack() -> i32 {
    let Some(dest) = super::answer::base().map(|b| b.join("mirror")) else {
        eprintln!("The helper cannot find its folder");
        return 2;
    };
    match unpack_into(&dest, &mut std::io::stdin().lock()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn read_len(input: &mut impl Read) -> std::io::Result<Option<u64>> {
    let mut len = [0; 8];
    match input.read_exact(&mut len) {
        Ok(()) => Ok(Some(u64::from_be_bytes(len))),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

fn unpack_into(dest: &Path, input: &mut impl Read) -> Result<(), String> {
    while let Some(len) = read_len(input).map_err(|e| e.to_string())? {
        let mut path = vec![0; usize::try_from(len).map_err(|e| e.to_string())?];
        input.read_exact(&mut path).map_err(|e| e.to_string())?;
        let path = String::from_utf8(path).map_err(|e| e.to_string())?;
        let mut kind = [0; 1];
        input.read_exact(&mut kind).map_err(|e| e.to_string())?;
        let size = read_len(input)
            .map_err(|e| e.to_string())?
            .ok_or("The stream ended early")?;
        let mut bytes = Vec::new();
        input
            .take(size)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        // Only plain names below the folder, so a stream cannot write outside it.
        let rel = Path::new(&path);
        if rel
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(format!("{path} leaves the folder"));
        }
        let target = dest.join(rel.strip_prefix("/").unwrap_or(rel));
        if kind[0] == Kind::Folder as u8 {
            make_dir(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            make_dir(parent)?;
        }
        write(&target, &bytes, kind[0] == Kind::Program as u8)?;
    }
    Ok(())
}

fn make_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("Cannot create {}: {e}", path.display()))
}

/// Writes `bytes` to `path` through a file beside it, so a command never reads half a file.
fn write(path: &Path, bytes: &[u8], program: bool) -> Result<(), String> {
    let part = path.with_extension("patchwork-part");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if program { 0o700 } else { 0o600 })
    };
    #[cfg(not(unix))]
    let _ = program;
    let mut file = options
        .open(&part)
        .map_err(|e| format!("Cannot write {}: {e}", part.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("Cannot write {}: {e}", part.display()))?;
    std::fs::rename(&part, path).map_err(|e| format!("Cannot write {}: {e}", path.display()))
}
