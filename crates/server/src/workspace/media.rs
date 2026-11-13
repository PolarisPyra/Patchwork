//! What a file a reply produced looks like: an image to draw, source text the diff list keeps, or
//! a card. Paths are judged by name and size, and nothing is read but the metadata.

// Rust guideline compliant 2026-10-09

use std::path::{Component, Path};

use serde::Serialize;

use crate::Error;

/// Largest image shown as a thumbnail. A bigger one shows as a card, so a huge file cannot freeze
/// the window.
const MAX_PREVIEW_BYTES: u64 = 20 * 1024 * 1024;

/// Most paths one `file_media` call looks at. The rest of a long reply stays in the text list.
const MAX_MEDIA_PATHS: usize = 50;

/// How a file a reply produced is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    /// A thumbnail, for an image within `MAX_PREVIEW_BYTES`.
    Image,
    /// A video. It streams, so it has no size cap.
    Video,
    /// Source or config text. The diff list keeps these, so they get no card.
    Text,
    /// A card with the file's size and type.
    Other,
}

/// What a file a reply touched looks like now. `size` is 0 for a file that is gone. `absolute` is
/// the same file by its full path, which the window needs to load it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileMedia {
    pub path: String,
    pub absolute: String,
    pub size: u64,
    pub kind: PreviewKind,
}

/// Images shown as thumbnails. This is not `types::image_media_type`, which decides what a vision
/// model receives, and no provider takes SVG there.
fn is_previewable_image(ext: &str) -> bool {
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "ico"
    )
}

/// Videos, shown by kind.
fn is_playable_video(ext: &str) -> bool {
    matches!(ext, "mp4" | "m4v" | "webm" | "mov" | "ogv" | "mkv")
}

const SOURCE_TEXT: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "java", "kt", "swift", "c", "h",
    "cc", "cpp", "hpp", "cs", "rb", "php", "sh", "css", "scss", "json", "toml", "yaml", "yml",
    "lock",
];

/// How `rel` is shown, judged by its name and size. A file that is gone is judged by its name too,
/// so a deleted image shows as a placeholder and a deleted source file stays in the diff list.
fn preview_kind(rel: &str, size: u64) -> PreviewKind {
    let ext = Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_previewable_image(&ext) {
        if size <= MAX_PREVIEW_BYTES {
            PreviewKind::Image
        } else {
            PreviewKind::Other
        }
    } else if is_playable_video(&ext) {
        PreviewKind::Video
    } else if SOURCE_TEXT.contains(&ext.as_str()) {
        PreviewKind::Text
    } else {
        PreviewKind::Other
    }
}

/// The media of one path. `root` is the canonical project root, for checks that resolve links, and
/// `base` the project root as it is stored, for the full path. A path that is not a plain file
/// inside the project, or that sits under `.git`, is never previewed, so it reports as `Other`
/// with no size.
fn media_of(root: &Path, base: &Path, rel: &str) -> FileMedia {
    let rel_path = Path::new(rel);
    let absolute = base.join(rel_path).to_string_lossy().into_owned();
    let media = |size, kind| FileMedia {
        path: rel.to_string(),
        absolute: absolute.clone(),
        size,
        kind,
    };
    let plain = rel_path
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    let in_git = rel_path.components().any(|c| c.as_os_str() == ".git");
    if !plain || in_git {
        return media(0, PreviewKind::Other);
    }
    let abs = root.join(rel_path);
    match std::fs::metadata(&abs) {
        Ok(meta) if meta.is_dir() => media(0, PreviewKind::Other),
        Ok(meta) if std::fs::canonicalize(&abs).is_ok_and(|p| p.starts_with(root)) => {
            media(meta.len(), preview_kind(rel, meta.len()))
        }
        Ok(_) => media(0, PreviewKind::Other),
        Err(_) => media(0, preview_kind(rel, 0)),
    }
}

/// What each of `rels` is shown as, for a reply's cards. Paths past `MAX_MEDIA_PATHS` are left out,
/// so they fall back to the text list.
///
/// # Errors
///
/// Returns an error when the project root cannot be resolved.
pub fn file_media(root: &str, rels: &[String]) -> Result<Vec<FileMedia>, Error> {
    if let Some(at) = crate::system::remote::split(root) {
        return remote_media(root, at, rels);
    }
    let canonical = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    Ok(rels
        .iter()
        .take(MAX_MEDIA_PATHS)
        .map(|rel| media_of(&canonical, Path::new(root), rel))
        .collect())
}

/// [`file_media`] for a project on an SSH host. Its pictures and videos are not on this machine
/// to show, so they list as other files; text still opens in the diff list.
fn remote_media(
    root: &str,
    at: crate::system::remote::Location<'_>,
    rels: &[String],
) -> Result<Vec<FileMedia>, Error> {
    let rels: Vec<String> = rels.iter().take(MAX_MEDIA_PATHS).cloned().collect();
    let sizes = crate::system::remote::fs::sizes(at.host, at.path, &rels)?;
    Ok(rels
        .into_iter()
        .zip(sizes)
        .map(|(rel, size)| {
            let rel_path = Path::new(&rel);
            let plain = rel_path
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
            let in_git = rel_path.components().any(|c| c.as_os_str() == ".git");
            let kind = match preview_kind(&rel, size.unwrap_or(0)) {
                PreviewKind::Text if plain && !in_git => PreviewKind::Text,
                _ => PreviewKind::Other,
            };
            FileMedia {
                absolute: crate::system::remote::join(root, &rel),
                path: rel,
                size: size.unwrap_or(0),
                kind,
            }
        })
        .collect())
}
