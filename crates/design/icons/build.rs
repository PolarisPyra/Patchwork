//! Build-time SVG icon packer and `IconName` code generator.
//!
//! Reads every `*.svg` under `assets/icons/`, detects optional `<name>.secondary.svg` duotone
//! companion layers, compresses each SVG payload with raw deflate (`miniz_oxide`), packs all
//! payloads into a single binary blob (`OUT_DIR/icons.pack`) with a 16-byte-per-entry index table
//! (`start_off`, `end_off`, `uncompressed_len`, `flags`), and emits `OUT_DIR/icon_data.rs` with
//! the compile-time typed `IconName` enum and `O(log N)` lookup tables.
//!
//! Regeneration is automatic whenever `assets/icons/` changes.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const FLAG_COMPRESSED: u32 = 1 << 0;
const FLAG_HAS_SECONDARY: u32 = 1 << 1;

struct PackedEntry {
    asset_path: String,
    start_off: u32,
    end_off: u32,
    uncompressed_len: u32,
    flags: u32,
}

fn locate_icons_dir() -> Result<PathBuf, Box<dyn Error>> {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../../assets/icons");
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(format!("could not find {}", dir.display()).into())
    }
}

fn kebab_to_pascal(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    for part in stem.split(['-', '_']) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            for upper in first.to_uppercase() {
                out.push(upper);
            }
            out.push_str(chars.as_str());
        }
    }
    out
}

fn read_svg_files(icons_dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, Box<dyn Error>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(icons_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        files.insert(name.to_owned(), fs::read(&path)?);
    }
    Ok(files)
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");

    let icons_dir = locate_icons_dir()?;
    println!("cargo:rerun-if-changed={}", icons_dir.display());

    let svg_files = read_svg_files(&icons_dir)?;
    if svg_files.is_empty() {
        return Err(format!("no .svg files found in {}", icons_dir.display()).into());
    }

    let mut primaries: Vec<String> = Vec::new();
    let mut secondaries: BTreeSet<String> = BTreeSet::new();
    for filename in svg_files.keys() {
        if let Some(stem) = filename.strip_suffix(".secondary.svg") {
            secondaries.insert(stem.to_owned());
        } else if let Some(stem) = filename.strip_suffix(".svg") {
            primaries.push(stem.to_owned());
        }
    }

    // Verify every `.secondary.svg` companion belongs to an existing primary icon.
    let primary_set: BTreeSet<&str> = primaries.iter().map(String::as_str).collect();
    for sec in &secondaries {
        if !primary_set.contains(sec.as_str()) {
            return Err(format!(
                "orphaned duotone layer `{sec}.secondary.svg`: missing `{sec}.svg`"
            )
            .into());
        }
    }

    // Pack all SVG payloads in sorted asset_path order so `PATHS` supports binary search.
    let mut pack_bytes: Vec<u8> = Vec::new();
    let mut entries: Vec<PackedEntry> = Vec::with_capacity(svg_files.len());
    let mut stem_to_entry_idx: BTreeMap<String, usize> = BTreeMap::new();

    for (filename, raw) in &svg_files {
        let asset_path = format!("icons/{filename}");
        let is_secondary = filename.ends_with(".secondary.svg");
        let primary_stem = filename.strip_suffix(".svg").filter(|_| !is_secondary);
        let has_secondary = primary_stem.is_some_and(|stem| secondaries.contains(stem));

        let compressed = miniz_oxide::deflate::compress_to_vec(raw, 8);
        let (payload, mut flags) = if compressed.len() < raw.len() {
            (compressed.as_slice(), FLAG_COMPRESSED)
        } else {
            (raw.as_slice(), 0)
        };
        if has_secondary {
            flags |= FLAG_HAS_SECONDARY;
        }

        let start_off = u32::try_from(pack_bytes.len())?;
        pack_bytes.extend_from_slice(payload);
        let end_off = u32::try_from(pack_bytes.len())?;
        let uncompressed_len = u32::try_from(raw.len())?;

        let idx = entries.len();
        entries.push(PackedEntry {
            asset_path,
            start_off,
            end_off,
            uncompressed_len,
            flags,
        });
        if let Some(stem) = primary_stem {
            stem_to_entry_idx.insert(stem.to_owned(), idx);
        }
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(out_dir.join("icons.pack"), &pack_bytes)?;

    let mut code = String::with_capacity(64 * 1024);
    code.push_str("// Generated by patchwork-design-icons/build.rs — do not edit.\n\n");

    let canonical_dir = fs::canonicalize(&icons_dir).unwrap_or(icons_dir);
    let _ = writeln!(
        code,
        "#[cfg(debug_assertions)]\npub(crate) const DEV_ICONS_DIR: &str = {:?};",
        canonical_dir.to_string_lossy()
    );
    code.push_str(
        "pub(crate) const PACK_DATA: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/icons.pack\"));\n\n",
    );
    let _ = writeln!(
        code,
        "pub(crate) const FLAG_COMPRESSED: u32 = {FLAG_COMPRESSED};"
    );
    let _ = writeln!(
        code,
        "pub(crate) const FLAG_HAS_SECONDARY: u32 = {FLAG_HAS_SECONDARY};\n"
    );

    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    code.push_str("pub(crate) struct PackEntry {\n");
    code.push_str("    pub(crate) start_off: u32,\n");
    code.push_str("    pub(crate) end_off: u32,\n");
    code.push_str("    pub(crate) uncompressed_len: u32,\n");
    code.push_str("    pub(crate) flags: u32,\n");
    code.push_str("}\n\n");

    code.push_str("pub(crate) const PATHS: &[&str] = &[\n");
    for entry in &entries {
        let _ = writeln!(code, "    {:?},", entry.asset_path);
    }
    code.push_str("];\n\n");

    code.push_str("pub(crate) const ENTRIES: &[PackEntry] = &[\n");
    for entry in &entries {
        let _ = writeln!(
            code,
            "    PackEntry {{ start_off: {}, end_off: {}, uncompressed_len: {}, flags: {} }},",
            entry.start_off, entry.end_off, entry.uncompressed_len, entry.flags
        );
    }
    code.push_str("];\n\n");

    let mut variants: Vec<(String, String, bool, usize)> = Vec::with_capacity(primaries.len());
    let mut seen_variants = BTreeSet::new();
    for stem in &primaries {
        let variant = kebab_to_pascal(stem);
        if !seen_variants.insert(variant.clone()) {
            return Err(format!("duplicate IconName variant `{variant}` for `{stem}`").into());
        }
        let has_sec = secondaries.contains(stem);
        let entry_idx = stem_to_entry_idx[stem];
        variants.push((variant, stem.clone(), has_sec, entry_idx));
    }

    code.push_str("/// Compile-time typed identifier for every SVG icon in `assets/icons/`.\n");
    code.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n");
    code.push_str("pub enum IconName {\n");
    for (variant, stem, _, _) in &variants {
        let _ = writeln!(code, "    /// `assets/icons/{stem}.svg`");
        let _ = writeln!(code, "    {variant},");
    }
    code.push_str("}\n\n");

    code.push_str("impl IconName {\n");
    code.push_str("    /// Every icon compiled into the packed asset table.\n");
    code.push_str("    pub const ALL: &'static [Self] = &[\n");
    for (variant, _, _, _) in &variants {
        let _ = writeln!(code, "        Self::{variant},");
    }
    code.push_str("    ];\n\n");

    code.push_str("    /// Kebab-case stem of the icon (without `.svg`).\n");
    code.push_str("    #[must_use]\n");
    code.push_str("    pub const fn kebab_name(self) -> &'static str {\n");
    code.push_str("        match self {\n");
    for (variant, stem, _, _) in &variants {
        let _ = writeln!(code, "            Self::{variant} => {stem:?},");
    }
    code.push_str("        }\n");
    code.push_str("    }\n\n");

    code.push_str(
        "    /// Asset path served through GPUI's `AssetSource` (`icons/<kebab>.svg`).\n",
    );
    code.push_str("    #[must_use]\n");
    code.push_str("    pub const fn asset_path(self) -> &'static str {\n");
    code.push_str("        match self {\n");
    for (variant, stem, _, _) in &variants {
        let path = format!("icons/{stem}.svg");
        let _ = writeln!(code, "            Self::{variant} => {path:?},");
    }
    code.push_str("        }\n");
    code.push_str("    }\n\n");

    code.push_str(
        "    /// Optional secondary duotone layer path (`icons/<kebab>.secondary.svg`), if present.\n",
    );
    code.push_str("    #[must_use]\n");
    code.push_str("    pub const fn secondary_asset_path(self) -> Option<&'static str> {\n");
    if secondaries.is_empty() {
        code.push_str("        let _ = self;\n");
        code.push_str("        None\n");
    } else {
        code.push_str("        match self {\n");
        for (variant, stem, has_sec, _) in &variants {
            if *has_sec {
                let path = format!("icons/{stem}.secondary.svg");
                let _ = writeln!(code, "            Self::{variant} => Some({path:?}),");
            }
        }
        code.push_str("            _ => None,\n");
        code.push_str("        }\n");
    }
    code.push_str("    }\n\n");

    code.push_str("    /// Index of this icon's primary payload in the packed entry table.\n");
    code.push_str("    #[must_use]\n");
    code.push_str("    pub(crate) const fn entry_index(self) -> usize {\n");
    code.push_str("        match self {\n");
    for (variant, _, _, idx) in &variants {
        let _ = writeln!(code, "            Self::{variant} => {idx},");
    }
    code.push_str("        }\n");
    code.push_str("    }\n\n");

    code.push_str("    /// Resolves a kebab-case icon name at runtime.\n");
    code.push_str("    #[must_use]\n");
    code.push_str("    pub fn from_kebab(name: &str) -> Option<Self> {\n");
    code.push_str("        match name {\n");
    for (variant, stem, _, _) in &variants {
        let _ = writeln!(code, "            {stem:?} => Some(Self::{variant}),");
    }
    code.push_str("            _ => None,\n");
    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    fs::write(out_dir.join("icon_data.rs"), code)?;
    Ok(())
}
