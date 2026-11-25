//! How big an agent's command line is, for the logs and for Windows' limit on it.
//!
//! Windows refuses to start a program whose command line passes 32,767 characters, and a `.cmd`
//! shim, which runs under `cmd.exe`, refuses past 8,191. Either way the error is only "The filename
//! or extension is too long" (os error 206), which names nothing. This measures a command as
//! Windows would quote it, says which arguments make it big, and names no value, since an
//! argument can hold a prompt, instructions, or a path in the user's home.

// Rust guideline compliant 2026-11-24

use std::ffi::OsStr;
use std::fmt;
use std::process::Command;

/// The longest command line Windows starts a program with, in UTF-16 units.
pub const WINDOWS_LIMIT: usize = 32_767;

/// The longest command line `cmd.exe` runs, which a `.cmd` or `.bat` shim goes through.
pub const SHIM_LIMIT: usize = 8_191;

/// How many of the biggest argument groups a summary names.
const LARGEST: usize = 5;

/// The size of one command, and the arguments that make it so.
#[derive(Debug, Clone)]
pub struct CommandSize {
    /// The program's file name, without its folder.
    pub program: String,
    /// Whether the program is a `.cmd` or `.bat` shim, or `cmd.exe` itself.
    pub shim: bool,
    /// How many arguments follow the program.
    pub args: usize,
    /// The whole command line as Windows quotes it, in UTF-16 units.
    pub total: usize,
    /// The environment block the program is given, in UTF-16 units.
    pub env: usize,
    /// The folder it starts in, in UTF-16 units, which Windows also answers with os error 206 when
    /// it passes 260; zero when it inherits Patchwork's.
    pub cwd: usize,
    /// The biggest groups of arguments by their label, with how many there are and their size.
    pub largest: Vec<(String, usize, usize)>,
}

impl CommandSize {
    /// Measures `cmd`, with the environment it inherits and the variables it sets.
    #[must_use]
    pub fn of(cmd: &Command) -> Self {
        let program = cmd.get_program();
        let name = std::path::Path::new(program)
            .file_name()
            .map_or_else(|| program.to_string_lossy(), OsStr::to_string_lossy)
            .into_owned();
        let shim = is_shim(program);
        let args: Vec<&OsStr> = cmd.get_args().collect();
        let mut total = quoted_len(program);
        let mut groups: Vec<(String, usize, usize)> = Vec::new();
        let mut flag: Option<String> = None;
        for arg in &args {
            let len = quoted_len(arg) + 1;
            total += len;
            let text = arg.to_string_lossy();
            let label = if text.starts_with('-') {
                let label = flag_label(&text);
                flag = Some(label.clone());
                label
            } else {
                value_label(flag.take().as_deref(), &text)
            };
            match groups.iter_mut().find(|(l, _, _)| *l == label) {
                Some((_, count, size)) => {
                    *count += 1;
                    *size += len;
                }
                None => groups.push((label, 1, len)),
            }
        }
        groups.sort_by_key(|g| std::cmp::Reverse(g.2));
        groups.truncate(LARGEST);
        Self {
            program: name,
            shim,
            args: args.len(),
            total,
            env: env_len(cmd),
            cwd: cmd.get_current_dir().map_or(0, |dir| {
                dir.as_os_str().to_string_lossy().encode_utf16().count()
            }),
            largest: groups,
        }
    }

    /// The limit this command must stay under on Windows.
    #[must_use]
    pub fn limit(&self) -> usize {
        if self.shim { SHIM_LIMIT } else { WINDOWS_LIMIT }
    }

    /// Whether Windows would refuse to start this command.
    #[must_use]
    pub fn too_long(&self) -> bool {
        self.total > self.limit()
    }

    /// The biggest groups, as "agents.* ×140 = 38,900 chars".
    #[must_use]
    pub fn largest_text(&self) -> String {
        self.largest
            .iter()
            .map(|(label, count, size)| {
                if *count > 1 {
                    format!("{label} ×{count} = {} chars", thousands(*size))
                } else {
                    format!("{label} = {} chars", thousands(*size))
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl fmt::Display for CommandSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}: {} arguments, command line {} chars (Windows limit {}), environment {} chars, \
             working folder {} chars; largest: {}",
            self.program,
            if self.shim { " (through cmd.exe)" } else { "" },
            self.args,
            thousands(self.total),
            thousands(self.limit()),
            thousands(self.env),
            self.cwd,
            self.largest_text(),
        )
    }
}

/// Starts `cmd` for `what`, such as "the Codex agent" or "the MCP server github".
///
/// On Windows a command past its limit is refused before it starts, with what makes it long. A
/// command that fails to start is logged with its size and the system's error code, whose text
/// Windows translates into the user's language.
///
/// # Errors
///
/// Returns the error starting it gave, or one that names the size of a command too long.
pub fn spawn(cmd: &mut Command, what: &str) -> std::io::Result<std::process::Child> {
    let size = CommandSize::of(cmd);
    if cfg!(windows) && size.too_long() {
        log::warn!(what:% = what, size:% = size; "{{what}} has a command line too long for Windows: {{size}}");
        return Err(std::io::Error::other(size.refusal()));
    }
    cmd.spawn().inspect_err(|e| log_failure(what, e, &size))
}

/// Logs that `what` could not start, with the error's code and what it means, and the size of the
/// command.
pub fn log_failure(what: &str, error: &std::io::Error, size: &CommandSize) {
    let code = error.raw_os_error();
    log::warn!(
        what:% = what, error:% = error, code:? = code,
        meaning:% = code.and_then(os_error_meaning).unwrap_or("-"), size:% = size;
        "could not start {{what}}: {{error}} (os error code {{code}}: {{meaning}}); {{size}}"
    );
}

/// What a Windows error code from starting a program means, for the ones it commonly gives.
#[must_use]
pub fn os_error_meaning(code: i32) -> Option<&'static str> {
    if !cfg!(windows) {
        return match code {
            2 => Some("the program was not found"),
            7 => Some("the arguments and environment are too long"),
            13 => Some("permission denied"),
            _ => None,
        };
    }
    Some(match code {
        2 => "the program was not found",
        3 => "a folder on the program's path was not found",
        5 => "access was denied, often by antivirus or the file's permissions",
        87 => "an argument holds a character Windows cannot pass",
        193 => "the file is not a program this Windows can run",
        206 => "the command line, a path, or the working folder is too long",
        225 => "antivirus blocked the program",
        267 => "the working folder is not a valid folder",
        1260 => "a group policy blocks the program",
        _ => return None,
    })
}

impl CommandSize {
    /// The error a user is shown for a command too long to start.
    #[must_use]
    pub fn refusal(&self) -> String {
        format!(
            "{} can't start: its command line is {} characters, over the {} Windows allows{}. \
             The largest parts: {}.",
            self.program,
            thousands(self.total),
            thousands(self.limit()),
            if self.shim {
                " through a .cmd shim"
            } else {
                ""
            },
            self.largest_text(),
        )
    }
}

/// Whether `program` is a `.cmd` or `.bat` shim, or `cmd.exe` itself, which Windows runs under
/// `cmd.exe` and its shorter limit.
#[must_use]
pub fn is_shim(program: &OsStr) -> bool {
    let name = std::path::Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy()
        .to_ascii_lowercase();
    let ext = std::path::Path::new(&name)
        .extension()
        .and_then(OsStr::to_str);
    matches!(ext, Some("cmd" | "bat")) || name == "cmd.exe"
}

/// The label of a flag: itself, without a value joined by `=`.
fn flag_label(flag: &str) -> String {
    flag.split('=').next().unwrap_or(flag).to_string()
}

/// The label of a value: for a `-c` setting its key, with the name of a custom agent or server
/// left out so they group together; otherwise the flag it follows.
fn value_label(flag: Option<&str>, value: &str) -> String {
    match flag {
        Some("-c" | "--config") => {
            let key = value.split('=').next().unwrap_or_default();
            let mut parts = key.split('.');
            match (parts.next(), parts.next()) {
                (Some(table @ ("agents" | "mcp_servers" | "hooks")), Some(_)) => {
                    format!("-c {table}.*")
                }
                _ => format!("-c {key}"),
            }
        }
        Some(flag) => format!("{flag} value"),
        None => "argument".into(),
    }
}

/// The length of `arg` once Windows quotes it, in UTF-16 units: quotes around one with a space,
/// tab or quote in it, and a backslash before each quote and doubled backslashes before those.
fn quoted_len(arg: &OsStr) -> usize {
    let text = arg.to_string_lossy();
    let units = text.encode_utf16().count();
    let needs_quotes = text.is_empty() || text.contains([' ', '\t', '"']);
    if !needs_quotes {
        return units;
    }
    let mut extra = 2;
    let mut backslashes = 0;
    for c in text.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                extra += backslashes + 1;
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
    }
    units + extra + backslashes
}

/// The environment block `cmd` starts with: each `NAME=value` and its terminator.
fn env_len(cmd: &Command) -> usize {
    let mut vars: std::collections::BTreeMap<String, Option<String>> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                Some(v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    for (key, value) in cmd.get_envs() {
        vars.insert(
            key.to_string_lossy().into_owned(),
            value.map(|v| v.to_string_lossy().into_owned()),
        );
    }
    vars.iter()
        .filter_map(|(k, v)| {
            Some(k.encode_utf16().count() + v.as_ref()?.encode_utf16().count() + 2)
        })
        .sum()
}

/// `n` with thousands separated by commas.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_like_windows() {
        assert_eq!(quoted_len(OsStr::new("plain")), 5);
        assert_eq!(quoted_len(OsStr::new("")), 2);
        assert_eq!(quoted_len(OsStr::new("a b")), 5);
        // "a\"b" -> "a\"b" with quotes: 3 + 1 escape + 2 quotes
        assert_eq!(quoted_len(OsStr::new("a\"b")), 6);
        // A trailing backslash before the closing quote is doubled.
        assert_eq!(quoted_len(OsStr::new("a b\\")), 7);
    }

    #[test]
    fn groups_agents_and_names_no_value() {
        let mut cmd = Command::new("C:/npm/codex.cmd");
        cmd.arg("app-server");
        for name in ["one", "two", "three"] {
            cmd.args(["-c", &format!("agents.{name}.description=\"secret text\"")]);
        }
        cmd.args(["--model", "gpt-5"]);
        let size = CommandSize::of(&cmd);
        assert!(size.shim);
        assert_eq!(size.args, 9);
        assert_eq!(size.limit(), SHIM_LIMIT);
        let labels: Vec<&str> = size.largest.iter().map(|(l, _, _)| l.as_str()).collect();
        assert_eq!(labels[0], "-c agents.*");
        assert_eq!(size.largest[0].1, 3);
        let text = size.to_string();
        assert!(!text.contains("secret") && !text.contains("one") && !text.contains("gpt-5"));
    }

    #[test]
    fn finds_shims() {
        assert!(is_shim(OsStr::new("C:\\x\\Codex.CMD")));
        assert!(is_shim(OsStr::new("run.bat")));
        assert!(is_shim(OsStr::new("cmd.exe")));
        assert!(!is_shim(OsStr::new("/usr/bin/codex")));
        assert!(!is_shim(OsStr::new("codex.exe")));
    }

    #[test]
    fn separates_thousands() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(32_767), "32,767");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
