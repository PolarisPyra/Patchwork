//! The `bash` tool: a command in bash, in the environment the user's shell sets up, with a time
//! limit. Patchwork runs it rather than the agent's process, so one still running at its limit
//! carries on as a background job; this is the command it runs and what the model is told of it.
//!
//! Each command starts where the chat's last one ended: in its folder, when that is in the
//! project, and with the variables it exported. Bash writes both to the chat's shell file as
//! the command exits, and the next command reads them back before it runs.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const BASH_TIMEOUT: u64 = 120;

pub const MAX_BASH_TIMEOUT: u64 = 600;

/// Runs `PATCHWORK_COMMAND` after reading back the shell file, and, when `PATCHWORK_SHELL_KEEP`
/// is set, writes the folder (as a comment) and exported variables back as bash exits, to a
/// file only the user can read. Variables that let a later command load code of its own
/// choosing are left out.
const CARRY: &str = r##"__pw_file=$PATCHWORK_SHELL
__pw_keep=$PATCHWORK_SHELL_KEEP
__pw_cmd=$PATCHWORK_COMMAND
unset PATCHWORK_SHELL PATCHWORK_SHELL_KEEP PATCHWORK_COMMAND
if [ -n "$__pw_keep" ]; then
  trap '__pw_s=$?
  umask 077
  {
    case $PWD in *$'"'"'\n'"'"'*) ;; *) printf "# %s\n" "$PWD" ;; esac
    for __pw_v in $(compgen -e); do
      case $__pw_v in
        SHLVL|_|PWD|OLDPWD|BASH_ENV|ENV|PROMPT_COMMAND|LD_*) ;;
        *) printf "export %s=%q\n" "$__pw_v" "${!__pw_v}" ;;
      esac
    done
  } > "$__pw_file" 2>/dev/null
  exit $__pw_s' EXIT
fi
[ -f "$__pw_file" ] && . "$__pw_file" 2>/dev/null
eval "$__pw_cmd""##;

/// The file chat `chat_id`'s shell is kept in between its commands.
#[must_use]
pub fn shell_file(chat_id: &str) -> PathBuf {
    let name: String = chat_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    patchwork_platform::dirs()
        .cache
        .join("shell")
        .join(format!("{name}.sh"))
}

/// `command` as the bash tool runs it, starting from the shell kept in `file`, or in `root` when
/// there is none. With `keep`, the shell it leaves is written back to `file`.
///
/// # Errors
///
/// Returns an error when no shell is found to run the command in.
pub fn bash_command(command: &str, root: &Path, file: &Path, keep: bool) -> Result<Command, Error> {
    let carries = patchwork_platform::command_shell()
        .is_some_and(|shell| patchwork_platform::shell_name(&shell) == "bash");
    let Some(mut cmd) = patchwork_platform::shell_command(if carries { CARRY } else { command })
    else {
        return Err("No shell was found to run the command in".into());
    };
    if !carries {
        cmd.current_dir(root);
        return Ok(cmd);
    }
    cmd.current_dir(last_dir(file, root).unwrap_or_else(|| root.to_path_buf()));
    cmd.env("PATCHWORK_COMMAND", command)
        .env("PATCHWORK_SHELL", file);
    if keep {
        if let Some(dir) = file.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            log::warn!("Could not make the folder for shell files: {e}");
        }
        cmd.env("PATCHWORK_SHELL_KEEP", "1");
    }
    Ok(cmd)
}

/// The folder the last command in `file` ended in, while it is still inside `root`.
fn last_dir(file: &Path, root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(file).ok()?;
    let dir = text.lines().next()?.strip_prefix("# ")?;
    let dir = std::fs::canonicalize(dir).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    (dir.starts_with(&root) && dir.is_dir()).then_some(dir)
}

/// What the model is told of a command that printed `text` and exited with `code`, None when a
/// signal ended it.
///
/// # Errors
///
/// Returns an error, holding the output and the exit code, when the command exited with a nonzero
/// code or was ended by a signal.
pub fn outcome(text: &str, code: Option<i32>) -> Result<String, Error> {
    let text = text.trim_end();
    match code {
        Some(0) => Ok(if text.is_empty() {
            "(no output)".into()
        } else {
            text.into()
        }),
        code => {
            let code = code.map_or_else(|| "a signal".into(), |c| c.to_string());
            Err(format!("{text}\n\nExit code {code}").trim_start().into())
        }
    }
}
