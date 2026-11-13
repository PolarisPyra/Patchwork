//! Emulators: starting one so it outlives Patchwork, waiting for Android to start in it, stopping
//! one, and whether this machine can run one at speed.
//!
//! An emulator is started in a session of its own with its output in a log file, so closing the
//! panel, or Patchwork, leaves it running. Its window is hidden: its screen shows in the panel.
//! Each one started is recorded in the app's state with its console port and process, which is
//! how a later look, or a later launch, knows it as Patchwork's own. Nothing here stops an emulator on its own account. When the user stops one, it
//! is asked to close; only the process of one Patchwork started is ever ended.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::devices::{self, ADB_LIMIT, emulator_port, emulator_serial};
use super::tool::{self, tail, tool_command};
use super::sdk;
use super::types::{Boot, Booting, Sdk};
use crate::persistence::Store;
use crate::{Error, Server};

/// Where the emulators Patchwork started are kept in the app's state.
const STARTED_KEY: &str = "android-emulators";

/// The console ports the emulator itself picks from: the even ones from 5554 to 5682. adb finds
/// emulators by scanning them, so one outside the range would not be listed.
const PORTS: std::ops::RangeInclusive<u16> = 5554..=5682;

/// How long Android gets to start. A cold boot of a new image on a slow disk takes a few minutes;
/// past this, the emulator is stuck rather than slow.
const BOOT_LIMIT: Duration = Duration::from_secs(5 * 60);

/// How often a booting emulator is asked whether Android has started.
const BOOT_POLL: Duration = Duration::from_secs(1);

/// How long an emulator told to close gets to save its snapshot before its process is ended.
const STOP_GRACE: Duration = Duration::from_secs(20);

/// An emulator Patchwork started.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Started {
    avd: String,
    port: u16,
    pid: u32,
    /// When the process started, as the process table says it, which tells it apart from a later
    /// process given the same pid. None where the table has no such time.
    token: Option<String>,
}

impl Started {
    /// Whether the process is still the emulator that was started.
    fn alive(&self) -> bool {
        #[cfg(unix)]
        {
            i32::try_from(self.pid)
                .ok()
                .and_then(crate::system::processes::find)
                .is_some_and(|process| Some(&process.started) == self.token.as_ref())
        }
        #[cfg(not(unix))]
        {
            // Windows has no process table here, so the pid is checked to still be an emulator.
            crate::system::host_command("tasklist")
                .args(["/FI", &format!("PID eq {}", self.pid), "/FO", "CSV", "/NH"])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .is_ok_and(|out| {
                    let listed = String::from_utf8_lossy(&out.stdout).to_lowercase();
                    listed.contains("emulator") || listed.contains("qemu")
                })
        }
    }
}

/// The emulators Patchwork started, and what is known of the ones running.
#[derive(Debug, Default)]
pub struct Emulators {
    /// None until read from the app's state.
    started: Mutex<Option<Vec<Started>>>,
    /// The virtual device each running emulator said it runs, by serial.
    names: Mutex<HashMap<String, String>>,
    /// The machine was found able to run emulators at speed, which does not change while it runs.
    accelerated: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Emulators {
    /// The started emulators still running, read from the app's state the first time.
    fn started(&self, store: &Store) -> Vec<Started> {
        let mut started = lock(&self.started);
        let known = started.get_or_insert_with(|| {
            store
                .state(STARTED_KEY)
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_default()
        });
        let before = known.len();
        known.retain(Started::alive);
        if known.len() != before {
            save(store, known);
        }
        known.clone()
    }

    /// Drops the emulators that have closed, so none is taken for one still running.
    pub(super) fn prune(&self, store: &Store) {
        self.started(store);
    }

    fn record(&self, store: &Store, entry: Started) {
        let mut started = lock(&self.started);
        let known = started.get_or_insert_with(Vec::new);
        known.retain(|other| other.port != entry.port);
        known.push(entry);
        save(store, known);
    }

    /// The virtual device the emulator on `port` runs, when Patchwork started it and it still
    /// runs. Reads only what [`Self::started`] last kept.
    pub(super) fn ours(&self, port: u16) -> Option<String> {
        lock(&self.started)
            .as_ref()?
            .iter()
            .find(|entry| entry.port == port)
            .map(|entry| entry.avd.clone())
    }

    /// The virtual device the emulator `serial` runs, asked for with `ask` the first time.
    pub(super) fn name(
        &self,
        serial: &str,
        ask: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        if let Some(name) = lock(&self.names).get(serial) {
            return Some(name.clone());
        }
        let name = ask()?;
        lock(&self.names).insert(serial.to_string(), name.clone());
        Some(name)
    }

    /// Forgets the names of emulators that are no longer listed, whose serials the next ones reuse.
    pub(super) fn keep_names(&self, serials: &[&str]) {
        lock(&self.names).retain(|serial, _| serials.contains(&serial.as_str()));
    }
}

fn save(store: &Store, started: &[Started]) {
    let json = serde_json::to_string(started).unwrap_or_default();
    if let Err(e) = store.set_state(STARTED_KEY, &json) {
        log::warn!(error:% = e; "could not record the started emulators: {{error}}");
    }
}

/// Whether this machine can run an emulator at speed, with what to do when it can't.
///
/// Linux is asked directly whether `/dev/kvm` is there and this user may open it, which holds
/// inside the AppImage too. Windows and macOS are asked through the emulator's own check, which
/// knows WHPX, AEHD and the Hypervisor framework.
///
/// # Errors
///
/// Returns an error that names the missing piece and how to turn it on.
pub fn acceleration(sdk: &Sdk) -> Result<(), Error> {
    if cfg!(target_os = "linux") {
        return kvm(Path::new("/dev/kvm"));
    }
    let Some(emulator) = sdk.emulator.as_deref() else {
        return Ok(());
    };
    let mut cmd = tool_command(sdk, emulator);
    cmd.arg("-accel-check");
    let out = tool::output(cmd, ADB_LIMIT)?;
    match accel_check(&out.stdout) {
        Some((0, _)) | None => Ok(()),
        Some((_, said)) => Err(Error::Tool {
            message: if cfg!(windows) {
                "Hardware acceleration is unavailable, so the emulator can't run. Turn on \
                 \"Windows Hypervisor Platform\" in \"Turn Windows features on or off\" and \
                 restart, or install the Android Emulator hypervisor driver (AEHD) from the SDK \
                 Manager's SDK Tools. Virtualization must also be enabled in the firmware \
                 (BIOS/UEFI) settings."
                    .into()
            } else {
                "Hardware acceleration is unavailable, so the emulator can't run. On macOS the \
                 emulator uses the Hypervisor framework, which is missing inside a virtual \
                 machine and on Macs older than macOS 10.10."
                    .into()
            },
            detail: format!("emulator -accel-check: {said}"),
        }),
    }
}

/// What `emulator -accel-check` found: its status, 0 when acceleration is usable, and its
/// sentence about it. The two are printed on the lines between two `accel` markers.
pub(super) fn accel_check(output: &str) -> Option<(u32, String)> {
    let mut lines = output
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "accel:")
        .skip(1);
    let status = lines.next()?.parse().ok()?;
    let said = lines
        .next()
        .filter(|line| *line != "accel")
        .unwrap_or_default();
    Some((status, said.to_string()))
}

/// Whether KVM is there and this user may open it.
fn kvm(device: &Path) -> Result<(), Error> {
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(device);
    let Err(error) = opened else {
        return Ok(());
    };
    let message = match error.kind() {
        std::io::ErrorKind::NotFound => {
            "Hardware acceleration is unavailable: /dev/kvm does not exist, so the emulator \
             can't run. Enable virtualization (Intel VT-x or AMD-V) in the firmware (BIOS/UEFI) \
             settings, then load the module with \"sudo modprobe kvm_intel\" or \"sudo modprobe \
             kvm_amd\"."
        }
        std::io::ErrorKind::PermissionDenied => {
            "Hardware acceleration is unavailable: this user may not open /dev/kvm, so the \
             emulator can't run. Add yourself to the kvm group with \"sudo usermod -aG kvm \
             $USER\", then sign out and back in."
        }
        _ => {
            "Hardware acceleration is unavailable: /dev/kvm could not be opened, so the emulator \
             can't run."
        }
    };
    Err(Error::Tool {
        message: message.into(),
        detail: format!("{}: {error}", device.display()),
    })
}

/// The gRPC port of the emulator whose console is on `console`: 8554 for 5554, as Android Studio
/// numbers them.
fn grpc_port(console: u16) -> u16 {
    console + 3000
}

/// A console port no emulator in `taken` uses and nothing else listens on, with the adb port
/// after it and the gRPC port it goes with.
fn free_port(taken: &[u16]) -> Option<u16> {
    PORTS.step_by(2).find(|port| {
        !taken.contains(port)
            && [*port, port + 1, grpc_port(*port)]
                .iter()
                .all(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
    })
}

/// Where the emulator running `avd` writes what it prints.
fn log_path(avd: &str) -> PathBuf {
    crate::system::dirs()
        .logs
        .join("android")
        .join(format!("{avd}.log"))
}

fn log_tail(avd: &str) -> String {
    let path = log_path(avd);
    match std::fs::read(&path) {
        Ok(bytes) => tail(&String::from_utf8_lossy(&bytes)),
        Err(e) => format!("{}: {e}", path.display()),
    }
}

impl Server {
    /// The Android SDK for the project at `project`, found as [`sdk::locate`] finds it.
    ///
    /// # Errors
    ///
    /// Returns an error when no SDK is found.
    pub fn android_sdk(&self, project: &Path) -> Result<Sdk, Error> {
        sdk::locate(self.store.settings().android_sdk.as_deref(), Some(project))
    }

    /// Starts the virtual device `avd`, its screen shown in the panel rather than a window, and
    /// returns once it is running, before Android has started in it; [`Self::await_boot`] waits
    /// for that.
    ///
    /// # Errors
    ///
    /// Returns an error when the SDK or its emulator is missing, the machine has no hardware
    /// acceleration, the device already runs, or the emulator does not start.
    pub fn start_emulator(&self, project: &Path, avd: &str, boot: Boot) -> Result<Booting, Error> {
        let sdk = self.android_sdk(project)?;
        let emulator = sdk.emulator()?;
        if !self.emulators.accelerated.load(Ordering::Relaxed) {
            acceleration(&sdk)?;
            self.emulators.accelerated.store(true, Ordering::Relaxed);
        }
        let started = self.emulators.started(&self.store);
        let running = devices::list(&sdk, &self.emulators).unwrap_or_default();
        if running
            .iter()
            .any(|device| device.avd.as_deref() == Some(avd))
        {
            return Err(format!("{avd} is already running").into());
        }
        // One started a moment ago holds its port before adb lists it.
        let used: Vec<u16> = running
            .iter()
            .filter_map(|d| emulator_port(&d.serial))
            .chain(started.iter().map(|s| s.port))
            .collect();
        let port = free_port(&used).ok_or(
            "Every emulator port (5554 to 5682) is in use. Close an emulator, then try again.",
        )?;

        let log = log_path(avd);
        if let Some(folder) = log.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let output = std::fs::File::create(&log)?;
        let mut cmd = tool_command(&sdk, emulator);
        // Its screen shows in the panel, streamed over gRPC, rather than in a window of its own.
        cmd.args([
            "-avd",
            avd,
            "-port",
            &port.to_string(),
            "-grpc",
            &grpc_port(port).to_string(),
            "-grpc-use-token",
            "-qt-hide-window",
        ])
        .stdout(output.try_clone()?)
        .stderr(output);
        match boot {
            Boot::Quick => {}
            Boot::Cold => {
                cmd.arg("-no-snapshot-load");
            }
            Boot::Wipe => {
                cmd.arg("-wipe-data");
            }
        }
        if let Some(folder) = emulator.parent() {
            cmd.current_dir(folder);
        }
        detach(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| Error::Tool {
            message: "The emulator didn't start. Reinstall \"Android Emulator\" in Android \
                      Studio's SDK Manager."
                .into(),
            detail: format!("{}: {e}", emulator.display()),
        })?;
        let pid = child.id();
        #[cfg(unix)]
        let token = i32::try_from(pid)
            .ok()
            .and_then(crate::system::processes::find)
            .map(|process| process.started);
        #[cfg(not(unix))]
        let token = None;
        // Reaped when it exits, so it does not linger as a zombie while Patchwork runs.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        log::info!(avd:%, port, pid; "started the emulator for {{avd}} on port {{port}}");
        self.emulators.record(
            &self.store,
            Started {
                avd: avd.to_string(),
                port,
                pid,
                token,
            },
        );
        Ok(Booting {
            avd: avd.to_string(),
            serial: emulator_serial(port),
        })
    }

    /// Waits until Android has started in an emulator [`Self::start_emulator`] started.
    ///
    /// # Errors
    ///
    /// Returns an error when the emulator closes first or Android has not started within five
    /// minutes, with the end of the emulator's log.
    pub fn await_boot(&self, project: &Path, booting: &Booting) -> Result<(), Error> {
        let sdk = self.android_sdk(project)?;
        let adb = sdk.adb()?;
        let port = emulator_port(&booting.serial);
        let deadline = Instant::now() + BOOT_LIMIT;
        while Instant::now() < deadline {
            let ours = self.emulators.started(&self.store);
            if !ours.iter().any(|entry| Some(entry.port) == port) {
                return Err(Error::Tool {
                    message: format!(
                        "{} closed before Android started. Try a cold boot; if it closes again, \
                         wipe its data, or create a device with another system image.",
                        booting.avd
                    ),
                    detail: log_tail(&booting.avd),
                });
            }
            let mut cmd = tool_command(&sdk, adb);
            cmd.args([
                "-s",
                &booting.serial,
                "shell",
                "getprop",
                "sys.boot_completed",
            ]);
            if tool::output(cmd, ADB_LIMIT).is_ok_and(|out| out.stdout.trim() == "1") {
                return Ok(());
            }
            std::thread::sleep(BOOT_POLL);
        }
        Err(Error::Tool {
            message: format!(
                "{} is running, but Android did not start within five minutes. Stop it and try \
                 a cold boot, or wipe its data.",
                booting.avd
            ),
            detail: log_tail(&booting.avd),
        })
    }

    /// Stops the emulator `serial` because the user asked for it: it is told to close through
    /// its console, which saves its snapshot.
    ///
    /// One Patchwork started has its process ended if it has not closed in time. One started
    /// elsewhere is only ever asked, since its process is not Patchwork's to end.
    ///
    /// # Errors
    ///
    /// Returns an error when `serial` is not an emulator, the SDK has no adb, or the emulator is
    /// still running after it was asked to close.
    pub fn stop_emulator(&self, project: &Path, serial: &str) -> Result<(), Error> {
        let port = emulator_port(serial).ok_or_else(|| format!("{serial} is not an emulator"))?;
        let ours = self
            .emulators
            .started(&self.store)
            .into_iter()
            .find(|entry| entry.port == port);
        let sdk = self.android_sdk(project)?;
        let adb = sdk.adb()?;
        let mut cmd = tool_command(&sdk, adb);
        cmd.args(["-s", serial, "emu", "kill"]);
        let asked = tool::output(cmd, ADB_LIMIT);
        let deadline = Instant::now() + STOP_GRACE;
        if let Some(entry) = ours {
            while entry.alive() && Instant::now() < deadline {
                // How often the closing emulator is looked at.
                std::thread::sleep(Duration::from_millis(200));
            }
            if entry.alive() {
                end(&entry);
            }
            self.emulators.prune(&self.store);
            return Ok(());
        }
        let listed = || {
            devices::list(&sdk, &self.emulators)
                .is_ok_and(|devices| devices.iter().any(|device| device.serial == serial))
        };
        while listed() && Instant::now() < deadline {
            // adb is asked again no more often than a closing emulator changes.
            std::thread::sleep(Duration::from_millis(500));
        }
        if listed() {
            return Err(Error::Tool {
                message: format!(
                    "{serial} is still running after it was asked to close. Patchwork did not \
                     start it, so it won't end its process: close the emulator's own window."
                ),
                detail: match asked {
                    Ok(out) => out.all(),
                    Err(e) => e.to_string(),
                },
            });
        }
        Ok(())
    }
}

impl Server {
    /// Deletes the virtual device `avd`, with its apps and data, once it is stopped.
    ///
    /// # Errors
    ///
    /// Returns an error when the device is running, no SDK or `avdmanager` is found, or
    /// `avdmanager` fails.
    pub fn delete_avd(&self, project: &Path, avd: &str) -> Result<(), Error> {
        let sdk = self.android_sdk(project)?;
        self.emulators.prune(&self.store);
        let running = devices::list(&sdk, &self.emulators).unwrap_or_default();
        if running
            .iter()
            .any(|device| device.avd.as_deref() == Some(avd))
        {
            return Err(format!("{avd} is running. Stop it, then delete it.").into());
        }
        super::avd::delete(&sdk, avd)
    }
}

/// Ends an emulator's process, which is checked to still be the one that was started.
fn end(entry: &Started) {
    #[cfg(unix)]
    if let Some(process) = i32::try_from(entry.pid)
        .ok()
        .and_then(crate::system::processes::find)
        .filter(|process| Some(&process.started) == entry.token.as_ref())
    {
        crate::system::processes::signal(&process, libc::SIGTERM);
    }
    #[cfg(not(unix))]
    crate::workspace::signal_process_group(entry.pid, crate::workspace::SIGKILL);
}

/// Sets `cmd` up to outlive Patchwork: in a session of its own on Unix, away from the terminal
/// Patchwork may have been started from, and in a process group of its own on Windows, with no
/// console window.
fn detach(cmd: &mut std::process::Command) {
    cmd.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and touches no memory of this process.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            })
        };
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
}
