//! A command run in a range of its own: a process group of its own, and on Linux a systemd scope
//! of its own too, which still holds what it starts in sessions of its own; on Windows, a job
//! object. Stopping it sends SIGTERM to all of it, then SIGKILL to what is left after
//! [`STOP_GRACE`]. Windows has no SIGTERM, so the job ends at once there.

// Rust guideline compliant 2026-10-09

use std::io;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

/// How long a stopped command has to end before what is left of it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

/// Starts `cmd` in a range of its own, with `stdin` and its output piped. The scope is a new
/// command that takes none of `cmd`'s pipes, which is why they are given here.
pub fn spawn(cmd: Command, stdin: Stdio) -> io::Result<(Child, Range)> {
    let (mut cmd, unit) = super::scope::command_scope(cmd);
    cmd.stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_SUSPENDED: u32 = 0x0000_0004;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }
    let child = cmd.spawn()?;
    #[cfg(windows)]
    let job = {
        // It starts suspended, so nothing it starts is outside the job.
        let job = windows::Job::new();
        if let Some(job) = &job {
            job.assign(&child);
        }
        windows::resume(child.id());
        job.map(std::sync::Arc::new)
    };
    #[cfg(windows)]
    let _ = unit;
    let range = Range {
        pid: child.id(),
        #[cfg(unix)]
        unit,
        #[cfg(windows)]
        job,
    };
    Ok((child, range))
}

/// What one command and everything it started run in.
#[derive(Debug, Clone)]
pub struct Range {
    pid: u32,
    #[cfg(unix)]
    unit: Option<String>,
    #[cfg(windows)]
    job: Option<std::sync::Arc<windows::Job>>,
}

impl Range {
    /// Stops the command and everything it started, and returns at once: SIGTERM now, and
    /// SIGKILL to what is left after [`STOP_GRACE`].
    pub fn stop(&self) {
        #[cfg(unix)]
        {
            use crate::workspace::{SIGKILL, SIGTERM, signal_process_group};
            // The scope's own stop signals all of it, then kills what is left after the grace.
            // A scope not yet set up, or none, leaves it to the group.
            let scoped = self.unit.as_deref().is_some_and(super::scope::stop);
            if !scoped {
                signal_process_group(self.pid, SIGTERM);
            }
            let pid = self.pid;
            std::thread::spawn(move || {
                let deadline = Instant::now() + STOP_GRACE;
                while group_alive(pid) && Instant::now() < deadline {
                    // How often the group is checked while it is given time to exit.
                    std::thread::sleep(Duration::from_millis(20));
                }
                signal_process_group(pid, SIGKILL);
            });
        }
        #[cfg(windows)]
        match &self.job {
            Some(job) => job.terminate(),
            None => crate::workspace::signal_process_group(self.pid, crate::workspace::SIGKILL),
        }
    }

    /// Whether anything the command started still runs, with the command itself or after it.
    pub fn running(&self) -> bool {
        #[cfg(unix)]
        {
            group_alive(self.pid) || self.unit.as_deref().is_some_and(super::scope::active)
        }
        #[cfg(windows)]
        {
            self.job.as_ref().is_some_and(|job| job.active())
        }
    }
}

/// Whether any process is left in the process group `pid` leads.
#[cfg(unix)]
fn group_alive(pid: u32) -> bool {
    // SAFETY: kill(2) with signal 0 only checks that the group exists.
    unsafe { libc::kill(-(pid as libc::pid_t), 0) == 0 }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
        QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    /// A job object that ends what is in it when Patchwork closes it or exits.
    #[derive(Debug)]
    pub(super) struct Job(HANDLE);

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        pub(super) fn new() -> Option<Job> {
            // SAFETY: plain Win32 calls on a handle this owns; the limits struct is zeroed
            // before its one flag is set.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let job = Job(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                Some(job)
            }
        }

        pub(super) fn assign(&self, child: &Child) {
            // SAFETY: both handles are open for the call.
            unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
        }

        pub(super) fn terminate(&self) {
            // SAFETY: the handle is open.
            unsafe { TerminateJobObject(self.0, 1) };
        }

        pub(super) fn active(&self) -> bool {
            // SAFETY: the handle is open and the struct is the size the class asks for.
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicAccountingInformation,
                    (&raw mut info).cast(),
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                ) != 0
                    && info.ActiveProcesses > 0
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is open and closed only here.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Resumes the threads of the process `pid`, which was started suspended.
    pub(super) fn resume(pid: u32) {
        // SAFETY: the snapshot and each thread handle are closed after use, and the entry's
        // size is set before the first read.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot.is_null() || snapshot as isize == -1 {
                return;
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snapshot, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        ResumeThread(thread);
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
    }
}
