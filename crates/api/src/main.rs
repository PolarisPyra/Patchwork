//! `patchwork-api`: serves the API, migrates its database, and looks after an account from the
//! server's shell, so there is no admin site to expose.

use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

use log::{Level, LevelFilter, Log, Metadata, Record};
use patchwork_api::{Api, Config, http};

const USAGE: &str = "usage: patchwork-api serve | migrate | user EMAIL | sign-out EMAIL";

#[tokio::main]
async fn main() -> ExitCode {
    Stderr::install();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: &[&str]) -> Result<(), String> {
    if !matches!(
        args,
        ["serve"] | ["migrate"] | ["user", _] | ["sign-out", _]
    ) {
        return Err(USAGE.into());
    }
    let config = Config::from_env()?;
    let api = Api::open(&config).await?;
    match args {
        ["serve"] => serve(api, config.listen).await,
        ["migrate"] => {
            api.migrate().await?;
            log::info!("the database is up to date");
            Ok(())
        }
        ["user", email] => show(&api, email).await,
        ["sign-out", email] => {
            let (account, _) = find(&api, email).await?;
            api.sign_out_everywhere(account.id, None, None)
                .await
                .map_err(|e| e.message().to_owned())?;
            log::info!("signed {} out of every device", account.email);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

async fn serve(api: Api, listen: SocketAddr) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| format!("couldn't listen on {listen}: {e}"))?;
    let pruner = api.clone();
    tokio::spawn(async move {
        let mut hourly = tokio::time::interval(Duration::from_secs(60 * 60));
        loop {
            hourly.tick().await;
            if let Err(e) = pruner.prune().await {
                log::warn!("{e}");
            }
        }
    });
    log::info!("listening on {listen}");
    axum::serve(listener, http::router(api))
        .with_graceful_shutdown(stopped())
        .await
        .map_err(|e| format!("the server stopped: {e}"))
}

/// Resolves when systemd stops the service, or on Ctrl+C.
async fn stopped() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
    log::info!("stopping");
}

async fn find(
    api: &Api,
    email: &str,
) -> Result<
    (
        patchwork_api::accounts::Account,
        Vec<patchwork_api::devices::Device>,
    ),
    String,
> {
    api.find_account(email)
        .await
        .map_err(|e| e.message().to_owned())?
        .ok_or_else(|| format!("no account signs in with {email}"))
}

async fn show(api: &Api, email: &str) -> Result<(), String> {
    let (account, devices) = find(api, email).await?;
    log::info!(
        "account {}: {} ({}), made {}",
        account.id,
        account.email,
        account.name.as_deref().unwrap_or("no name"),
        account.created_at.format("%Y-%m-%d"),
    );
    for d in &devices {
        let seen = d
            .last_seen_at
            .map_or("never".into(), |t| t.format("%Y-%m-%d %H:%M").to_string());
        log::info!(
            "  device {}: {} on {}, {}, last seen {seen}",
            d.id,
            d.name,
            d.platform,
            d.version.as_deref().unwrap_or("unknown version"),
        );
    }
    if devices.is_empty() {
        log::info!("  signed in on no devices");
    }
    Ok(())
}

/// Writes the log to stderr, where systemd's journal picks it up with its own timestamps.
/// `RUST_LOG` sets the level: error, warn, info (the default), debug or trace.
struct Stderr(LevelFilter);

impl Stderr {
    fn install() {
        let level = std::env::var("RUST_LOG")
            .ok()
            .and_then(|l| l.parse().ok())
            .unwrap_or(LevelFilter::Info);
        if log::set_boxed_logger(Box::new(Stderr(level))).is_ok() {
            log::set_max_level(level);
        }
    }
}

impl Log for Stderr {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.0
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            Level::Error => "error",
            Level::Warn => "warning",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        };
        let message = format!("{level}: {}\n", record.args());
        let _ = std::io::Write::write_all(&mut std::io::stderr(), message.as_bytes());
    }

    fn flush(&self) {}
}
