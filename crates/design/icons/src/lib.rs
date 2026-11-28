//! Packed, compressed SVG icon pool and compile-time typed [`IconName`] registry.
//!
//! At build time (`build.rs`), every SVG in `assets/icons/` (including any optional
//! `<name>.secondary.svg` duotone companion layer) is compressed with raw deflate and packed into
//! a single contiguous binary blob (`icons.pack`) indexed by a 16-byte-per-entry table
//! (`start_off`, `end_off`, `uncompressed_len`, `flags`).
//!
//! In development builds (`cfg(debug_assertions)`), [`load_icon_svg`] prefers reading live files
//! from `assets/icons/` on disk, so an edited icon shows on the next launch without a rebuild,
//! falling back to the packed blob whenever the source tree is unavailable.

// Rust guideline compliant 2026-10-09

mod icon;

pub use icon::{Icon, icon, icon_by_name, mono_svg};

use std::borrow::Cow;

include!(concat!(env!("OUT_DIR"), "/icon_data.rs"));

impl IconName {
    /// Whether this icon ships with a `.secondary.svg` duotone companion layer.
    #[must_use]
    pub const fn has_secondary(self) -> bool {
        (ENTRIES[self.entry_index()].flags & FLAG_HAS_SECONDARY) != 0
    }
}

/// Loads an icon's SVG bytes for an `AssetSource` path such as `"icons/check.svg"` (ignoring any
/// `?generation` cache-busting query suffix).
#[must_use]
pub fn load_icon_svg(path: &str) -> Option<Cow<'static, [u8]>> {
    let clean = path.split_once('?').map_or(path, |(base, _)| base);

    #[cfg(debug_assertions)]
    if let Some(rel) = clean.strip_prefix("icons/")
        && !rel.contains("..")
        && !rel.contains('\\')
        && let Ok(bytes) = std::fs::read(std::path::Path::new(DEV_ICONS_DIR).join(rel))
    {
        return Some(Cow::Owned(bytes));
    }

    let idx = PATHS.binary_search(&clean).ok()?;
    inflate_entry(ENTRIES[idx])
}

/// Loads an icon's SVG bytes directly from the packed binary blob, bypassing the dev disk read.
#[must_use]
pub fn load_packed_icon_svg(icon: IconName) -> Option<Cow<'static, [u8]>> {
    inflate_entry(ENTRIES[icon.entry_index()])
}

/// Lists all packed icon asset paths matching `prefix`.
#[must_use]
pub fn list_icon_paths(prefix: &str) -> Vec<&'static str> {
    PATHS
        .iter()
        .copied()
        .filter(|path| path.starts_with(prefix))
        .collect()
}

/// Total number of SVG entries (primary + secondary) in the packed binary pool.
#[must_use]
pub const fn packed_entry_count() -> usize {
    ENTRIES.len()
}

/// Compressed byte size of the embedded `icons.pack` payload pool.
#[must_use]
pub const fn pack_compressed_bytes() -> usize {
    PACK_DATA.len()
}

/// Total uncompressed byte size of all SVGs stored in `icons.pack`.
#[must_use]
pub fn pack_uncompressed_bytes() -> usize {
    ENTRIES
        .iter()
        .map(|entry| entry.uncompressed_len as usize)
        .sum()
}

fn inflate_entry(entry: PackEntry) -> Option<Cow<'static, [u8]>> {
    let slice = PACK_DATA.get(entry.start_off as usize..entry.end_off as usize)?;
    if (entry.flags & FLAG_COMPRESSED) == 0 {
        return Some(Cow::Borrowed(slice));
    }
    let inflated =
        miniz_oxide::inflate::decompress_to_vec_with_limit(slice, entry.uncompressed_len as usize)
            .ok()?;
    Some(Cow::Owned(inflated))
}
