//! The user's machine: the programs Patchwork starts and the environment they run with, the
//! environment the user's shell sets up, the shells commands and the terminal run in, opening
//! files and links on the desktop, the systemd scope agents run in, the process table,
//! Patchwork's own folders, and time.

pub mod dirs;
mod environment;
mod host;
pub mod launch;
pub mod fs;
pub(crate) mod processes;
pub(crate) mod scope;
mod shells;
pub(crate) mod time;

pub use host::{hide_console, host_command, host_env, path_var};
pub use dirs::{Dirs, dirs};
pub use time::now_ms;
pub use environment::{ENV_READ, find_program, login_env, login_var, search_path};
pub use shells::{
    available_shells, command_shell, effective_shell, find_shell, is_posix, login_shell,
    shell_args, shell_command, shell_name,
};

/// An id for a record Patchwork creates.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
