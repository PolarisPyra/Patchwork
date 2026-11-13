//! Embeds `assets/logo.ico` in the Windows exe as icon resource 1, which GPUI loads for the
//! window and taskbar icon and Explorer shows for the exe. Other targets skip it.
//!
//! `assets/logo.ico` is made from `assets/logo.png`; after changing the logo, regenerate it from
//! the repo root with:
//!
//! ```sh
//! magick assets/logo.png -define icon:auto-resize=256,128,64,48,32,24,16 assets/logo.ico
//! ```

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    // Without one of these, cargo runs this again whenever any file of the crate changes.
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let icon = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../assets/logo.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let icon = icon.display().to_string().replace('\\', "/");
    let rc = PathBuf::from(env::var("OUT_DIR")?).join("patchwork.rc");
    fs::write(&rc, format!("1 ICON \"{icon}\"\n"))?;
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .map_err(|err| err.to_string())?;
    Ok(())
}
