//! The tools a chat's agent uses Android devices with, offered on Patchwork's own server when
//! the user allows it and the chat's project is a mobile app: list and start devices, build and
//! run the app, look at the screen, tap, swipe and type, and read the app's log.
//!
//! Every agent Patchwork runs reaches its own server, so one set of tools serves them all. A
//! tool's result is text; a screenshot is written to a file the agent then reads as a picture.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::types::{
    Boot, Device, DeviceState, MobileProject, RunMode, RunTarget, Sdk, Snapshot, Variant,
};
use super::{detect, device, gradle, logcat};
use crate::{Error, Server};

/// What every tool's name starts with on Patchwork's own server.
pub const TOOL_PREFIX: &str = "android_";

/// The tools' names, without [`TOOL_PREFIX`].
const TOOLS: [&str; 9] = [
    "devices",
    "start",
    "run",
    "screenshot",
    "tap",
    "swipe",
    "type",
    "key",
    "logcat",
];

/// The lines of log an agent is given when it does not say how many.
const LOG_LINES: u64 = 200;

/// How long a swipe takes when the agent does not say, in milliseconds.
const SWIPE_MILLIS: u64 = 300;

/// Every tool's full name, for an agent that is told which tools need no asking.
pub(crate) fn tool_names() -> impl Iterator<Item = String> {
    TOOLS.iter().map(|tool| format!("{TOOL_PREFIX}{tool}"))
}

/// The `serial` argument every tool that acts on a device takes.
fn serial_property() -> Value {
    json!({"type": "string", "description": "The device's serial from android_devices. Leave out to use the device picked in the Android panel, or the only one online."})
}

/// One tool as a server lists it, with the `serial` argument added when it acts on a device.
fn tool(name: &str, description: &str, mut properties: Value, required: &[&str]) -> Value {
    if !matches!(name, "devices" | "start") {
        properties["serial"] = serial_property();
    }
    json!({
        "name": format!("{TOOL_PREFIX}{name}"),
        "description": description,
        "inputSchema": {"type": "object", "properties": properties, "required": required},
    })
}

/// The tools as Patchwork's own server lists them, for an app built with `framework`.
fn tools(project: &MobileProject) -> Vec<Value> {
    let framework = project.framework.label();
    let pixel = |what: &str| json!({"type": "integer", "description": format!("{what}, in pixels of the latest android_screenshot.")});
    vec![
        tool(
            "devices",
            "List the Android virtual devices that can be started, and the emulators and phones \
             that are running, with their serials and the build variants of this project's app.",
            json!({}),
            &[],
        ),
        tool(
            "start",
            "Start an Android virtual device by its name from android_devices and wait until \
             Android has booted. Returns its serial.",
            json!({"avd": {"type": "string", "description": "The virtual device's name."}}),
            &["avd"],
        ),
        tool(
            "run",
            &format!(
                "Build this project's {framework} app, install it on a device and open it, then \
                 return the end of the build's output. Call it after changing code, then look \
                 with android_screenshot and read android_logcat to check the change."
            ),
            json!({"variant": {"type": "string", "description": "A build variant from android_devices, such as freeDebug. Leave out for the one picked in the Android panel."}}),
            &[],
        ),
        tool(
            "screenshot",
            "Take a picture of a device's screen. Returns the path of a PNG file to read as an \
             image, and its size, which is what android_tap and android_swipe measure in.",
            json!({}),
            &[],
        ),
        tool(
            "tap",
            "Tap a point on a device's screen.",
            json!({"x": pixel("From the left"), "y": pixel("From the top")}),
            &["x", "y"],
        ),
        tool(
            "swipe",
            "Drag a finger across a device's screen, to scroll or to move something.",
            json!({
                "x1": pixel("Where the drag starts, from the left"),
                "y1": pixel("Where the drag starts, from the top"),
                "x2": pixel("Where the drag ends, from the left"),
                "y2": pixel("Where the drag ends, from the top"),
                "duration_ms": {"type": "integer", "description": "How long the drag takes. 300 when left out; under 100 flings."},
            }),
            &["x1", "y1", "x2", "y2"],
        ),
        tool(
            "type",
            "Type text into the field that has the focus on a device. Tap the field first.",
            json!({"text": {"type": "string"}}),
            &["text"],
        ),
        tool(
            "key",
            "Press a key on a device by its Android key code.",
            json!({"key": {"type": "string", "description": "Such as KEYCODE_BACK, KEYCODE_HOME, KEYCODE_ENTER, KEYCODE_DEL or KEYCODE_APP_SWITCH."}}),
            &["key"],
        ),
        tool(
            "logcat",
            "Read the end of a device's log, kept to this project's app. When the app has \
             crashed and is no longer running, returns the device's latest crashes instead.",
            json!({
                "lines": {"type": "integer", "description": "How many lines, from the end. 200 when left out."},
                "level": {"type": "string", "enum": ["V", "D", "I", "W", "E", "F"], "description": "The least serious level to include. Leave out for all."},
                "all_apps": {"type": "boolean", "description": "True for every app's lines, not only this project's."},
            }),
            &[],
        ),
    ]
}

/// The unsigned number the agent gave as `key`.
fn number(args: &Value, key: &str) -> Result<u32, Error> {
    args[key]
        .as_u64()
        .or_else(|| {
            // Some models write a pixel as `540.0`.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a pixel on a screen, checked to be zero or more"
            )]
            args[key].as_f64().filter(|n| *n >= 0.).map(|n| n as u64)
        })
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| format!("Give {key} as a whole number of pixels.").into())
}

/// One device as a line of the list an agent reads.
fn device_line(device: &Device) -> String {
    let kind = match (&device.avd, &device.model) {
        (Some(avd), _) => format!("emulator running {avd}"),
        (None, Some(model)) => format!("phone, {model}"),
        (None, None) => "phone".to_string(),
    };
    let state = match &device.state {
        DeviceState::Online => "online".to_string(),
        _ => super::problem(device).map_or_else(String::new, |problem| problem.to_string()),
    };
    format!("- {} ({kind}): {state}", device.serial)
}

impl Server {
    /// The mobile app of the chat's project. None when the chat has no project or its project
    /// is no mobile app.
    fn thread_app(&self, thread_id: &str) -> Option<(String, MobileProject)> {
        let project_id = self.store.thread(thread_id)?.project_id?;
        let root = self.project_root(&project_id).ok()?;
        Some((project_id, detect(&PathBuf::from(root))?))
    }

    /// The Android tools as Patchwork's own server lists them for the chat's agent. None unless
    /// the user lets agents use Android devices and the chat's project is a mobile app.
    pub(crate) fn android_tools(&self, thread_id: &str) -> Vec<Value> {
        if !self.store.settings().android_use {
            return Vec::new();
        }
        self.thread_app(thread_id)
            .map(|(_, project)| tools(&project))
            .unwrap_or_default()
    }

    /// The device a tool acts on: the one the agent named, else the one picked in the panel
    /// when it is online, else the only one online.
    fn agent_device(
        &self,
        project_id: &str,
        snapshot: &Snapshot,
        args: &Value,
    ) -> Result<Device, Error> {
        let devices = snapshot.devices.clone()?;
        if let Some(serial) = args["serial"].as_str().filter(|serial| !serial.is_empty()) {
            let device = devices
                .into_iter()
                .find(|device| device.serial == serial)
                .ok_or_else(|| {
                    format!("No device has the serial {serial}. List them with android_devices.")
                })?;
            return match super::problem(&device) {
                Some(problem) => Err(problem),
                None => Ok(device),
            };
        }
        let mut online: Vec<Device> = devices
            .into_iter()
            .filter(|device| device.state == DeviceState::Online)
            .collect();
        let picked = self.android_device(project_id);
        let picked = picked.as_deref();
        if let Some(at) = online.iter().position(|device| {
            Some(device.serial.as_str()) == picked || device.avd.as_deref() == picked
        }) {
            return Ok(online.swap_remove(at));
        }
        match online.len() {
            0 => Err(
                "No device is online. Start a virtual device with android_start, or \
                      list them with android_devices."
                    .into(),
            ),
            1 => Ok(online.swap_remove(0)),
            _ => Err(format!(
                "Several devices are online: {}. Pass one as serial.",
                online
                    .iter()
                    .map(|device| device.serial.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into()),
        }
    }

    /// The list of devices and variants an agent reads.
    fn agent_devices(
        &self,
        project_id: &str,
        project: &MobileProject,
        snapshot: &Snapshot,
    ) -> String {
        let mut lines = vec!["Virtual devices:".to_string()];
        let running = |avd: &str| {
            snapshot.devices.as_ref().is_ok_and(|devices| {
                devices
                    .iter()
                    .any(|device| device.avd.as_deref() == Some(avd))
            })
        };
        lines.extend(snapshot.avds.iter().map(|avd| {
            let state = if running(&avd.name) {
                "running"
            } else {
                "stopped"
            };
            format!("- {} ({state})", avd.name)
        }));
        if snapshot.avds.is_empty() {
            lines.push("- none. The user creates one in the Android panel.".to_string());
        }
        lines.push("Running emulators and connected phones:".to_string());
        match &snapshot.devices {
            Ok(devices) if devices.is_empty() => lines.push("- none".to_string()),
            Ok(devices) => lines.extend(devices.iter().map(device_line)),
            Err(e) => lines.push(format!("- adb could not list them: {e}")),
        }
        let picked = self.android_variant(project_id, project).name();
        let variants: Vec<String> = gradle::variants(project)
            .iter()
            .map(Variant::name)
            .collect();
        lines.push(format!(
            "Build variants of the {} app: {} (picked: {picked})",
            project.framework.label(),
            variants.join(", ")
        ));
        lines.join("\n")
    }

    /// Runs one of the Android tools for the chat's agent and returns its result as text.
    /// `stopped` says the agent's turn was stopped.
    ///
    /// # Errors
    ///
    /// Returns an error the agent is told, when the user has not allowed the tools, the chat's
    /// project is no mobile app, an argument is wrong, or the device or the build fails.
    pub(crate) fn android_tool(
        &self,
        thread_id: &str,
        tool: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        if !self.store.settings().android_use {
            return Err(
                "The user has not let agents use Android devices. They turn it on in \
                        Settings → Permissions."
                    .into(),
            );
        }
        let (project_id, project) = self
            .thread_app(thread_id)
            .ok_or("This chat's project is not an Android app.")?;
        let sdk: Sdk = self.android_sdk(&project.root)?;
        let snapshot = self.android_snapshot(&project.root);
        let device = || self.agent_device(&project_id, &snapshot, args);
        match tool.strip_prefix(TOOL_PREFIX).unwrap_or(tool) {
            "devices" => Ok(self.agent_devices(&project_id, &project, &snapshot)),
            "start" => {
                let avd = args["avd"].as_str().unwrap_or_default();
                if !snapshot.avds.iter().any(|known| known.name == avd) {
                    return Err(format!(
                        "No virtual device is named \"{avd}\". List them with android_devices."
                    )
                    .into());
                }
                let booting = self.start_emulator(&project.root, avd, Boot::Quick)?;
                self.await_boot(&project.root, &booting)?;
                Ok(format!("{avd} is running as {}.", booting.serial))
            }
            "run" => {
                let device = device()?;
                let variant = match args["variant"].as_str().filter(|name| !name.is_empty()) {
                    Some(name) => gradle::variants(&project)
                        .into_iter()
                        .find(|variant| variant.name() == name)
                        .ok_or_else(|| {
                            format!(
                                "The app has no variant {name}. List them with android_devices."
                            )
                        })?,
                    None => self.android_variant(&project_id, &project),
                };
                let target = RunTarget {
                    serial: &device.serial,
                    avd: device.avd.as_deref(),
                    variant: &variant,
                    mode: RunMode::Once,
                };
                let output = self.android_run_once(&project, &target, stopped)?;
                Ok(format!(
                    "Built {} and installed it on {}.\n{output}",
                    variant.name(),
                    device.serial
                ))
            }
            "screenshot" => {
                let device = device()?;
                let png = device::screenshot(&sdk, &device.serial)?;
                let size = image::load_from_memory(&png)
                    .map(|picture| {
                        format!(" It is {}x{} pixels.", picture.width(), picture.height())
                    })
                    .unwrap_or_default();
                let dir = crate::system::dirs().cache.join("screenshots");
                std::fs::create_dir_all(&dir)?;
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|since| since.as_millis())
                    .unwrap_or_default();
                let path = dir.join(format!("android-{stamp}.png"));
                std::fs::write(&path, png)?;
                Ok(format!(
                    "Screenshot of {} written to {}. Read it as an image to see the screen.{size}",
                    device.serial,
                    path.display()
                ))
            }
            "tap" => {
                let (x, y) = (number(args, "x")?, number(args, "y")?);
                let device = device()?;
                device::tap(&sdk, &device.serial, x, y)?;
                Ok(format!("Tapped {x}, {y} on {}.", device.serial))
            }
            "swipe" => {
                let from = (number(args, "x1")?, number(args, "y1")?);
                let to = (number(args, "x2")?, number(args, "y2")?);
                let millis = args["duration_ms"].as_u64().unwrap_or(SWIPE_MILLIS);
                let millis = u32::try_from(millis.clamp(1, 60_000)).unwrap_or(1);
                let device = device()?;
                device::swipe(&sdk, &device.serial, from, to, millis)?;
                Ok(format!("Swiped on {}.", device.serial))
            }
            "type" => {
                let text = args["text"].as_str().unwrap_or_default();
                if text.is_empty() {
                    return Err("Give the text to type.".into());
                }
                let device = device()?;
                device::type_text(&sdk, &device.serial, text)?;
                Ok(format!("Typed the text on {}.", device.serial))
            }
            "key" => {
                let key = args["key"].as_str().unwrap_or_default();
                let device = device()?;
                device::key(&sdk, &device.serial, key)?;
                Ok(format!("Pressed {key} on {}.", device.serial))
            }
            "logcat" => {
                let device = device()?;
                let variant = self.android_variant(&project_id, &project);
                let package = gradle::application_id(&project, &variant);
                let level = args["level"]
                    .as_str()
                    .and_then(|level| level.chars().next())
                    .map(|level| level.to_ascii_uppercase());
                logcat::recent(
                    &sdk,
                    &device.serial,
                    package.as_deref(),
                    args["lines"].as_u64().unwrap_or(LOG_LINES),
                    level.filter(|level| "VDIWEF".contains(*level)),
                    args["all_apps"].as_bool().unwrap_or(false),
                )
            }
            _ => Err(format!("Patchwork has no tool {tool}").into()),
        }
    }
}
