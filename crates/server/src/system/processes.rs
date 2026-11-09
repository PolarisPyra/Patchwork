//! The processes running on the machine, read from the process table, and signals that reach a
//! process only while it is still the one that was read, so a pid the system has since handed to
//! another program is never hit. Windows has no such table here.

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
pub struct Snapshot(Vec<Process>);

impl Snapshot {
    /// The table now; None where it can't be read.
    pub fn read() -> Option<Snapshot> {
        read_all().map(Snapshot)
    }

    /// `pid` and every process it started, and theirs.
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
    pub fn session(&self, pid: i32) -> Vec<Process> {
        self.0
            .iter()
            .filter(|p| pid > 1 && p.sid == pid)
            .cloned()
            .collect()
    }

    /// Whether `process` is still running: its pid is in the table, with the same start.
    pub fn alive(&self, process: &Process) -> bool {
        self.0
            .iter()
            .any(|p| p.pid == process.pid && p.started == process.started)
    }
}

/// Process `pid` as the table has it now.
pub fn find(pid: i32) -> Option<Process> {
    #[cfg(unix)]
    return read_one(pid);
    #[cfg(not(unix))]
    return read_all()?.into_iter().find(|p| p.pid == pid);
}

/// Sends `signal` to `process` if it is still the process that was read. Never to init or to
/// Patchwork itself. Whether it was sent.
#[cfg(unix)]
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
    let out = crate::system::host_command("ps")
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
