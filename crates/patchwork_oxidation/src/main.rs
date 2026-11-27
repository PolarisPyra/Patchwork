//! Thin CLI wrapper around the [`patchwork_oxidation`] hot-reload library.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, bail};
use patchwork_oxidation::Term;
use patchwork_oxidation::config;

const USAGE: &str = "\
usage: cargo oxidation [OPTIONS] [-- ARGS FOR THE APP]

Rebuilds a Rust app on each save and restarts it. Reads oxidation.toml from the current folder
or the nearest one above it. While running: r rebuilds, c clears the terminal, q quits.

Options:
  --config <path>      Load an explicit oxidation.toml file
  -p, --package <pkg>  Override [build].package
  --bin <name>         Override [build].bin
  --no-handoff         Disable the oxidation-client handoff protocol
  -V, --version        Print version
  -h, --help           Print this help message";

fn main() -> ExitCode {
    let mut term = Term::new();
    match run(&mut term) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            term.fail(&format!("{e:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run(term: &mut Term) -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config_path: Option<PathBuf> = None;
    let mut package: Option<String> = None;
    let mut bin: Option<String> = None;
    let mut no_handoff = false;
    let mut app_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => app_args.extend(args.by_ref()),
            "-h" | "--help" => {
                term.plain(USAGE);
                return Ok(());
            }
            "-V" | "--version" => {
                term.plain(&format!("oxidation {}", env!("CARGO_PKG_VERSION")));
                return Ok(());
            }
            "--config" => {
                let value = args.next().context("--config expects a file path")?;
                config_path = Some(PathBuf::from(value));
            }
            "-p" | "--package" => {
                let value = args.next().context("--package expects a package name")?;
                package = Some(value);
            }
            "--bin" => {
                let value = args.next().context("--bin expects a binary name")?;
                bin = Some(value);
            }
            "--no-handoff" => no_handoff = true,
            _ => bail!("unknown argument `{arg}`\n\n{USAGE}"),
        }
    }

    let (mut config, root) = if let Some(path) = config_path {
        config::load_file(&path)?
    } else {
        let here = std::env::current_dir().context("could not read the current folder")?;
        config::load(&here)?
    };
    if let Some(pkg) = package {
        config.build.package = Some(pkg);
    }
    if let Some(target_bin) = bin {
        config.build.bin = Some(target_bin);
    }
    if no_handoff {
        config.run.handoff = false;
    }

    patchwork_oxidation::run_session(root, config, app_args, term)
}
