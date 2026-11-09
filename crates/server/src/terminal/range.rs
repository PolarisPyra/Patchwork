//! The processes a terminal owns: its shell and everything the shell started, found in the
//! process table and remembered once seen, so a program that outlives its parent is still ended
//! with the terminal. Every signal reaches only the process that was seen, never a later one
//! given the same pid.

// Rust guideline compliant 2026-10-09

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, PoisonError};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::activity::Activity;
use super::session::Term;
use crate::Error;
use crate::system::processes::{self, Process, Snapshot};

/// How long a process gets to stop after each signal.
const GRACE: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(25);

/// The processes the shell started that still run: those seen before, and those now in its tree
/// or its session. Its tree and session count only while its pid is still the shell, since a pid
/// handed on would bring another program's processes.
pub(super) fn descendants(term: &Term, table: &Snapshot) -> Vec<Process> {
    let mut tracked = term.tracked.lock().unwrap_or_else(PoisonError::into_inner);
    let mut found = std::mem::take(&mut *tracked);
    if let (Some(pid), Some(root)) = (term.pid, &term.root) {
        let tree = table.tree(pid);
        if tree.iter().any(|p| same(p, root)) {
            found.extend(tree);
            found.extend(table.session(pid));
        }
    }
    let mut members: Vec<Process> = Vec::new();
    for p in found {
        if Some(p.pid) != term.pid && table.alive(&p) && !members.iter().any(|m| same(m, &p)) {
            members.push(p);
        }
    }
    (*tracked).clone_from(&members);
    members
}

/// Ends what the terminal owns: what the shell started, then the shell, then anything started
/// meanwhile. Each is asked to stop, and made to a second later. Err names what still runs.
pub(super) fn close(term: &Term) -> Result<(), Error> {
    let survivors = stop_descendants(term);
    if !survivors.is_empty() {
        return Err(still_running(&survivors).into());
    }
    stop_shell(term)?;
    let survivors = stop_descendants(term);
    if !survivors.is_empty() {
        return Err(still_running(&survivors).into());
    }
    Ok(())
}

/// Ends what the terminal owns at once, as Patchwork quits.
pub(super) fn kill(term: &Term) {
    let members = |term: &Term| match Snapshot::read() {
        Some(table) => descendants(term, &table),
        None => term
            .tracked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    };
    signal_all(&members(term), Signal::Kill);
    if !term.exited.load(Ordering::Acquire) {
        signal_shell(term, Signal::Kill);
    }
    signal_all(&members(term), Signal::Kill);
}

/// The shell's state for idle reclaim: busy while anything it started runs or a program holds
/// the screen, else what its prompt hooks last recorded; unknown when that can't be told. With
/// a revision that changes whenever the answer or the record behind it does.
pub(super) fn inspect(term: &Term) -> (Activity, u64) {
    let (shell, revision) = match (&term.activity, term.pid) {
        (Some(activity), Some(pid)) => activity.inspect(pid),
        _ => (Activity::Unknown, 0),
    };
    let state = match (Snapshot::read(), term.pid) {
        (Some(table), Some(pid)) => {
            let descendants = descendants(term, &table);
            let root = table.tree(pid).into_iter().find(|p| p.pid == pid);
            let exited = term.exited.load(Ordering::Acquire);
            if !descendants.is_empty() {
                Activity::Busy
            } else if exited && cfg!(target_os = "linux") && root.is_none() {
                if table.session(pid).iter().any(|p| table.alive(p)) {
                    Activity::Busy
                } else {
                    Activity::Idle
                }
            } else if root.is_some_and(|r| term.root.as_ref().is_some_and(|t| same(&r, t))) {
                foreground(term, pid, shell)
            } else {
                Activity::Unknown
            }
        }
        _ => Activity::Unknown,
    };
    let key = format!("{revision}:{}", state.name());
    let mut seen = term.observed.lock().unwrap_or_else(PoisonError::into_inner);
    if key != seen.0 {
        seen.0 = key;
        seen.1 += 1;
    }
    (state, seen.1)
}

/// The shell's own state while it holds the screen; busy while another program does.
#[cfg(unix)]
fn foreground(term: &Term, pid: i32, shell: Activity) -> Activity {
    let master = term.master.lock().unwrap_or_else(PoisonError::into_inner);
    match master.process_group_leader() {
        None => Activity::Unknown,
        Some(group) if group == pid => shell,
        Some(_) => Activity::Busy,
    }
}

#[cfg(not(unix))]
fn foreground(_term: &Term, _pid: i32, _shell: Activity) -> Activity {
    Activity::Unknown
}

/// The same process: the same pid, started at the same time.
fn same(a: &Process, b: &Process) -> bool {
    a.pid == b.pid && a.started == b.started
}

fn still_running(members: &[Process]) -> String {
    let pids: Vec<String> = members.iter().map(|p| p.pid.to_string()).collect();
    format!("Could not end the terminal's processes {}", pids.join(", "))
}

fn stop_descendants(term: &Term) -> Vec<Process> {
    let Some(table) = Snapshot::read() else {
        return Vec::new();
    };
    let captured = descendants(term, &table);
    signal_all(&captured, Signal::Stop);
    let mut members = wait_for(&captured);
    if let Some(table) = Snapshot::read() {
        union(&mut members, descendants(term, &table));
    }
    signal_all(&members, Signal::Kill);
    let mut survivors = wait_for(&members);
    match Snapshot::read() {
        Some(table) => {
            union(&mut survivors, descendants(term, &table));
            survivors.retain(|p| table.alive(p));
            survivors
        }
        None => survivors,
    }
}

fn union(members: &mut Vec<Process>, more: Vec<Process>) {
    for p in more {
        if !members.iter().any(|m| same(m, &p)) {
            members.push(p);
        }
    }
}

/// The members still running once they stop or [`GRACE`] runs out.
fn wait_for(members: &[Process]) -> Vec<Process> {
    let until = Instant::now() + GRACE;
    loop {
        let Some(table) = Snapshot::read() else {
            return members.to_vec();
        };
        let survivors: Vec<Process> = members.iter().filter(|p| table.alive(p)).cloned().collect();
        if survivors.is_empty() || Instant::now() >= until {
            return survivors;
        }
        std::thread::sleep(POLL);
    }
}

fn stop_shell(term: &Term) -> Result<(), Error> {
    for signal in [Signal::Stop, Signal::Kill] {
        if term.exited.load(Ordering::Acquire) {
            return Ok(());
        }
        signal_shell(term, signal);
        let until = Instant::now() + GRACE;
        while !term.exited.load(Ordering::Acquire) && Instant::now() < until {
            std::thread::sleep(POLL);
        }
    }
    if term.exited.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(format!(
            "Could not end the terminal's shell {}",
            term.pid.unwrap_or_default()
        )
        .into())
    }
}

#[derive(Clone, Copy)]
enum Signal {
    /// Asks a program to stop.
    Stop,
    Kill,
}

#[cfg(unix)]
fn signal_all(members: &[Process], signal: Signal) {
    for member in members {
        crate::system::processes::signal(member, signal.number());
    }
}

#[cfg(not(unix))]
fn signal_all(_members: &[Process], _signal: Signal) {}

/// Signals the shell as it was seen to start, or through the terminal when it was not seen.
#[cfg(unix)]
fn signal_shell(term: &Term, signal: Signal) {
    match &term.root {
        Some(root) => {
            crate::system::processes::signal(root, signal.number());
        }
        None => {
            let _ = term
                .killer
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .kill();
        }
    }
}

/// On Windows the shell's whole tree goes with it, asked first, then forced.
#[cfg(not(unix))]
fn signal_shell(term: &Term, signal: Signal) {
    let Some(pid) = term.pid else {
        let _ = term
            .killer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kill();
        return;
    };
    let mut cmd = crate::system::host_command("taskkill");
    if matches!(signal, Signal::Kill) {
        cmd.arg("/F");
    }
    let _ = cmd.args(["/T", "/PID", &pid.to_string()]).output();
}

#[cfg(unix)]
impl Signal {
    fn number(self) -> i32 {
        match self {
            Signal::Stop => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        }
    }
}

/// Where terminals keep their records and prompt hooks: a folder for each Patchwork running, named
/// for its pid and start, with one folder in it for each terminal.
fn instances() -> PathBuf {
    crate::system::dirs().cache.join("shells")
}

/// This Patchwork's folder name. None where the process table can't be read.
fn instance() -> Option<&'static str> {
    static INSTANCE: OnceLock<Option<String>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            let own = processes::find(std::process::id() as i32)?;
            Some(format!("{}-{}", own.pid, encode(&own.started)))
        })
        .as_deref()
}

/// A start time as it can be part of a folder name.
fn encode(started: &str) -> String {
    started
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// A new folder only this user can read, for one terminal. None where the process table can't
/// be read, since the folder's name can't then tell this Patchwork apart.
pub(super) fn create_dir() -> Option<PathBuf> {
    let parent = instances().join(instance()?);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&parent).ok()?;
    let dir = parent.join(crate::system::new_id());
    builder.recursive(false).create(&dir).ok()?;
    Some(dir)
}

/// Notes the terminal's shell in its folder, so a Patchwork that starts after this one was killed
/// can end what the shell left running.
pub(super) fn record(dir: &Path, shell: &Process) {
    let text = format!("{}\n{}", shell.pid, shell.started);
    if let Err(e) = write_private(&dir.join("shell"), &text) {
        log::warn!(error:% = e; "could not note the terminal's shell: {{error}}");
    }
}

pub(super) fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(text.as_bytes())
}

/// Ends what the terminals of a Patchwork that was killed left running, and removes their
/// folders. A Patchwork still running keeps its own.
pub fn sweep() {
    let Ok(entries) = std::fs::read_dir(instances()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((pid, started)) = name.split_once('-') else {
            continue;
        };
        let Ok(pid) = pid.parse::<i32>() else {
            continue;
        };
        let running = processes::find(pid).is_some_and(|p| encode(&p.started) == started);
        if Some(name.as_str()) == instance() || running {
            continue;
        }
        let mut kept = false;
        for terminal in std::fs::read_dir(entry.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let shell = std::fs::read_to_string(terminal.path().join("shell")).ok();
            let shell = shell.as_deref().and_then(|text| {
                let (pid, started) = text.split_once('\n')?;
                Some(Process {
                    pid: pid.parse().ok()?,
                    ppid: 0,
                    sid: 0,
                    started: started.to_string(),
                })
            });
            if let Some(shell) = shell {
                let left = left_behind(&shell);
                if !left.is_empty() {
                    let pids: Vec<String> = left.iter().map(|p| p.pid.to_string()).collect();
                    log::info!(
                        pids:% = pids.join(", ");
                        "ending what a terminal left running when Patchwork was killed: {{pids}}"
                    );
                }
                let survivors = end(&left);
                if !survivors.is_empty() {
                    log::warn!(survivors:% = still_running(&survivors); "{{survivors}}");
                    kept = true;
                    continue;
                }
            }
            let _ = std::fs::remove_dir_all(terminal.path());
        }
        if !kept {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// What a shell recorded before Patchwork was killed still runs: the shell, if it does, what it
/// started, and its session. Its pid stays reserved while anything in its session runs, so a
/// session under that number with no process holding it is the shell's own; one led by another
/// process is not.
fn left_behind(shell: &Process) -> Vec<Process> {
    let Some(table) = Snapshot::read() else {
        return Vec::new();
    };
    if table.alive(shell) {
        let mut members = table.tree(shell.pid);
        union(&mut members, table.session(shell.pid));
        members
    } else if processes::find(shell.pid).is_none() {
        table.session(shell.pid)
    } else {
        Vec::new()
    }
}

/// Asks `members` to stop and makes those still running stop; those that would not.
fn end(members: &[Process]) -> Vec<Process> {
    signal_all(members, Signal::Stop);
    let survivors = wait_for(members);
    signal_all(&survivors, Signal::Kill);
    wait_for(&survivors)
}
