//! Virtual devices: reading the ones defined, the hardware profiles a new one can take, and
//! creating one.
//!
//! The definitions are read from their own files rather than from `avdmanager list avd`, which
//! starts Java and takes seconds: `<name>.ini` in the AVD folder points at the device's folder,
//! whose `config.ini` describes it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::tool::{self, tool_command};
use super::types::{Avd, HardwareProfile, Sdk, SystemImage, variant};
use crate::Error;

/// How long `avdmanager` gets. It starts Java and reads every installed package first, which
/// takes several seconds on a cold disk.
const AVDMANAGER_LIMIT: Duration = Duration::from_secs(90);

/// The system image tags a phone or tablet runs. Wear, TV, Automotive and XR images need
/// hardware profiles and windows of their own.
const PHONE_TAGS: [&str; 3] = ["default", "google_apis", "google_apis_playstore"];

/// The Android version each API level is, since Android 4.1. A level missing here, such as a
/// preview's, is shown by its number alone.
const VERSIONS: [(u32, &str); 22] = [
    (16, "4.1"),
    (17, "4.2"),
    (18, "4.3"),
    (19, "4.4"),
    (21, "5.0"),
    (22, "5.1"),
    (23, "6.0"),
    (24, "7.0"),
    (25, "7.1"),
    (26, "8.0"),
    (27, "8.1"),
    (28, "9"),
    (29, "10"),
    (30, "11"),
    (31, "12"),
    (32, "12L"),
    (33, "13"),
    (34, "14"),
    (35, "15"),
    (36, "16"),
    (37, "17"),
    (38, "18"),
];

/// The Android version of an API level as the SDK writes it: `14` for `34`, `17` for `37.0`.
pub(super) fn android_version(api: &str) -> Option<String> {
    let level = api_number(api)?;
    VERSIONS
        .iter()
        .find(|(known, _)| *known == level)
        .map(|(_, version)| (*version).to_string())
}

/// The whole number an API level starts with, which orders levels: 37 for `37.0`.
pub(super) fn api_number(api: &str) -> Option<u32> {
    let digits: String = api.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Whether images and profiles with this tag are for phones and tablets.
pub(super) fn phone_tag(tag: &str) -> bool {
    PHONE_TAGS.iter().any(|known| tag.starts_with(known))
}

/// The `key=value` lines of an `.ini` file.
fn ini(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

/// The virtual devices defined in `avd_home`, in name order.
pub(super) fn list(avd_home: &Path) -> Vec<Avd> {
    let Ok(entries) = std::fs::read_dir(avd_home) else {
        return Vec::new();
    };
    let mut found: Vec<Avd> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ini"))
        .filter_map(|path| read(avd_home, &path))
        .collect();
    found.sort_by_key(|avd| avd.display_name.to_lowercase());
    found
}

/// The device `pointer`, its `<name>.ini`, defines. None when its folder is gone.
fn read(avd_home: &Path, pointer: &Path) -> Option<Avd> {
    let name = pointer.file_stem()?.to_string_lossy().into_owned();
    let head = ini(&std::fs::read_to_string(pointer).ok()?);
    // The folder moves with the user's home when `path` is stale, which `path.rel` survives.
    let folder = head
        .get("path")
        .map(PathBuf::from)
        .filter(|folder| folder.is_dir())
        .or_else(|| {
            let relative = avd_home.parent()?.join(head.get("path.rel")?);
            relative.is_dir().then_some(relative)
        })
        .unwrap_or_else(|| avd_home.join(format!("{name}.avd")));
    let config = ini(&std::fs::read_to_string(folder.join("config.ini")).ok()?);
    // `system-images/android-34/google_apis/x86_64/`
    let image: Vec<&str> = config
        .get("image.sysdir.1")
        .map(|dir| {
            dir.split(['/', '\\'])
                .filter(|part| !part.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let api = head
        .get("target")
        .or_else(|| config.get("target"))
        .map(String::as_str)
        .or_else(|| image.get(1).copied())
        .map(|target| target.trim_start_matches("android-").to_string())
        .unwrap_or_default();
    let tag = config
        .get("tag.id")
        .map(String::as_str)
        .or_else(|| image.get(2).copied())
        .unwrap_or("default");
    Some(Avd {
        display_name: config
            .get("avd.ini.displayname")
            .cloned()
            .unwrap_or_else(|| name.replace('_', " ")),
        android_version: android_version(&api),
        api,
        abi: config
            .get("abi.type")
            .cloned()
            .or_else(|| image.get(3).map(ToString::to_string))
            .unwrap_or_default(),
        variant: variant(tag).to_string(),
        name,
    })
}

/// The phone and tablet profiles of `avdmanager list device`, whose entries read
/// `id: 30 or "pixel_7"`, then `Name:`, `OEM :` and `Tag :` lines.
pub(super) fn parse_profiles(listing: &str) -> Vec<HardwareProfile> {
    let mut profiles = Vec::new();
    let mut current: Option<(HardwareProfile, bool)> = None;
    let mut finish = |entry: Option<(HardwareProfile, bool)>| {
        if let Some((profile, true)) = entry {
            profiles.push(profile);
        }
    };
    for line in listing.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("id:") {
            finish(current.take());
            let id = rest.split('"').nth(1).unwrap_or_default().to_string();
            let profile = HardwareProfile {
                name: id.clone(),
                id,
                oem: String::new(),
            };
            current = Some((profile, true));
        } else if let (Some((profile, phone)), Some((key, value))) =
            (current.as_mut(), line.split_once(':'))
        {
            let value = value.trim();
            match key.trim() {
                "Name" => profile.name = value.to_string(),
                "OEM" => profile.oem = value.to_string(),
                "Tag" => *phone = phone_tag(value),
                _ => {}
            }
        }
    }
    finish(current.take());
    profiles.retain(|profile| !profile.id.is_empty());
    profiles
}

/// The hardware profiles a new phone or tablet can take.
///
/// # Errors
///
/// Returns an error when the SDK has no `avdmanager`, or it fails.
pub fn profiles(sdk: &Sdk) -> Result<Vec<HardwareProfile>, Error> {
    let avdmanager = avdmanager(sdk)?;
    let out = tool::output(
        {
            let mut cmd = tool_command(sdk, avdmanager);
            cmd.args(["list", "device"]);
            cmd
        },
        AVDMANAGER_LIMIT,
    )?;
    let found = parse_profiles(&out.stdout);
    if found.is_empty() {
        return Err(Error::Tool {
            message: "avdmanager listed no hardware profiles. It needs Java 17 or newer: install \
                      Android Studio, or set JAVA_HOME, then try again."
                .into(),
            detail: out.all(),
        });
    }
    Ok(found)
}

fn avdmanager(sdk: &Sdk) -> Result<&Path, Error> {
    sdk.avdmanager
        .as_deref()
        .ok_or_else(|| tool::missing("avdmanager", "Android SDK Command-line Tools (latest)", sdk))
}

/// A name for a new device with this profile and API level that no device in `existing` has:
/// `Pixel_7_API_34`, then `Pixel_7_API_34_2`.
#[must_use]
pub fn suggest_name(profile: &str, api: &str, existing: &[Avd]) -> String {
    let base: String = format!("{profile}_API_{api}")
        .chars()
        .map(|c| if valid_name_char(c) { c } else { '_' })
        .collect();
    let taken = |name: &str| {
        existing
            .iter()
            .any(|avd| avd.name.eq_ignore_ascii_case(name))
    };
    if !taken(&base) {
        return base;
    }
    // More devices than this share no name.
    (2..1000)
        .map(|n| format!("{base}_{n}"))
        .find(|name| !taken(name))
        .unwrap_or(base)
}

/// The characters `avdmanager` takes in a device's name.
fn valid_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Creates the virtual device `name` from an installed system image and a hardware profile.
///
/// # Errors
///
/// Returns an error when the name is empty, has characters `avdmanager` refuses or is taken,
/// when the SDK has no `avdmanager`, or when it fails, with what it printed.
pub fn create(sdk: &Sdk, name: &str, image: &SystemImage, profile: &str) -> Result<(), Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the device a name".into());
    }
    if !name.chars().all(valid_name_char) {
        return Err(
            "A device's name can have letters, digits, dots, dashes and underscores".into(),
        );
    }
    if list(&sdk.avd_home)
        .iter()
        .any(|avd| avd.name.eq_ignore_ascii_case(name))
    {
        return Err(format!("A device named {name} already exists. Pick another name.").into());
    }
    let mut cmd = tool_command(sdk, avdmanager(sdk)?);
    cmd.args(["create", "avd", "--name", name, "--package"])
        .arg(image.avd_package())
        .args(["--device", profile]);
    let out = tool::output(cmd, AVDMANAGER_LIMIT)?;
    if out.ok {
        return Ok(());
    }
    Err(Error::Tool {
        message: format!(
            "The device {name} could not be created. Check that the system image finished \
             installing, then try again."
        ),
        detail: out.all(),
    })
}

/// Deletes the virtual device `name`, with its apps and data.
///
/// # Errors
///
/// Returns an error when the SDK has no `avdmanager`, or it fails, with what it printed.
pub(super) fn delete(sdk: &Sdk, name: &str) -> Result<(), Error> {
    let mut cmd = tool_command(sdk, avdmanager(sdk)?);
    cmd.args(["delete", "avd", "--name", name]);
    let out = tool::output(cmd, AVDMANAGER_LIMIT)?;
    if out.ok {
        return Ok(());
    }
    Err(Error::Tool {
        message: format!(
            "The device {name} could not be deleted. Stop it if it is running, then try again."
        ),
        detail: out.all(),
    })
}
