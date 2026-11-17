//! The processes running on the machine, and signals to them.
//!
//! They are read from the process table, and a signal reaches a process only while it is still
//! the one that was read, so a pid the system has since handed to another program is never hit.
//! Windows has no such table here.

// Rust guideline compliant 2026-10-09

/// One running process, with when it started, which tells it apart from a later process given the
/// same pid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: i32,
    pub ppid: i32,
    /// The session it belongs to, or 0 where the table does not say.
    pub sid: i32,
    /// When it started, in the table's own terms.
    pub started: String,
}

/// The process table at one moment, without processes that have exited and wait to be reaped.
#[derive(Debug)]
pub struct Snapshot(Vec<Process>);

impl Snapshot {
    /// The table now; None where it can't be read.
    pub fn read() -> Option<Snapshot> {
        read_all().map(Snapshot)
    }

    /// `pid` and every process it started, and theirs.
    #[must_use]
    pub fn tree(&self, pid: i32) -> Vec<Process> {
        let mut found: Vec<Process> = self.0.iter().filter(|p| p.pid == pid).cloned().collect();
        let mut parents = vec![pid];
        while let Some(parent) = parents.pop() {
            for child in self.0.iter().filter(|p| p.ppid == parent && p.pid != pid) {
                if !found.contains(child) {
                    parents.push(child.pid);
                    found.push(child.clone());
                }
            }
        }
        found
    }

    /// The processes in the session `pid` leads.
    #[must_use]
    pub fn session(&self, pid: i32) -> Vec<Process> {
        self.0
            .iter()
            .filter(|p| pid > 1 && p.sid == pid)
            .cloned()
            .collect()
    }

    /// Whether `process` is still running: its pid is in the table, with the same start.
    #[must_use]
    pub fn alive(&self, process: &Process) -> bool {
        self.0
            .iter()
            .any(|p| p.pid == process.pid && p.started == process.started)
    }
}

/// Process `pid` as the table has it now.
#[must_use]
pub fn find(pid: i32) -> Option<Process> {
    #[cfg(unix)]
    return read_one(pid);
    #[cfg(not(unix))]
    return read_all()?.into_iter().find(|p| p.pid == pid);
}

/// Sends `signal` to `process` if it is still the process that was read. Never to init or to
/// Patchwork itself. Whether it was sent.
#[cfg(unix)]
#[expect(
    clippy::must_use_candidate,
    reason = "it is called to send the signal; few callers need to know whether it was sent"
)]
pub fn signal(process: &Process, signal: i32) -> bool {
    if process.pid <= 1 || process.pid == std::process::id() as i32 {
        return false;
    }
    if read_one(process.pid).is_none_or(|now| now.started != process.started) {
        return false;
    }
    // SAFETY: kill only sends a signal, to a single pid checked above.
    unsafe { libc::kill(process.pid, signal) == 0 }
}

#[cfg(target_os = "linux")]
fn read_all() -> Option<Vec<Process>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    Some(
        entries
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
            .filter_map(read_one)
            .collect(),
    )
}

/// Process `pid` from `/proc/<pid>/stat`, whose fields after the program's name, which may hold
/// spaces and parentheses, are its state, parent, group, session, …, and 20th its start time.
#[cfg(target_os = "linux")]
fn read_one(pid: i32) -> Option<Process> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<&str> = stat
        .get(stat.rfind(')')? + 1..)?
        .split_whitespace()
        .collect();
    if *fields.first()? == "Z" {
        return None;
    }
    Some(Process {
        pid,
        ppid: fields.get(1)?.parse().ok()?,
        sid: fields.get(3)?.parse().ok()?,
        started: fields.get(19)?.to_string(),
    })
}

/// Elsewhere on Unix, from `ps`, which names no session.
#[cfg(all(unix, not(target_os = "linux")))]
fn read_all() -> Option<Vec<Process>> {
    ps(&["-A"])
}

#[cfg(all(unix, not(target_os = "linux")))]
fn read_one(pid: i32) -> Option<Process> {
    ps(&["-p", &pid.to_string()])?
        .into_iter()
        .find(|p| p.pid == pid)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn ps(which: &[&str]) -> Option<Vec<Process>> {
    let out = crate::host_command("ps")
        .args(which)
        .args(["-o", "pid=,ppid=,stat=,lstart="])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(
        text.lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let pid = parts.next()?.parse().ok()?;
                let ppid = parts.next()?.parse().ok()?;
                let state = parts.next()?;
                let started = parts.collect::<Vec<_>>().join(" ");
                (!state.starts_with('Z')).then_some(Process {
                    pid,
                    ppid,
                    sid: 0,
                    started,
                })
            })
            .collect(),
    )
}

#[cfg(not(unix))]
fn read_all() -> Option<Vec<Process>> {
    None
}

/// Sends `signal` to the process group led by `pid`. Agent children are spawned
/// as group leaders, so this also reaches their own subprocesses.
#[cfg(unix)]
pub fn signal_process_group(pid: u32, signal: i32) {
    // SAFETY: kill(2) has no memory-safety preconditions; a group that already
    // exited just yields an ignored error.
    unsafe { libc::kill(-(pid as libc::pid_t), signal) };
}

/// Ends the process and the processes it started, as `taskkill /T` finds them.
///
/// Windows has no signals a console program can be sent, so either one does this. Agents started
/// through a `.cmd` shim run under `cmd.exe`, so killing the shim alone would leave the agent
/// running.
#[cfg(not(unix))]
pub fn signal_process_group(pid: u32, _signal: i32) {
    let _ = crate::host_command("taskkill")
        .args(["/T", "/F", "/PID"])
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Signals for `signal_process_group`. Windows libc has no SIGKILL, and the call ignores the
/// signal there, so the values only need to exist.
#[cfg(unix)]
pub use libc::{SIGKILL, SIGTERM};
#[cfg(not(unix))]
pub const SIGKILL: i32 = 9;
#[cfg(not(unix))]
pub const SIGTERM: i32 = 15;
