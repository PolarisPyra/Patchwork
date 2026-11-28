//! Embeds `assets/logo.ico` in the Windows exe as icon resource 1, which GPUI loads for the
//! window and taskbar icon and Explorer shows for the exe, and the exe's version details. Other
//! targets skip it.
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
    // Explorer and security software read who made the exe and which version it is from here.
    let version = env::var("CARGO_PKG_VERSION")?;
    let numbers = version.replace('.', ",");
    fs::write(
        &rc,
        format!(
            r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers},0
PRODUCTVERSION {numbers},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "ProductName", "Patchwork"
      VALUE "FileDescription", "Patchwork"
      VALUE "InternalName", "patchwork"
      VALUE "OriginalFilename", "patchwork.exe"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
        ),
    )?;
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .map_err(|err| err.to_string())?;
    Ok(())
}
