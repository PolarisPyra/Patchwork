//! The user's machine: the programs Patchwork starts and the environment they run with, the login
//! shells agents and the terminal use, opening files and links on the desktop, the systemd scope
//! agents run in, Patchwork's own folders, and time.

pub mod dirs;
mod host;
pub mod launch;
pub mod fs;
pub(crate) mod scope;
mod shells;
pub(crate) mod time;

pub use host::{hide_console, host_command, host_env, path_var};
pub use dirs::{Dirs, dirs};
pub use time::now_ms;
pub use shells::{
    available_shells, effective_shell, find_shell, is_posix, login_shell, posix_shell, shell_args,
    shell_name,
};

/// An id for a record Patchwork creates.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
