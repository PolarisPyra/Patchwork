//! Patchwork's helper on an SSH host.
//!
//! Patchwork copies this binary to a host the first time it opens a project there, and runs the
//! project's agents, checks and hooks through it. It is built static for Linux on `x86_64` and
//! `aarch64`, so it runs on any distribution, and for macOS and Windows. The hooks, relays and bridges an agent starts on the
//! host run it with the same arguments they give Patchwork's own binary on this machine.

// Rust guideline compliant 2026-10-09

use patchwork_platform::ssh;

mod answer;
mod serve;

#[expect(
    clippy::print_stderr,
    reason = "ssh carries the helper's stderr back to Patchwork; it has no log of its own"
)]
fn main() {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("Patchwork starts this helper; it is not run by hand");
        std::process::exit(2);
    };
    let run: fn() -> i32 = match arg.as_str() {
        // A command Patchwork runs in a project on this host, until the connection ends.
        ssh::EXEC_ARG => serve::exec,
        // The launch files Patchwork copies here for an agent.
        ssh::UNPACK_ARG => serve::unpack,
        // A short command, such as git, described on the command line.
        ssh::RUN_ARG => serve::run_inline,
        // A question about this host's folders, on stdin.
        ssh::op::OP_ARG => answer::answer,
        // The same questions, one per line, for as long as Patchwork keeps the connection.
        ssh::OPS_ARG => answer::answer_all,
        // A terminal's shell on a Windows host.
        ssh::SHELL_ARG => serve::shell,
        // The roles Patchwork's own binary has too: bridges, hooks and relays.
        other => patchwork_backend::helper_role(other).unwrap_or_else(|| {
            eprintln!("Unknown argument {arg}");
            std::process::exit(2);
        }),
    };
    std::process::exit(run());
}
