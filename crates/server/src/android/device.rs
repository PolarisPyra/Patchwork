//! Acting on one device through adb: a picture of its screen, taps and typing, installing an
//! APK, and the controls of an emulator's console.
//!
//! Everything here works the same on an emulator and a phone, except the console, which only an
//! emulator has: its rotation, location, network speed and saved states.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::time::Duration;

use super::devices::{ADB_LIMIT, emulator_port};
use super::tool::{self, Output, tool_command};
use super::types::{NetworkSpeed, Sdk, StateAction};
use crate::{Error, Server};

/// How long an install gets: a large APK over a slow cable takes minutes.
const INSTALL_LIMIT: Duration = Duration::from_secs(5 * 60);

/// How long saving or loading an emulator's state gets, which writes or reads its whole memory.
const SNAPSHOT_LIMIT: Duration = Duration::from_secs(2 * 60);

/// The key code Android gives the R key, which a React Native app reloads on when sent twice.
const KEYCODE_R: &str = "46";

/// Runs adb on the device `serial` with `args`, and returns what it printed.
///
/// # Errors
///
/// Returns an error when the SDK has no adb, or adb fails or does not finish within `limit`.
pub(super) fn adb(
    sdk: &Sdk,
    serial: &str,
    args: &[&str],
    limit: Duration,
) -> Result<Output, Error> {
    let mut cmd = tool_command(sdk, sdk.adb()?);
    cmd.args(["-s", serial]).args(args);
    let out = tool::output(cmd, limit)?;
    if out.ok {
        return Ok(out);
    }
    Err(Error::Tool {
        message: format!(
            "adb {} failed on {serial}: {}",
            args.first().copied().unwrap_or_default(),
            last_line(&out.all())
        ),
        detail: out.all(),
    })
}

fn last_line(text: &str) -> &str {
    text.lines()
        .next_back()
        .unwrap_or("it printed nothing")
        .trim()
}

/// The device's screen as a PNG file's bytes, at the device's own size.
///
/// # Errors
///
/// Returns an error when adb fails, or the device sends no picture, as one whose screen is
/// locked against capture does.
pub(super) fn screenshot(sdk: &Sdk, serial: &str) -> Result<Vec<u8>, Error> {
    let mut cmd = tool_command(sdk, sdk.adb()?);
    cmd.args(["-s", serial, "exec-out", "screencap", "-p"]);
    let out = tool::output_bytes(cmd, ADB_LIMIT, &|| false)?;
    if !out.ok || !out.stdout.starts_with(b"\x89PNG") {
        return Err(Error::Tool {
            message: format!(
                "{serial} sent no picture of its screen. Unlock the device, then try again."
            ),
            detail: out.stderr,
        });
    }
    Ok(out.stdout)
}

/// The device's screen as it is now, in the BGRA order the app draws, with its width and
/// height. Faster than [`screenshot`], which the device compresses first.
///
/// # Errors
///
/// Returns an error when adb fails or the picture is not the size its header says.
pub(super) fn raw_screen(sdk: &Sdk, serial: &str) -> Result<(u32, u32, Vec<u8>), Error> {
    let mut cmd = tool_command(sdk, sdk.adb()?);
    cmd.args(["-s", serial, "exec-out", "screencap"]);
    let out = tool::output_bytes(cmd, ADB_LIMIT, &|| false)?;
    let bytes = out.stdout;
    let word = |at: usize| {
        let word: Option<[u8; 4]> = bytes.get(at..at + 4).and_then(|word| word.try_into().ok());
        word.map(u32::from_le_bytes)
    };
    let size = word(0).zip(word(4)).filter(|(width, height)| {
        // A screen is a few thousand pixels across; more is not a picture's header.
        (1..=16_384).contains(width) && (1..=16_384).contains(height)
    });
    let pixels = size.and_then(|(width, height)| {
        let len = (width * height * 4) as usize;
        // The header is the size and the pixel format, and since Android 9 the color space too.
        let header = bytes.len().checked_sub(len)?;
        matches!(header, 12 | 16).then_some(header)
    });
    let (Some((width, height)), Some(header)) = (size, pixels) else {
        return Err(Error::Tool {
            message: format!(
                "{serial} sent no picture of its screen. Unlock the device, then try again."
            ),
            detail: out.stderr,
        });
    };
    let mut bgra = bytes;
    bgra.drain(..header);
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Ok((width, height, bgra))
}

/// The device's screen size in pixels, as `wm size` gives it: the size an app sees when the
/// user set another, else the size the device was made with.
pub(super) fn screen_size(sdk: &Sdk, serial: &str) -> Option<(u32, u32)> {
    let out = adb(sdk, serial, &["shell", "wm", "size"], ADB_LIMIT).ok()?;
    out.stdout.lines().rev().find_map(|line| {
        let (width, height) = line.rsplit(' ').next()?.split_once('x')?;
        Some((width.parse().ok()?, height.parse().ok()?))
    })
}

/// Taps the point `x`, `y`, in the pixels of a [`screenshot`].
pub(super) fn tap(sdk: &Sdk, serial: &str, x: u32, y: u32) -> Result<(), Error> {
    let (x, y) = (x.to_string(), y.to_string());
    adb(sdk, serial, &["shell", "input", "tap", &x, &y], ADB_LIMIT).map(|_| ())
}

/// Drags a finger from one point to another over `millis` milliseconds, in the pixels of a
/// [`screenshot`].
pub(super) fn swipe(
    sdk: &Sdk,
    serial: &str,
    from: (u32, u32),
    to: (u32, u32),
    millis: u32,
) -> Result<(), Error> {
    let numbers = [from.0, from.1, to.0, to.1, millis].map(|n| n.to_string());
    let mut args = vec!["shell", "input", "swipe"];
    args.extend(numbers.iter().map(String::as_str));
    adb(sdk, serial, &args, ADB_LIMIT).map(|_| ())
}

/// Types `text` into whatever has the focus on the device.
pub(super) fn type_text(sdk: &Sdk, serial: &str, text: &str) -> Result<(), Error> {
    // `input text` reads a space as `%s`, and the device's shell reads the rest.
    let quoted = format!("'{}'", text.replace(' ', "%s").replace('\'', "'\\''"));
    adb(sdk, serial, &["shell", "input", "text", &quoted], ADB_LIMIT).map(|_| ())
}

/// Presses the key `code`, an Android key code's name or number such as `KEYCODE_BACK`.
///
/// # Errors
///
/// Returns an error when `code` is not a key code's name or number, or adb fails.
pub(super) fn key(sdk: &Sdk, serial: &str, code: &str) -> Result<(), Error> {
    if code.is_empty() || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "\"{code}\" is not an Android key code. Use a name such as KEYCODE_BACK or \
             KEYCODE_ENTER."
        )
        .into());
    }
    // The device takes a name in lowercase without a word, and presses nothing.
    let code = code.to_ascii_uppercase();
    adb(
        sdk,
        serial,
        &["shell", "input", "keyevent", &code],
        ADB_LIMIT,
    )
    .map(|_| ())
}

/// Sends an emulator's console `command`, and returns what it answered before its `OK`.
///
/// # Errors
///
/// Returns an error when `serial` is not an emulator, adb fails, or the console refuses.
fn console(sdk: &Sdk, serial: &str, command: &[&str], limit: Duration) -> Result<String, Error> {
    if emulator_port(serial).is_none() {
        return Err(format!("{serial} is a phone, and only an emulator has this control.").into());
    }
    let mut args = vec!["emu"];
    args.extend(command);
    let out = adb(sdk, serial, &args, limit)?;
    if let Some(refused) = out.stdout.lines().find_map(|line| line.strip_prefix("KO:")) {
        return Err(Error::Tool {
            message: format!("The emulator refused: {}", refused.trim()),
            detail: out.all(),
        });
    }
    Ok(out
        .stdout
        .lines()
        .filter(|line| line.trim() != "OK")
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The names in the console's list of saved states, whose rows read
/// `--        before_login            68M 2026-11-13 15:36:30   00:00:27.908`.
pub(super) fn snapshot_names(listing: &str) -> Vec<String> {
    listing
        .lines()
        .skip_while(|line| !line.starts_with("ID"))
        .skip(1)
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(str::to_string)
        .collect()
}

/// `name` when it can be given to the console as a saved state's name.
fn snapshot_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if plain {
        Ok(name)
    } else {
        Err("Name the state with letters, digits, dots, dashes and underscores only.".into())
    }
}

impl Server {
    /// Installs the APK at `apk` on the device `serial`, over the same app when it is there.
    ///
    /// # Errors
    ///
    /// Returns an error when the SDK has no adb, or the device refuses the APK.
    pub fn android_install(&self, project: &Path, serial: &str, apk: &Path) -> Result<(), Error> {
        let sdk = self.android_sdk(project)?;
        let path = apk.to_string_lossy();
        adb(&sdk, serial, &["install", "-r", &path], INSTALL_LIMIT).map(|_| ())
    }

    /// Reloads the React Native app in front on the device `serial`.
    ///
    /// # Errors
    ///
    /// Returns an error when the SDK has no adb, or adb fails.
    pub fn android_reload(&self, project: &Path, serial: &str) -> Result<(), Error> {
        let sdk = self.android_sdk(project)?;
        let keys = ["shell", "input", "keyevent", KEYCODE_R, KEYCODE_R];
        adb(&sdk, serial, &keys, ADB_LIMIT).map(|_| ())
    }

    /// Turns the emulator `serial` a quarter turn.
    ///
    /// # Errors
    ///
    /// Returns an error when `serial` is not an emulator, or its console does not answer.
    pub fn android_rotate(&self, project: &Path, serial: &str) -> Result<(), Error> {
        let sdk = self.android_sdk(project)?;
        console(&sdk, serial, &["rotate"], ADB_LIMIT).map(|_| ())
    }

    /// Puts the emulator `serial` at a place on Earth, in degrees.
    ///
    /// # Errors
    ///
    /// Returns an error when the place is not on Earth, `serial` is not an emulator, or its
    /// console does not answer.
    pub fn android_set_location(
        &self,
        project: &Path,
        serial: &str,
        latitude: f64,
        longitude: f64,
    ) -> Result<(), Error> {
        if !(-90. ..=90.).contains(&latitude) || !(-180. ..=180.).contains(&longitude) {
            return Err("Enter a latitude from -90 to 90 and a longitude from -180 to 180.".into());
        }
        let sdk = self.android_sdk(project)?;
        // The console takes the longitude first.
        let (longitude, latitude) = (longitude.to_string(), latitude.to_string());
        console(
            &sdk,
            serial,
            &["geo", "fix", &longitude, &latitude],
            ADB_LIMIT,
        )
        .map(|_| ())
    }

    /// Limits the emulator `serial` to the speed of a mobile network.
    ///
    /// # Errors
    ///
    /// Returns an error when `serial` is not an emulator, or its console does not answer.
    pub fn android_set_network(
        &self,
        project: &Path,
        serial: &str,
        speed: NetworkSpeed,
    ) -> Result<(), Error> {
        let sdk = self.android_sdk(project)?;
        console(
            &sdk,
            serial,
            &["network", "speed", speed.console_name()],
            ADB_LIMIT,
        )
        .map(|_| ())
    }

    /// The names of the states saved for the emulator `serial`.
    ///
    /// # Errors
    ///
    /// Returns an error when `serial` is not an emulator, or its console does not answer.
    pub fn android_saved_states(&self, project: &Path, serial: &str) -> Result<Vec<String>, Error> {
        let sdk = self.android_sdk(project)?;
        let listing = console(&sdk, serial, &["avd", "snapshot", "list"], ADB_LIMIT)?;
        Ok(snapshot_names(&listing))
    }

    /// Saves the emulator's state as `name`, loads the state saved as `name`, or deletes it.
    ///
    /// # Errors
    ///
    /// Returns an error when the name holds more than letters, digits, dots, dashes and
    /// underscores, `serial` is not an emulator, or its console refuses.
    pub fn android_saved_state(
        &self,
        project: &Path,
        serial: &str,
        action: StateAction,
        name: &str,
    ) -> Result<(), Error> {
        let name = snapshot_name(name)?;
        let sdk = self.android_sdk(project)?;
        let verb = match action {
            StateAction::Save => "save",
            StateAction::Load => "load",
            StateAction::Delete => "delete",
        };
        console(
            &sdk,
            serial,
            &["avd", "snapshot", verb, name],
            SNAPSHOT_LIMIT,
        )
        .map(|_| ())
    }
}
