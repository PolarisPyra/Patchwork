use std::path::PathBuf;

#[cfg(target_os = "macos")]
use std::sync::PoisonError;
#[cfg(windows)]
use std::sync::atomic::Ordering;
#[cfg(windows)]
use std::sync::PoisonError;

use patchwork_server::Res;

use super::Updates;

pub fn can_install() -> bool {
    use tauri::utils::config::BundleType;
    if cfg!(debug_assertions) {
        return false;
    }
    match tauri::utils::platform::bundle_type() {
        Some(BundleType::AppImage) => std::env::var_os("APPIMAGE").is_some(),
        Some(BundleType::Nsis) => true,
        Some(BundleType::App) => app_bundle().is_some(),
        _ => false,
    }
}

/// The `.app` bundle this copy runs from, on macOS.
fn app_bundle() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    // Patchwork.app/Contents/MacOS/patchwork
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

impl Updates {
    pub fn install_on_exit(&self) {
        #[cfg(windows)]
        if let Some(installer) = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            // Passive mode with a progress bar, as an update, like the installer's own updates.
            let mut cmd = std::process::Command::new(&installer);
            cmd.args(["/P", "/UPDATE"]);
            if self.restarting.load(Ordering::SeqCst) {
                cmd.arg("/R");
            }
            match cmd.spawn() {
                Ok(_) => std::process::exit(0),
                Err(e) => log::warn!("installing the update failed: {e}"),
            }
        }
    }
}

#[cfg(windows)]
pub(super) fn install(file: PathBuf, updates: &Updates) -> Res<()> {
    *updates
        .pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(file);
    Ok(())
}

/// Swaps the app bundle for the verified one.
#[cfg(target_os = "macos")]
pub(super) fn install(file: PathBuf, _updates: &Updates) -> Res<()> {
    let result = install_bundle(&file);
    let _ = std::fs::remove_file(&file);
    result
}

/// Moves the verified AppImage over the running one, which keeps its open file until the restart.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn install(file: PathBuf, _updates: &Updates) -> Res<()> {
    use std::os::unix::fs::PermissionsExt;
    let target = PathBuf::from(std::env::var_os("APPIMAGE").ok_or("Not running as an AppImage.")?);
    // Copied beside the AppImage first, so the move into place is a rename on one filesystem.
    let staged = target.with_extension("download");
    let result = std::fs::copy(&file, &staged)
        .and_then(|_| std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)))
        .and_then(|()| std::fs::rename(&staged, &target))
        .map_err(|e| format!("Could not replace {}: {e}", target.display()));
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    let _ = std::fs::remove_file(&file);
    result
}

/// Unpacks the new app bundle beside the running one, then swaps them. A bundle in a folder the
/// user cannot write to is not replaced.
#[cfg(target_os = "macos")]
fn install_bundle(archive: &std::path::Path) -> Res<()> {
    let bundle = app_bundle().ok_or("Not running from an app bundle.")?;
    let parent = bundle.parent().ok_or("The app bundle has no folder.")?;
    let staging = parent.join(".patchwork-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| {
        format!(
            "Patchwork cannot write to {}, so download the update from the releases page: {e}",
            parent.display()
        )
    })?;
    let result = (|| {
        let unpacked = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(&staging)
            .status()
            .map_err(|e| format!("Could not unpack the update: {e}"))?;
        if !unpacked.success() {
            return Err("Could not unpack the update.".to_string());
        }
        let new = staging.join("Patchwork.app");
        if !new.is_dir() {
            return Err("The update has no Patchwork.app.".to_string());
        }
        let old = staging.join("previous.app");
        std::fs::rename(&bundle, &old)
            .map_err(|e| format!("Could not replace {}: {e}", bundle.display()))?;
        if let Err(e) = std::fs::rename(&new, &bundle) {
            let _ = std::fs::rename(&old, &bundle);
            return Err(format!("Could not replace {}: {e}", bundle.display()));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}
