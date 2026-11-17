//! System images: the ones installed, the ones `sdkmanager` offers, and downloading one with its
//! progress and its licence shown.

// Rust guideline compliant 2026-10-09

use std::io::{Read, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use super::avd::{android_version, api_number, phone_tag};
use super::tool::{self, tail, tool_command};
use super::types::{InstallEvent, Sdk, SystemImage};
use crate::Error;

/// How long `sdkmanager --list` gets: it starts Java and downloads the package lists.
const LIST_LIMIT: Duration = Duration::from_secs(120);

/// What `sdkmanager` ends a licence with when it waits for an answer.
const LICENSE_PROMPT: &str = "(y/N)";

/// A system image from its package, which older tools write as
/// `system-images;android-34;google_apis;x86_64` and newer ones with `/` between the parts.
fn image(package: &str, installed: bool) -> Option<SystemImage> {
    let parts: Vec<&str> = package.split([';', '/']).collect();
    let [kind, platform, tag, abi] = parts.as_slice() else {
        return None;
    };
    let api = platform.strip_prefix("android-")?;
    (*kind == "system-images" && phone_tag(tag)).then(|| SystemImage {
        package: package.to_string(),
        android_version: android_version(api),
        api: api.to_string(),
        tag: (*tag).to_string(),
        abi: (*abi).to_string(),
        installed,
    })
}

/// The phone and tablet images in the SDK at `root`, which keeps each in
/// `system-images/<platform>/<tag>/<abi>`.
pub(super) fn installed_in(root: &Path) -> Vec<SystemImage> {
    let folders = |folder: &Path| -> Vec<String> {
        std::fs::read_dir(folder)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.path().is_dir())
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    let images = root.join("system-images");
    let mut found = Vec::new();
    for platform in folders(&images) {
        for tag in folders(&images.join(&platform)) {
            for abi in folders(&images.join(&platform).join(&tag)) {
                // A download that was cut off leaves the folder without its description.
                let folder = images.join(&platform).join(&tag).join(&abi);
                if folder.join("source.properties").is_file() {
                    found.extend(image(
                        &format!("system-images;{platform};{tag};{abi}"),
                        true,
                    ));
                }
            }
        }
    }
    sort(&mut found);
    found
}

/// The newest first, a release before its previews (`37.2` before `37.2-beta3`), and for one API
/// level, the installed ones first.
fn sort(images: &mut [SystemImage]) {
    let preview = |image: &SystemImage| image.api.contains('-');
    images.sort_by(|a, b| {
        api_number(&b.api)
            .cmp(&api_number(&a.api))
            .then_with(|| preview(a).cmp(&preview(b)))
            .then_with(|| b.api.cmp(&a.api))
            .then_with(|| b.installed.cmp(&a.installed))
            .then_with(|| a.tag.cmp(&b.tag))
            .then_with(|| a.abi.cmp(&b.abi))
    });
}

/// The system images in the output of `sdkmanager --list`, each named by the first word of its
/// line.
pub(super) fn parse_list(listing: &str) -> Vec<SystemImage> {
    let mut found: Vec<SystemImage> = Vec::new();
    for line in listing.lines() {
        let Some(image) = line
            .split_whitespace()
            .next()
            .and_then(|package| image(package, false))
        else {
            continue;
        };
        if !found.iter().any(|other| other.same(&image)) {
            found.push(image);
        }
    }
    found
}

/// The phone and tablet system images already installed, the newest first.
#[must_use]
pub fn installed_images(sdk: &Sdk) -> Vec<SystemImage> {
    installed_in(&sdk.root)
}

/// Every phone and tablet system image: the installed ones, and the ones offered for download.
///
/// # Errors
///
/// Returns an error when the SDK has no `sdkmanager`, or it could not fetch the list.
pub fn available_images(sdk: &Sdk) -> Result<Vec<SystemImage>, Error> {
    let mut cmd = tool_command(sdk, sdk.sdkmanager()?);
    cmd.arg("--list").arg(sdk_root(sdk));
    let out = tool::output(cmd, LIST_LIMIT)?;
    let mut images = parse_list(&out.stdout);
    if images.is_empty() {
        return Err(Error::Tool {
            message: "The list of system images could not be fetched. Check the internet \
                      connection, and that Java 17 or newer is installed, then try again."
                .into(),
            detail: out.all(),
        });
    }
    let installed = installed_in(&sdk.root);
    for image in &mut images {
        image.installed = installed.iter().any(|have| have.same(image));
    }
    // One the lists no longer offer is still there to use.
    for have in installed {
        if !images.iter().any(|image| image.same(&have)) {
            images.push(have);
        }
    }
    sort(&mut images);
    Ok(images)
}

/// The argument that tells `sdkmanager` which SDK to work on, whichever one it was started from.
fn sdk_root(sdk: &Sdk) -> String {
    format!("--sdk_root={}", sdk.root.display())
}

/// The percentage in a line of `sdkmanager`'s progress, such as
/// `[=====      ] 25% Downloading x86_64-34_r12.zip...`, and what follows it.
pub(super) fn progress(line: &str) -> Option<(u8, String)> {
    let at = line.find('%')?;
    let digits: String = line[..at]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let percent: u8 = digits.parse().ok().filter(|percent| *percent <= 100)?;
    Some((percent, line[at + 1..].trim().to_string()))
}

/// Downloads and installs the system image `package`, a [`SystemImage::package`].
///
/// `on_event` is told how far the download is, and when `sdkmanager` shows a licence and waits.
/// The download then waits too, for an answer on `answers`: true accepts the licence, false or a
/// closed channel declines it, which ends the download. It blocks until the image is installed,
/// so call it off the UI thread.
///
/// # Errors
///
/// Returns an error when the SDK has no `sdkmanager`, the licence is declined, or the download
/// or the install fails, with the end of what `sdkmanager` printed.
pub fn install_image(
    sdk: &Sdk,
    package: &str,
    on_event: &mut dyn FnMut(InstallEvent),
    answers: &Receiver<bool>,
) -> Result<(), Error> {
    let mut cmd = tool_command(sdk, sdk.sdkmanager()?);
    cmd.arg("--install")
        .arg(package)
        .arg(sdk_root(sdk))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("sdkmanager didn't start: {e}"))?;
    let mut stdin = child.stdin.take();
    let (chunks, received) = channel::<Vec<u8>>();
    forward(child.stdout.take(), chunks.clone());
    forward(child.stderr.take(), chunks);

    let mut pending: Vec<u8> = Vec::new();
    // What was printed since the last question, which is the licence when one is asked about.
    let mut said: Vec<String> = Vec::new();
    let mut log = String::new();
    let mut last: Option<(u8, String)> = None;
    let mut declined = false;
    // Ends when both pipes close, which is when sdkmanager exits.
    for chunk in received {
        pending.extend_from_slice(&chunk);
        while let Some(end) = pending
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
        {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line).trim().to_string();
            if line.is_empty() {
                continue;
            }
            match progress(&line) {
                Some(now) if last.as_ref() != Some(&now) => {
                    on_event(InstallEvent::Progress {
                        percent: now.0,
                        step: now.1.clone(),
                    });
                    last = Some(now);
                }
                Some(_) => {}
                None => {
                    log.push_str(&line);
                    log.push('\n');
                    said.push(line);
                }
            }
        }
        // The question has no line end after it, as sdkmanager waits on the same line.
        if String::from_utf8_lossy(&pending).contains(LICENSE_PROMPT) {
            pending.clear();
            on_event(InstallEvent::License {
                text: std::mem::take(&mut said).join("\n"),
            });
            let accepted = answers.recv().unwrap_or(false);
            declined |= !accepted;
            let answer: &[u8] = if accepted { b"y\n" } else { b"N\n" };
            if let Some(stdin) = stdin.as_mut() {
                let written = stdin.write_all(answer).and_then(|()| stdin.flush());
                if let Err(e) = written {
                    log::warn!(error:% = e; "could not answer sdkmanager: {{error}}");
                }
            }
        }
    }
    drop(stdin);
    let status = child.wait()?;
    if declined {
        return Err(
            "The system image was not downloaded, because its licence was declined.".into(),
        );
    }
    if !status.success() {
        return Err(Error::Tool {
            message: "The system image didn't download. Check the internet connection and that \
                      the disk has a few gigabytes free, then try again."
                .into(),
            detail: tail(&log),
        });
    }
    Ok(())
}

/// Sends what a pipe gives to `chunks` as it arrives, on a thread of its own.
fn forward<R: Read + Send + 'static>(pipe: Option<R>, chunks: Sender<Vec<u8>>) {
    let Some(mut pipe) = pipe else {
        return;
    };
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4 << 10];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 || chunks.send(chunk[..n].to_vec()).is_err() {
                break;
            }
        }
    });
}
