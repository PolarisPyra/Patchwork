//! The user's machine, which no domain owns.
//!
//! The programs Patchwork starts and the environment they run with, the
//! environment the user's shell sets up, the shells commands and the terminal run in, opening
//! files and links on the desktop, the systemd scope agents run in, the range a command runs in,
//! the process table, Patchwork's own folders, its icon on the desktop, and time.

// Rust guideline compliant 2026-10-09

pub mod browse;
mod error;
pub mod dirs;
mod environment;
mod host;
pub mod instance;
mod launcher;
pub mod memory;
pub mod fs;
pub mod contained;
pub mod device;
pub mod processes;
pub mod ssh;
pub mod scope;
mod shells;
pub mod time;

/// This build's version, which names the helper a host runs and the release it comes from.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use host::{hide_console, host_command, host_env};
pub(crate) use host::path_var;
pub use host::{blocked, lossy_lines, unreachable};
#[cfg(windows)]
pub use host::expand_windows_vars;
#[doc(inline)]
pub use dirs::{Dirs, dirs};
#[doc(inline)]
pub use launcher::set_launcher_icon;
#[doc(inline)]
pub use time::{new_id, now_ms};
#[doc(inline)]
pub use environment::{ENV_READ, find_program, login_env, login_var, search_path};
#[doc(inline)]
pub use shells::{
    available_shells, command_shell, effective_shell, find_shell, is_posix, login_shell,
    shell_args, shell_command, shell_name,
};
