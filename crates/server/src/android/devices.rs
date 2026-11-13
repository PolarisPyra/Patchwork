//! The running emulators and connected devices, as `adb devices -l` lists them.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use super::emulator::Emulators;
use super::tool::{self, tool_command};
use super::types::{Device, DeviceState, Sdk};
use crate::Error;

/// How long an adb command gets. The first one starts adb's server, which takes a few seconds;
/// a server that is stuck answers nothing at all.
pub(super) const ADB_LIMIT: Duration = Duration::from_secs(20);

/// The serial adb gives the emulator whose console listens on a port, such as `emulator-5554`.
const EMULATOR_PREFIX: &str = "emulator-";

/// The console port in an emulator's serial.
pub(super) fn emulator_port(serial: &str) -> Option<u16> {
    serial.strip_prefix(EMULATOR_PREFIX)?.parse().ok()
}

pub(super) fn emulator_serial(port: u16) -> String {
    format!("{EMULATOR_PREFIX}{port}")
}

/// The devices in the output of `adb devices -l`, whose lines read
/// `emulator-5554  device product:sdk_gphone64 model:sdk_gphone64_x86_64 transport_id:1`.
pub(super) fn parse(listing: &str) -> Vec<Device> {
    listing
        .lines()
        .skip_while(|line| !line.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?.to_string();
            let rest: Vec<&str> = parts.collect();
            let state = match *rest.first()? {
                "device" => DeviceState::Online,
                "offline" | "authorizing" | "connecting" => DeviceState::Offline,
                "unauthorized" => DeviceState::Unauthorized,
                // `no permissions (user in plugdev group; are your udev rules wrong?)`
                "no" => DeviceState::NoPermissions,
                other => DeviceState::Other(other.to_string()),
            };
            let model = rest
                .iter()
                .find_map(|part| part.strip_prefix("model:"))
                .map(|model| model.replace('_', " "));
            Some(Device {
                emulator: emulator_port(&serial).is_some(),
                serial,
                state,
                avd: None,
                model,
                ours: false,
            })
        })
        .collect()
}

/// The error for a device that is connected and can't be used yet, with what to do about it.
/// None for one that is ready.
#[must_use]
pub fn problem(device: &Device) -> Option<Error> {
    let message = match &device.state {
        DeviceState::Online => return None,
        DeviceState::Unauthorized => format!(
            "{} has not allowed this computer to debug it. Unlock the device and tap Allow on \
             the \"Allow USB debugging?\" prompt. If no prompt shows, turn USB debugging off and \
             on in Developer options.",
            device.serial
        ),
        DeviceState::NoPermissions => format!(
            "This user can't open {} over USB. Install your distribution's Android udev rules \
             (the android-udev or android-sdk-platform-tools-common package), add yourself to \
             the plugdev or adbusers group, then plug the device in again.",
            device.serial
        ),
        DeviceState::Offline => format!(
            "{} is connected and not answering yet. An emulator answers once Android has \
             started; a phone, once it is unlocked and plugged in again.",
            device.serial
        ),
        DeviceState::Other(state) => {
            format!(
                "{} is in the state \"{state}\", so adb can't use it yet.",
                device.serial
            )
        }
    };
    Some(message.into())
}

/// The devices adb lists, each emulator with the virtual device it runs.
///
/// # Errors
///
/// Returns an error when the SDK has no adb, or adb fails.
pub(super) fn list(sdk: &Sdk, emulators: &Emulators) -> Result<Vec<Device>, Error> {
    let adb = sdk.adb()?;
    let mut cmd = tool_command(sdk, adb);
    cmd.args(["devices", "-l"]);
    let out = tool::output(cmd, ADB_LIMIT)?;
    if !out.ok {
        return Err(Error::Tool {
            message: "adb could not list the devices. Run \"adb kill-server\" in a terminal, \
                      then open this panel again."
                .into(),
            detail: out.all(),
        });
    }
    let mut devices = parse(&out.stdout);
    let serials: Vec<&str> = devices.iter().map(|d| d.serial.as_str()).collect();
    emulators.keep_names(&serials);
    for device in &mut devices {
        let Some(port) = emulator_port(&device.serial) else {
            continue;
        };
        let started = emulators.ours(port);
        device.ours = started.is_some();
        device.avd = started.or_else(|| {
            emulators.name(&device.serial, || {
                // The emulator's console answers with the name, then `OK`.
                let mut cmd = tool_command(sdk, adb);
                cmd.args(["-s", &device.serial, "emu", "avd", "name"]);
                let out = tool::output(cmd, ADB_LIMIT).ok()?;
                let name = out.stdout.lines().next()?.trim();
                (out.ok && !name.is_empty() && name != "OK").then(|| name.to_string())
            })
        });
    }
    Ok(devices)
}
