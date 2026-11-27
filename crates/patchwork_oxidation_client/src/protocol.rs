//! Wire protocol shared between the Oxidation host and the managed application.
//!
//! Commands are newline-delimited UTF-8 frames written to the child's standard input:
//! - `quit`: save state to `OXIDATION_STATE` and exit.
//! - `restart`: save state, optionally hand over file descriptors on `OXIDATION_FDS`, and exit.
//! - `reload <path>`: reload the asset/file at `<path>` without restarting.
//! - `error <json>`: display the latest build errors (ANSI-stripped JSON string).
//! - `fixed`: clear previously reported build errors.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

/// Environment variable naming the JSON file used to pass state across restarts.
pub const STATE_VAR: &str = "OXIDATION_STATE";

/// Environment variable naming the inherited Unix socket FD used for descriptor handover.
pub const FDS_VAR: &str = "OXIDATION_FDS";

/// A control command sent from the Oxidation host to the managed application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Save state and exit cleanly.
    Quit,
    /// Save state, hand over live file descriptors, and exit for immediate relaunch.
    Restart,
    /// Re-read the changed file in place without restarting.
    Reload(PathBuf),
    /// The latest build failed with these plain-text compiler diagnostics.
    Errors(String),
    /// A subsequent build succeeded after a failure.
    Fixed,
}

/// Encodes `command` into a single newline-terminated wire frame.
#[must_use]
pub fn encode_command(command: &Command) -> String {
    match command {
        Command::Quit => "quit\n".to_owned(),
        Command::Restart => "restart\n".to_owned(),
        Command::Reload(path) => format!("reload {}\n", path.display()),
        Command::Errors(errors) => {
            let text = serde_json::Value::String(strip_ansi(errors)).to_string();
            format!("error {text}\n")
        }
        Command::Fixed => "fixed\n".to_owned(),
    }
}

/// Parses one line received on standard input into a [`Command`].
#[must_use]
pub fn parse_command(line: &str) -> Option<Command> {
    let trimmed = line.trim();
    match trimmed.split_once(' ') {
        Some(("reload", path)) if !path.is_empty() => Some(Command::Reload(PathBuf::from(path))),
        Some(("error", text)) => serde_json::from_str(text).ok().map(Command::Errors),
        None => match trimmed {
            "quit" => Some(Command::Quit),
            "restart" => Some(Command::Restart),
            "fixed" => Some(Command::Fixed),
            _ => None,
        },
        _ => None,
    }
}

/// Removes ANSI CSI terminal escape sequences from `text`.
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            plain.push(c);
            continue;
        }
        // A CSI sequence ends at its first ASCII alphabetic final byte.
        if chars.next() == Some('[') {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_all_commands() {
        let cases = [
            Command::Quit,
            Command::Restart,
            Command::Reload(PathBuf::from("assets/themes/dark.json")),
            Command::Errors("error[E0308]: mismatched types\n  line 2".into()),
            Command::Fixed,
        ];
        for command in cases {
            let wire = encode_command(&command);
            assert!(wire.ends_with('\n'));
            assert_eq!(parse_command(&wire), Some(command));
        }
    }

    #[test]
    fn strips_ansi_when_encoding_errors() {
        let wire = encode_command(&Command::Errors(
            "\x1b[1m\x1b[31merror\x1b[0m: bad\n  \x1b[38;5;12m|\x1b[0m".into(),
        ));
        assert_eq!(
            parse_command(&wire),
            Some(Command::Errors("error: bad\n  |".into()))
        );
    }

    #[test]
    fn ignores_unknown_lines() {
        assert_eq!(parse_command(""), None);
        assert_eq!(parse_command("unknown"), None);
        assert_eq!(parse_command("error not-json"), None);
        assert_eq!(parse_command("reload "), None);
    }
}
