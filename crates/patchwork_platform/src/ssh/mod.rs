//! Projects on other machines, reached over SSH.
//!
//! A remote project's root is `ssh://<alias><path>` ([`location`]), on a Linux, macOS or Windows
//! host ([`host`]). Commands for it run over one shared connection per host ([`connection`]);
//! [`command`] turns a command built for a folder into one that runs on the host. Patchwork's
//! helper there, copied over the first time ([`upload`]) with the launch files a command names ([`mirror`]),
//! answers questions about the host's folders ([`op`]), from one process that stays running per
//! host ([`session`]). The helper itself is the `patchwork-remote` crate.
//! [`config`] reads the hosts in the user's `~/.ssh/config`.

// Rust guideline compliant 2026-10-09

mod command;
pub mod config;
mod connection;
pub mod fs;
mod host;
mod location;
mod mirror;
pub mod op;
mod session;
mod upload;

#[doc(inline)]
pub use command::{EXEC_ARG, RUN_ARG, SHELL_ARG, Spec, interactive, prepare, shell_in, supervised};
#[doc(inline)]
pub use connection::{close_all, relay_reaches, set_relay_port, ssh};
#[doc(inline)]
pub use host::{Host, Os, home, kind, probe};
#[doc(inline)]
pub use location::{Location, display, has_drive, here, is_remote, join, quote, root, same_host, split};
#[doc(inline)]
pub use mirror::{Kind, UNPACK_ARG, mirrored};
#[doc(inline)]
pub use session::OPS_ARG;
#[doc(inline)]
pub use upload::{forget, set_helper_source};
