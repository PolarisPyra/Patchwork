//! The user's machine, which no domain owns.
//!
//! The programs Patchwork starts and the environment they run with, the
//! environment the user's shell sets up, the shells commands and the terminal run in, opening
//! files and links on the desktop, the systemd scope agents run in, the range a command runs in,
//! the process table, Patchwork's own folders, its icon on the desktop, the credential store,
//! and time.

// Rust guideline compliant 2026-10-09

mod error;
pub mod files;
pub mod keychain;
pub mod machine;
pub mod processes;
pub mod ssh;
pub mod time;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use processes::programs::{hide_console, host_command, host_env};
pub(crate) use processes::programs::path_var;
pub use processes::programs::{Locked, blocked, lossy_lines, unreachable};
#[cfg(windows)]
pub use processes::programs::expand_windows_vars;
#[doc(inline)]
pub use files::dirs::{Dirs, dirs};
#[cfg(feature = "launcher-icon")]
#[doc(inline)]
pub use machine::launcher::set_launcher_icon;
#[doc(inline)]
pub use files::editor::open_in_editor;
#[doc(inline)]
pub use time::{new_id, now_ms};
#[doc(inline)]
pub use processes::environment::{ENV_READ, find_program, login_env, login_var, search_path};
#[doc(inline)]
pub use processes::shells::{
    available_shells, command_shell, effective_shell, find_shell, is_posix, login_shell,
    shell_args, shell_command, shell_name,
};
