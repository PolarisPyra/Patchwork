//! Projects on other machines, reached over SSH.
//!
//! A remote project's root is `ssh://<alias><path>` ([`location`]), on a Linux, macOS or Windows
//! host ([`host`]). Commands for it run over one shared connection per host ([`connection`]);
//! [`command`] turns a command built for a folder into one that runs on the host. Patchwork's
//! helper there, copied over the first time with the launch files a command names ([`upload`]),
//! answers questions about the host's folders ([`op`], [`answer`]) and runs long commands
//! ([`serve`]).

// Rust guideline compliant 2026-10-09

mod answer;
mod command;
mod connection;
pub mod fs;
mod host;
mod location;
pub mod op;
pub mod serve;
mod upload;

#[doc(inline)]
pub use command::{interactive, prepare, shell_in, supervised};
#[doc(inline)]
pub use connection::{close_all, set_relay_port, ssh};
#[doc(inline)]
pub use host::{Host, Os, home, probe};
#[doc(inline)]
pub use location::{Location, display, has_drive, here, is_remote, join, quote, root, same_host, split};
#[doc(inline)]
pub use upload::mirrored;
