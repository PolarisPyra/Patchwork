//! Patchwork's helper on an SSH host.
//!
//! Patchwork copies this binary to a host the first time it opens a project there, and runs the
//! project's agents, checks and hooks through it. It is built static for Linux on `x86_64` and
//! `aarch64`, so it runs on any distribution, and for macOS and Windows. The hooks, relays and bridges an agent starts on the
//! host run it with the same arguments they give Patchwork's own binary on this machine.

// Rust guideline compliant 2026-10-09

use patchwork_server::system::remote::serve;
use patchwork_server::{harness, permissions, provider};

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
        serve::EXEC_ARG => serve::exec,
        // The launch files Patchwork copies here for an agent.
        serve::UNPACK_ARG => serve::unpack,
        // A short command, such as git, described on the command line.
        serve::RUN_ARG => serve::run_inline,
        // A question about this host's folders, on stdin.
        serve::OP_ARG => serve::answer,
        // A terminal's shell on a Windows host.
        serve::SHELL_ARG => serve::shell,
        // An OpenCode turn's bridge to `opencode serve`.
        provider::opencode::bridge::BRIDGE_ARG => provider::opencode::bridge::run,
        // An API provider's turn, run as Patchwork's own agent.
        harness::BRIDGE_ARG => harness::run,
        // Checks a tool call against the permission rules.
        permissions::hook::HOOK_ARG => permissions::hook::run,
        // Antigravity's session start, for the system prompt from Settings.
        provider::hooks::PROMPT_ARG => provider::hooks::run_prompt_hook,
        // Claude Code's and Codex's relay to an MCP server Patchwork runs.
        provider::mcp::PROXY_ARG => provider::mcp::run_proxy,
        _ => {
            eprintln!("Unknown argument {arg}");
            std::process::exit(2);
        }
    };
    std::process::exit(run());
}
