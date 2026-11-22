//! A picture of a quilt's version for the gallery, taken by the installed browser once and
//! kept in the cache.
//!
//! The browser is started headless for each picture with a profile of its own, draws the page
//! for a few seconds of page time, writes a PNG and exits. One picture is taken at a time.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::browser::{die_with_patchwork, find_browser};
use crate::{Core, Error};

/// The window the page is drawn in, in CSS pixels; cards show it scaled down.
const SIZE: (u32, u32) = (1280, 800);

/// How much page time scripts get to draw before the picture, in milliseconds.
const PAGE_TIME: u32 = 2500;

/// How long the browser may take in all before it is killed.
const LIMIT: Duration = Duration::from_secs(30);

/// Held while a picture is taken, so the profile folder is never shared.
static TAKING: Mutex<()> = Mutex::new(());

impl Core {
    /// A picture of version `n` of the quilt `id`, as a PNG file, drawn dark when `dark` as the
    /// viewer draws it for a dark theme. Taken the first time it is asked for and kept after
    /// that. Blocks for the few seconds the browser takes, so it runs off the UI
    /// thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the quilt or version is gone, no browser is installed, or the
    /// browser fails or takes too long.
    pub fn quilt_thumbnail(&self, id: &str, n: u32, dark: bool) -> Result<PathBuf, Error> {
        let folder = crate::system::dirs().cache.join("quilts").join("thumbs");
        let file = folder.join(thumbnail_name(id, n, dark));
        if file.is_file() {
            return Ok(file);
        }
        let browser = find_browser().ok_or("Pictures of quilts need a Chromium browser")?;
        let url = self.quilt_page(id, n)?;
        std::fs::create_dir_all(&folder)?;

        let _taking = TAKING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if file.is_file() {
            return Ok(file);
        }
        let profile = crate::system::dirs()
            .cache
            .join("browser")
            .join("thumbnail-profile");
        // A Patchwork that was killed mid-picture leaves the folder behind.
        let _ = std::fs::remove_dir_all(&profile);
        std::fs::create_dir_all(&profile)?;
        let partial = folder.join(format!("{id}-{n}.part.png"));
        let mut command = Command::new(&browser);
        die_with_patchwork(&mut command);
        let mut child = command
            .arg("--headless")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--window-size={},{}", SIZE.0, SIZE.1))
            .arg(format!("--virtual-time-budget={PAGE_TIME}"))
            .arg(format!("--screenshot={}", partial.display()))
            // The page reads it as `prefers-color-scheme`: 0 is dark and 1 light.
            .arg(format!(
                "--blink-settings=preferredColorScheme={}",
                u8::from(!dark)
            ))
            .args([
                "--hide-scrollbars",
                "--mute-audio",
                "--no-first-run",
                "--no-default-browser-check",
            ])
            .arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Message(format!("Could not start {}: {e}", browser.display())))?;

        let started = Instant::now();
        let done = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if started.elapsed() < LIMIT => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
            }
        };
        // Its helper processes may hold the folder a moment longer.
        std::thread::sleep(Duration::from_millis(200));
        let _ = std::fs::remove_dir_all(&profile);
        if !done || !partial.is_file() {
            let _ = std::fs::remove_file(&partial);
            return Err("The browser did not take the picture".into());
        }
        std::fs::rename(&partial, &file)?;
        Ok(file)
    }
}

/// The file a version's picture is kept in, one for each of the dark and the light theme.
pub(crate) fn thumbnail_name(id: &str, n: u32, dark: bool) -> String {
    let scheme = if dark { "dark" } else { "light" };
    format!("{id}-{n}-{scheme}.png")
}
