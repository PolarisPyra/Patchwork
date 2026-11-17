//! Running the SDK's tools: the command each starts with, what it printed, and a limit on how
//! long it may take, since every one of them can hang.

// Rust guideline compliant 2026-10-09

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::sdk::Os;
use super::types::Sdk;
use crate::Error;

/// The error for a tool the SDK lacks, with the SDK package that has it.
pub(super) fn missing(tool: &str, package: &str, sdk: &Sdk) -> Error {
    Error::Tool {
        message: format!(
            "{tool} is missing from the Android SDK. Install \"{package}\" in Android Studio's \
             SDK Manager (Settings → Languages & Frameworks → Android SDK → SDK Tools)."
        ),
        detail: format!("SDK: {}", sdk.root.display()),
    }
}

/// Each of the SDK's tools, or the error that says which package to install for it.
impl Sdk {
    pub(super) fn adb(&self) -> Result<&Path, Error> {
        let adb = self.adb.as_deref();
        adb.ok_or_else(|| missing("adb", "Android SDK Platform-Tools", self))
    }

    pub(super) fn emulator(&self) -> Result<&Path, Error> {
        let emulator = self.emulator.as_deref();
        emulator.ok_or_else(|| missing("The emulator", "Android Emulator", self))
    }

    pub(super) fn avdmanager(&self) -> Result<&Path, Error> {
        let avdmanager = self.avdmanager.as_deref();
        avdmanager.ok_or_else(|| missing("avdmanager", COMMAND_LINE_TOOLS, self))
    }

    pub(super) fn sdkmanager(&self) -> Result<&Path, Error> {
        let sdkmanager = self.sdkmanager.as_deref();
        sdkmanager.ok_or_else(|| missing("sdkmanager", COMMAND_LINE_TOOLS, self))
    }
}

/// The SDK package with `avdmanager` and `sdkmanager`.
const COMMAND_LINE_TOOLS: &str = "Android SDK Command-line Tools (latest)";

/// One error for each tool the SDK lacks, the ones every action needs first.
#[must_use]
pub fn missing_tools(sdk: &Sdk) -> Vec<Error> {
    [
        (sdk.emulator.is_none(), "The emulator", "Android Emulator"),
        (sdk.adb.is_none(), "adb", "Android SDK Platform-Tools"),
        (
            sdk.avdmanager.is_none() || sdk.sdkmanager.is_none(),
            "avdmanager and sdkmanager, which create devices,",
            COMMAND_LINE_TOOLS,
        ),
    ]
    .into_iter()
    .filter(|(lacking, _, _)| *lacking)
    .map(|(_, tool, package)| missing(tool, package, sdk))
    .collect()
}

/// Where Android Studio keeps the Java it comes with, which runs the SDK's Java tools when the
/// machine has no other.
fn studio_java(os: Os, home: Option<&Path>, var: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    match os {
        Os::Mac => ["/Applications", "~/Applications"]
            .iter()
            .filter_map(|apps| match apps.strip_prefix("~/") {
                Some(rest) => Some(home?.join(rest)),
                None => Some(PathBuf::from(apps)),
            })
            .map(|apps| apps.join("Android Studio.app/Contents/jbr/Contents/Home"))
            .collect(),
        Os::Windows => var("ProgramFiles")
            .map(|programs| PathBuf::from(programs).join(r"Android\Android Studio\jbr"))
            .into_iter()
            .collect(),
        Os::Linux => [
            "/opt/android-studio",
            "/usr/local/android-studio",
            "/snap/android-studio/current",
        ]
        .iter()
        .map(PathBuf::from)
        .chain(home.map(|home| home.join("android-studio")))
        .map(|studio| studio.join("jbr"))
        .collect(),
    }
}

/// A command for an SDK tool, run as a terminal would run it.
///
/// The AppImage's own library paths are left out and the user's shell environment is put in, so
/// the tool finds Java, the display and the user's Android settings whether Patchwork was started
/// from a shell, a desktop launcher or the Finder. The tool is told which SDK it belongs to.
pub(super) fn tool_command(sdk: &Sdk, program: &Path) -> Command {
    let mut cmd = crate::system::host_command(program);
    cmd.envs(crate::system::login_env().iter().map(|(k, v)| (k, v)))
        .env("ANDROID_HOME", &sdk.root)
        .env("ANDROID_SDK_ROOT", &sdk.root)
        .stdin(Stdio::null());
    if let Some(path) = crate::system::search_path() {
        cmd.env("PATH", path);
    }
    let has_java = crate::system::login_var("JAVA_HOME").is_some()
        || crate::system::find_program("java").is_some();
    if !has_java {
        let home = crate::system::fs::home_dir();
        let var = |name: &str| std::env::var(name).ok();
        if let Some(java) = studio_java(Os::current(), home.as_deref(), &var)
            .into_iter()
            .find(|java| java.is_dir())
        {
            cmd.env("JAVA_HOME", java);
        }
    }
    cmd
}

/// What a tool printed, and whether it exited zero.
#[derive(Debug)]
pub(super) struct Output {
    pub(super) ok: bool,
    pub(super) stdout: String,
    pub(super) stderr: String,
}

impl Output {
    /// Everything it printed, for the detail of an error.
    pub(super) fn all(&self) -> String {
        [self.stdout.trim(), self.stderr.trim()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// How long a pipe is read after its program exited, for output still on its way. A program the
/// tool left running, as adb leaves its server, can hold the pipe open for good.
const DRAIN: Duration = Duration::from_millis(300);

/// Runs `cmd` to its end and returns what it printed, stopping it after `limit`.
///
/// # Errors
///
/// Returns an error when the program does not start, or is still running at the limit.
pub(super) fn output(cmd: Command, limit: Duration) -> Result<Output, Error> {
    let raw = output_bytes(cmd, limit, &|| false)?;
    Ok(Output {
        ok: raw.ok,
        stdout: String::from_utf8_lossy(&raw.stdout).into_owned(),
        stderr: raw.stderr,
    })
}

/// What a tool printed as the bytes it wrote, for a picture or an archive.
#[derive(Debug)]
pub(super) struct Bytes {
    pub(super) ok: bool,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: String,
}

/// Runs `cmd` to its end as [`output`] does, and also stops it once `stopped` says the person
/// who asked for it gave up.
///
/// # Errors
///
/// Returns an error when the program does not start, is still running at the limit, or was
/// stopped.
pub(super) fn output_bytes(
    mut cmd: Command,
    limit: Duration,
    stopped: &dyn Fn() -> bool,
) -> Result<Bytes, Error> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{name} didn't start: {e}"))?;
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None if stopped() => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} was stopped before it finished.").into());
            }
            // Short, so a quick tool returns at once.
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Tool {
                    message: format!(
                        "{name} did not finish within {} seconds, so it was stopped. Try again.",
                        limit.as_secs()
                    ),
                    detail: tail(&format!("{}\n{}", text(&stdout), text(&stderr))),
                });
            }
        }
    };
    let drained = Instant::now() + DRAIN;
    while Arc::strong_count(&stdout) + Arc::strong_count(&stderr) > 2 && Instant::now() < drained {
        // Short, so output already read is returned at once.
        std::thread::sleep(Duration::from_millis(5));
    }
    let stdout = std::mem::take(&mut *stdout.lock().unwrap_or_else(PoisonError::into_inner));
    Ok(Bytes {
        ok: status.success(),
        stdout,
        stderr: text(&stderr),
    })
}

type Collected = Arc<Mutex<Vec<u8>>>;

/// Reads a pipe to its end on a thread of its own. The thread holds the second reference to what
/// it read until the pipe closes.
fn collect<R: Read + Send + 'static>(pipe: Option<R>) -> Collected {
    let collected = Collected::default();
    if let Some(mut pipe) = pipe {
        let into = Arc::clone(&collected);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8 << 10];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                into.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..n]);
            }
        });
    }
    collected
}

fn text(collected: &Collected) -> String {
    String::from_utf8_lossy(&collected.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
}

/// The end of a tool's output, which is where it says what went wrong.
pub(super) fn tail(output: &str) -> String {
    /// Enough lines to hold a Java stack trace's cause without filling the panel.
    const LINES: usize = 60;
    let lines: Vec<&str> = output.lines().collect();
    lines[lines.len().saturating_sub(LINES)..].join("\n")
}
