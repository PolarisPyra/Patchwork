//! What kind of computer a machine is, a desktop or a laptop, read from what its system reports
//! about its case and battery.

// Rust guideline compliant 2026-10-09

use super::browse::Machine;

/// What kind of computer a machine is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Desktop,
    Laptop,
    /// A server, a virtual machine, or a machine that could not be reached or did not say.
    Other,
}

/// What kind of computer `machine` is: this computer read here, an SSH host asked over SSH.
///
/// Reaching a host can take a while, so call this off the UI thread.
#[must_use]
pub fn kind(machine: &Machine) -> Kind {
    if !machine.local {
        return super::remote::kind(&machine.name).unwrap_or_else(|e| {
            log::warn!(host:% = machine.name, error:% = e; "could not tell what {{host}} is: {{error}}");
            Kind::Other
        });
    }
    #[cfg(target_os = "linux")]
    {
        let chassis = std::fs::read_to_string("/sys/class/dmi/id/chassis_type").unwrap_or_default();
        let supplies = std::fs::read_dir("/sys/class/power_supply")
            .map(|dir| {
                dir.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        from_linux(&format!("{chassis}\n{supplies}"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            (
                "powershell",
                &["-NoProfile", "-NonInteractive", "-Command", WINDOWS_QUERY],
            )
        } else {
            ("pmset", &["-g", "batt"])
        };
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        super::hide_console(&mut cmd);
        let said = cmd
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        if cfg!(windows) {
            from_chassis(&said)
        } else {
            from_pmset(&said)
        }
    }
}

/// Forgets what the SSH host `machine` said about itself, so [`kind`] asks it again.
pub fn forget(machine: &Machine) {
    if !machine.local {
        super::remote::forget(&machine.name);
    }
}

/// The PowerShell line that prints a Windows machine's chassis types, one per line.
pub(crate) const WINDOWS_QUERY: &str = "(Get-CimInstance Win32_SystemEnclosure).ChassisTypes";

/// The kind a list of SMBIOS chassis types names, from the first one that says.
pub(crate) fn from_chassis(said: &str) -> Kind {
    said.split_whitespace()
        .filter_map(|word| word.parse::<u32>().ok())
        .map(|chassis| match chassis {
            // Desktop, low-profile desktop, pizza box, mini tower, tower, all in one, space-saving,
            // lunch box, mini PC and stick PC.
            3..=7 | 13 | 15 | 16 | 35 | 36 => Kind::Desktop,
            // Portable, laptop, notebook, hand held, sub notebook, tablet, convertible and
            // detachable.
            8..=11 | 14 | 30..=32 => Kind::Laptop,
            _ => Kind::Other,
        })
        .find(|kind| *kind != Kind::Other)
        .unwrap_or(Kind::Other)
}

/// A Linux machine's kind from its chassis type, then the names in `/sys/class/power_supply`:
/// a machine whose case does not say but that has a battery is a laptop.
pub(crate) fn from_linux(said: &str) -> Kind {
    let mut lines = said.lines();
    match from_chassis(lines.next().unwrap_or_default()) {
        Kind::Other if lines.any(|l| l.trim().starts_with("BAT")) => Kind::Laptop,
        kind => kind,
    }
}

/// A Mac's kind from `pmset -g batt`, which names an internal battery only on a laptop.
pub(crate) fn from_pmset(said: &str) -> Kind {
    if said.contains("InternalBattery") {
        Kind::Laptop
    } else if said.trim().is_empty() {
        Kind::Other
    } else {
        Kind::Desktop
    }
}
