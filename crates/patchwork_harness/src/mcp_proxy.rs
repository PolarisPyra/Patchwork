//! The relay process a CLI starts for an MCP server Patchwork runs: it passes the CLI's stdin to
//! Patchwork and Patchwork's answers to its stdout.

// Rust guideline compliant 2026-11-27

use std::io::{BufRead, BufReader, Write};

use serde_json::{Value, json};

/// The argument that starts Patchwork's binary as a relay to one of Patchwork's servers.
pub const PROXY_ARG: &str = "--mcp-proxy";

/// The server Patchwork's own tools are served under, to every agent.
pub const OWN_SERVER: &str = "patchwork";

/// Runs the MCP relay process that a CLI starts, and returns its exit code.
///
/// Runs the relay a CLI starts: `--mcp-proxy <relay file> <server> [chat]`, with the chat
/// for Patchwork's own server. It passes stdin to Patchwork and Patchwork's answers to stdout; the
/// process exits with what this returns.
#[expect(
    clippy::print_stderr,
    reason = "a CLI runs this as a program of its own and shows its stderr; it has no log"
)]
pub fn run_proxy() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let (file, server, chat) = match args.as_slice() {
        [file, server] => (file, server, None),
        [file, server, chat] => (file, server, Some(chat)),
        _ => {
            eprintln!("Usage: {PROXY_ARG} <relay file> <server> [chat]");
            return 2;
        }
    };
    let relay: Value = if let Some(relay) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        relay
    } else {
        eprintln!("Patchwork is not running");
        return 1;
    };
    let Some(port) = relay["port"].as_u64().and_then(|p| u16::try_from(p).ok()) else {
        eprintln!("Patchwork's relay file names no port");
        return 1;
    };
    let mut stream = match std::net::TcpStream::connect(("127.0.0.1", port)) {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Could not reach Patchwork: {e}");
            return 1;
        }
    };
    let hello = json!({"token": relay["token"], "server": server, "chat": chat});
    if writeln!(stream, "{hello}").is_err() {
        return 1;
    }
    let Ok(incoming) = stream.try_clone() else {
        return 1;
    };
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        for line in BufReader::new(incoming).lines().map_while(Result::ok) {
            if writeln!(stdout, "{line}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
        // Patchwork closed the relay, e.g. on quitting.
        #[expect(
            clippy::exit,
            reason = "the relay helper ends when Patchwork closes it"
        )]
        std::process::exit(0);
    });
    for line in std::io::stdin().lines().map_while(Result::ok) {
        if writeln!(stream, "{line}").is_err() {
            return 1;
        }
    }
    // The CLI is done sending; the answers still owed arrive before Patchwork closes the relay,
    // which ends the process above.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    loop {
        std::thread::park();
    }
}
