//! A PDF's pages as images, drawn by Poppler's `pdftoppm` and kept in the cache until the file
//! changes.

// Rust guideline compliant 2026-10-09

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::Error;

/// Most pages drawn, so a long manual does not fill the cache.
const MAX_PAGES: u32 = 60;
/// Dots per inch pages are drawn at: sharp on a 2x screen at the canvas's usual width.
const PAGE_DPI: u32 = 110;

/// The pages of the PDF at `file` as PNGs, in order, drawn once per version of the file.
///
/// # Errors
///
/// When `pdftoppm` is not installed, or cannot read the file.
pub fn pdf_pages(file: &Path) -> Result<Vec<PathBuf>, Error> {
    let meta = std::fs::metadata(file).map_err(|e| format!("Could not read the PDF: {e}"))?;
    let mut hasher = DefaultHasher::new();
    file.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().hash(&mut hasher);
    let folder = crate::system::dirs()
        .cache
        .join("pdf-pages")
        .join(format!("{:016x}", hasher.finish()));
    let pages = drawn(&folder);
    if !pages.is_empty() {
        return Ok(pages);
    }
    crate::system::files::paths::make_folder(&folder)?;
    let output = crate::provider::user_command("pdftoppm")
        .args([
            "-png",
            "-r",
            &PAGE_DPI.to_string(),
            "-l",
            &MAX_PAGES.to_string(),
        ])
        .arg(file)
        .arg(folder.join("page"))
        .output()
        .map_err(|e| {
            format!(
                "Showing PDFs needs Poppler's pdftoppm. Install poppler, then reopen the file ({e})"
            )
        })?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(format!(
            "pdftoppm could not draw the PDF: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(drawn(&folder))
}

/// The pages already in `folder`, by page number.
fn drawn(folder: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    // `pdftoppm` pads page numbers to the same width, so names sort by page.
    pages.sort();
    pages
}
