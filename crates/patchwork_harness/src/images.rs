//! Attached images, read and encoded for an agent's input.

// Rust guideline compliant 2026-11-27

use patchwork_model::chats::{Attachment, AttachmentKind, image_media_type};
use patchwork_platform::Error;

/// An attached image, base64-encoded for the agent's input.
#[derive(Debug, Clone)]
pub struct Image {
    pub media_type: &'static str,
    pub data: String,
}

/// Largest image sent to an agent; the agents reject bigger ones.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Reads and encodes every image attachment.
///
/// # Errors
///
/// Returns an error when an image attachment is not a supported image type, cannot be read, or is
/// larger than 5 MB.
pub fn load_images(attachments: &[Attachment]) -> Result<Vec<Image>, Error> {
    use base64::Engine;
    attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image)
        .map(|a| {
            let media_type = image_media_type(&a.path).ok_or("Not an image")?;
            let size = std::fs::metadata(&a.path)
                .map_err(|e| format!("{}: {e}", a.name))?
                .len();
            if size > MAX_IMAGE_BYTES {
                return Err(format!("{} is larger than 5 MB", a.name).into());
            }
            let bytes = std::fs::read(&a.path).map_err(|e| format!("{}: {e}", a.name))?;
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok::<_, Error>(Image { media_type, data })
        })
        .collect()
}

/// An attachment for a path that exists on disk: a folder, an image or a file.
///
/// # Errors
///
/// Returns an error when the path cannot be read.
pub fn attachment(path: &str) -> Result<Attachment, Error> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    let kind = if meta.is_dir() {
        AttachmentKind::Folder
    } else if image_media_type(path).is_some() {
        AttachmentKind::Image
    } else {
        AttachmentKind::File
    };
    let name = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    Ok(Attachment {
        kind,
        path: path.to_string(),
        name,
    })
}
