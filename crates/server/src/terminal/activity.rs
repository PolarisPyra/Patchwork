//! Whether a terminal's shell sits at its prompt. A bash or zsh started plain (`-i`) gets a startup
//! file of Patchwork's that runs the user's own and then records, in the terminal's private folder,
//! each time the shell shows its prompt and each time it runs a command. Output never counts as
//! either.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::Launch;
use super::range::write_private;

/// Where the shell stands, as its prompt hooks last recorded it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Activity {
    Idle,
    Busy,
    Unknown,
}

impl Activity {
    pub(super) fn name(self) -> &'static str {
        match self {
            Activity::Idle => "idle",
            Activity::Busy => "busy",
            Activity::Unknown => "unknown",
        }
    }
}

/// One shell's prompt hooks and the file they write.
pub(super) struct ShellActivity {
    state: PathBuf,
    seen: Mutex<Seen>,
}

#[derive(Default)]
struct Seen {
    revision: u64,
    observed: String,
    /// The record when input was last typed, which says nothing about the shell since.
    invalidated: Option<String>,
}

impl ShellActivity {
    /// What was typed may have started a command, so the last record no longer counts.
    pub(super) fn invalidate(&self) {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        seen.invalidated = Some(self.read());
        seen.revision += 1;
    }

    /// The shell's last recorded state, and a revision that changes with each new record.
    pub(super) fn inspect(&self, pid: i32) -> (Activity, u64) {
        let record = self.read();
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if record != seen.observed {
            seen.observed.clone_from(&record);
            seen.revision += 1;
        }
        let mut parts = record.trim_end_matches('\n').split(':');
        let (shell, sequence, state) = (parts.next(), parts.next(), parts.next());
        let whole = parts.next().is_none()
            && sequence.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
        let state = match state {
            _ if seen.invalidated.as_ref() == Some(&record) => Activity::Unknown,
            _ if !whole || shell != Some(pid.to_string().as_str()) => Activity::Unknown,
            Some("idle") => Activity::Idle,
            Some("busy") => Activity::Busy,
            _ => Activity::Unknown,
        };
        (state, seen.revision)
    }

    fn read(&self) -> String {
        std::fs::read_to_string(&self.state).unwrap_or_default()
    }
}

/// Sets `launch` up to record its prompt in `dir` when it starts bash or zsh with only `-i`, which
/// keeps the user's startup files. None for other shells and arguments, and when the files can't
/// be written.
pub(super) fn prepare(launch: &mut Launch, dir: &Path) -> Option<ShellActivity> {
    if cfg!(windows) || launch.args != ["-i"] {
        return None;
    }
    let shell = crate::system::shell_name(&launch.shell);
    if shell != "bash" && shell != "zsh" {
        return None;
    }
    let state = quote(&dir.join("state").to_string_lossy());
    let guards = quote(&dir.join("guards").to_string_lossy());
    let written = if shell == "bash" {
        let rc = dir.join("bashrc");
        let ps0 = quote(&format!(
            "$(builtin printf '%s:%s:busy' \"$__patchwork_shell_pid\" \"$__patchwork_shell_sequence\" >| {state})"
        ));
        let text = BASHRC
            .replace("@STATE@", &state)
            .replace("@GUARDS@", &guards)
            .replace("@PS0@", &ps0);
        write_private(&rc, &text).map(|()| {
            launch.args = vec![
                "--rcfile".into(),
                rc.to_string_lossy().into_owned(),
                "-i".into(),
            ];
        })
    } else {
        let zdotdir = match launch.env.get("ZDOTDIR") {
            Some(dir) => format!("ZDOTDIR={}", quote(dir)),
            None => "unset ZDOTDIR".into(),
        };
        let text = ZSHENV
            .replace("@ZDOTDIR@", &zdotdir)
            .replace("@STATE@", &state)
            .replace("@GUARDS@", &guards);
        write_private(&dir.join(".zshenv"), &text).map(|()| {
            launch
                .env
                .insert("ZDOTDIR".into(), dir.to_string_lossy().into_owned());
        })
    };
    match written {
        Ok(()) => Some(ShellActivity {
            state: dir.join("state"),
            seen: Mutex::default(),
        }),
        Err(e) => {
            log::warn!(error:% = e; "could not write the terminal's prompt hooks: {{error}}");
            None
        }
    }
}

/// `value` as one word in single quotes.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Bash reads this in place of `.bashrc`, which it runs first. The prompt hook runs before each
/// prompt, and `PS0` as each command starts. A trap makes its state unknown. A bash that is too
/// old for `PS0`, or whose prompt variables are read-only, runs the user's file alone.
const BASHRC: &str = r#"[[ ! -r ~/.bashrc ]] || builtin source ~/.bashrc
if (( BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4) )) && [[ ! $(declare -p PROMPT_COMMAND PS0 2>/dev/null) =~ declare\ -[^[:space:]]*r ]]; then
  __patchwork_shell_pid=$BASHPID; __patchwork_shell_sequence=0
  __patchwork_shell_idle() {
    local result=$?
    if [[ $BASHPID == "$__patchwork_shell_pid" ]]; then
      (( ++__patchwork_shell_sequence ))
      local activity=idle
      builtin trap -p >| @GUARDS@
      [[ ! -s @GUARDS@ ]] || activity=unknown
      builtin printf '%s:%s:%s\n' "$BASHPID" "$__patchwork_shell_sequence" "$activity" >| @STATE@
    fi
    return "$result"
  }
  if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a "* ]]; then
    PROMPT_COMMAND+=(__patchwork_shell_idle)
  else
    PROMPT_COMMAND="${PROMPT_COMMAND}"$'\n'"__patchwork_shell_idle"
  fi
  PS0+=@PS0@
fi
"#;

/// Zsh reads this as its `.zshenv` through `ZDOTDIR`. It puts the user's `ZDOTDIR` back first, so
/// zsh goes on to the user's own files, and adds its hooks at the first prompt, after `.zshrc`.
/// A trap or file handler makes its state unknown.
const ZSHENV: &str = r#"@ZDOTDIR@
[[ ! -r ${ZDOTDIR:-$HOME}/.zshenv ]] || builtin source "${ZDOTDIR:-$HOME}/.zshenv"
typeset -g __patchwork_shell_pid=$$ __patchwork_shell_sequence=0
__patchwork_shell_activity() {
  (( ZSH_SUBSHELL == 0 && $$ == __patchwork_shell_pid )) || return
  (( ++__patchwork_shell_sequence ))
  builtin printf '%s:%s:%s\n' "$$" "$__patchwork_shell_sequence" "$1" >| @STATE@
  return 0
}
__patchwork_shell_idle() {
  { builtin trap; zle -F; } >| @GUARDS@
  if [[ $CONTEXT != start || -n $BUFFER ]]; then __patchwork_shell_activity busy
  elif [[ -s @GUARDS@ || -n ${(k)functions[(I)TRAP*]} ]]; then __patchwork_shell_activity unknown
  else __patchwork_shell_activity idle; fi
}
__patchwork_shell_busy() { __patchwork_shell_activity busy }
__patchwork_shell_init() {
  autoload -Uz add-zle-hook-widget add-zsh-hook
  add-zle-hook-widget line-init __patchwork_shell_idle
  add-zle-hook-widget line-finish __patchwork_shell_busy
  add-zsh-hook preexec __patchwork_shell_busy
  precmd_functions=(${precmd_functions:#__patchwork_shell_init})
}
typeset -ga precmd_functions
precmd_functions+=(__patchwork_shell_init)
"#;
