//! The user's machine, which no domain owns.
//!
//! The programs Patchwork starts and the environment they run with, the
//! environment the user's shell sets up, the shells commands and the terminal run in, opening
//! files and links on the desktop, the systemd scope agents run in, the range a command runs in,
//! the process table, Patchwork's own folders, and time.

// Rust guideline compliant 2026-10-09

pub mod browse;
pub(crate) mod dirs;
mod environment;
mod host;
pub mod instance;
pub mod fs;
pub(crate) mod contained;
pub mod device;
pub(crate) mod processes;
pub mod remote;
pub(crate) mod scope;
mod shells;
pub mod ssh;
pub(crate) mod time;

#[doc(inline)]
pub use host::{hide_console, host_command, host_env, path_var};
pub(crate) use host::lossy_lines;
#[cfg(windows)]
pub(crate) use host::expand_windows_vars;
#[doc(inline)]
pub use dirs::{Dirs, dirs};
#[doc(inline)]
pub use time::{new_id, now_ms};
#[doc(inline)]
pub use environment::{ENV_READ, find_program, login_env, login_var, search_path};
#[doc(inline)]
pub use shells::{
    available_shells, command_shell, effective_shell, find_shell, is_posix, login_shell,
    shell_args, shell_command, shell_name,
};
