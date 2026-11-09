//! Chat backgrounds: the one every chat shows, kept in Settings, a chat's own, and the pictures
//! picked for them, which Patchwork keeps in its own folder.

use std::path::{Path, PathBuf};

use image::{GenericImageView, ImageFormat, imageops::FilterType};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::{Res, Server};

/// What shows behind a chat's messages.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatBackground {
    /// The plain window color.
    #[default]
    None,
    /// One of the backgrounds the app comes with, by its id in the web app's list.
    Preset { id: String },
    /// A picture in Patchwork's backgrounds folder, by its file name there.
    Image {
        #[serde(alias = "path", deserialize_with = "file_name")]
        file: String,
    },
}

/// How much of a background shows through the window color laid over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundStrength {
    Subtle,
    #[default]
    Medium,
    Strong,
}

/// A picture in Patchwork's backgrounds folder.
#[derive(Debug, Clone, Serialize)]
pub struct BackgroundImage {
    /// Its file name, as a `ChatBackground::Image` names it.
    pub file: String,
    /// Where it is, for the webview to load it.
    pub path: String,
    /// The name of the file it was copied from, without its extension. None for a picture saved
    /// before names were kept.
    pub name: Option<String>,
}

/// A deleted picture, and what showed it, so the deletion can be taken back until Patchwork
/// closes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedBackground {
    pub file: String,
    /// The chats that showed it as their own background.
    pub threads: Vec<String>,
    /// It was the background every chat shares.
    pub shared: bool,
}

/// The longest side a picture is kept at. A background is mostly covered by the window color, so
/// a larger one only costs memory each time a chat shows it.
const MAX_SIDE: u32 = 2560;

/// The `app_state` key of the name a picture was picked by.
fn name_key(file: &str) -> String {
    format!("background-name:{file}")
}

impl Server {
    /// Gives a chat a background of its own, or None to show the one every chat shares.
    pub fn set_thread_background(&self, id: &str, background: Option<ChatBackground>) -> Res<()> {
        self.store
            .update_thread(id, |t| t.background = background.clone())
    }

    /// Copies a picture into Patchwork's folder, so the background stays when the original is
    /// moved or deleted. Returns the copy's file name.
    pub fn import_background_image(&self, path: &str) -> Res<String> {
        let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {path}: {e}"))?;
        let name = Path::new(path).file_stem().and_then(|n| n.to_str());
        self.import_background_bytes(bytes, name)
    }

    /// Saves a picture's bytes in Patchwork's folder, shrunk when it is larger than `MAX_SIDE`.
    /// The copy is named after the bytes, so the same picture picked again is kept once; `name`
    /// is what the picker calls it. Returns the copy's file name.
    pub fn import_background_bytes(&self, bytes: Vec<u8>, name: Option<&str>) -> Res<String> {
        let not_picture = || "That is not a PNG, JPEG, WebP or GIF picture".to_owned();
        let format = image::guess_format(&bytes).map_err(|_| not_picture())?;
        if ![
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Gif,
        ]
        .contains(&format)
        {
            return Err(not_picture());
        }
        let hash: String = Sha256::digest(&bytes)[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        std::fs::create_dir_all(&self.backgrounds_dir).map_err(|e| e.to_string())?;
        let existing = self.background_files()?.into_iter().find(|f| {
            Path::new(f)
                .file_stem()
                .is_some_and(|stem| stem == hash.as_str())
        });
        let file = match existing {
            Some(file) => file,
            None => {
                // A GIF is kept as it is: shrinking it would keep only its first frame.
                let (bytes, format) = if format == ImageFormat::Gif {
                    (bytes, format)
                } else {
                    shrink(bytes, format).map_err(|e| format!("couldn't read the picture: {e}"))?
                };
                let extension = format.extensions_str().first().copied().unwrap_or("img");
                let file = format!("{hash}.{extension}");
                std::fs::write(self.backgrounds_dir.join(&file), bytes)
                    .map_err(|e| format!("couldn't save the picture: {e}"))?;
                file
            }
        };
        if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty())
            && self.store.state(&name_key(&file)).is_none()
        {
            self.store.set_state(&name_key(&file), name)?;
        }
        Ok(file)
    }

    /// The pictures in Patchwork's backgrounds folder, oldest first.
    pub fn list_background_images(&self) -> Res<Vec<BackgroundImage>> {
        Ok(self
            .background_files()?
            .into_iter()
            .map(|file| BackgroundImage {
                path: self
                    .backgrounds_dir
                    .join(&file)
                    .to_string_lossy()
                    .into_owned(),
                name: self.store.state(&name_key(&file)),
                file,
            })
            .collect())
    }

    /// Deletes a picture from Patchwork's folder. The chats that showed it go back to the shared
    /// background, and the shared background, if it was this picture, to the plain window color.
    /// The picture waits in a folder of its own until Patchwork next opens, so
    /// `restore_background_image` can take the deletion back.
    pub fn remove_background_image(&self, file: &str) -> Res<RemovedBackground> {
        // A name with a folder in it could reach a file outside the backgrounds folder.
        if Path::new(file).file_name().and_then(|n| n.to_str()) != Some(file) {
            return Err(format!("{file} is not a background picture"));
        }
        let removed_dir = self.removed_backgrounds_dir();
        std::fs::create_dir_all(&removed_dir).map_err(|e| e.to_string())?;
        std::fs::rename(self.backgrounds_dir.join(file), removed_dir.join(file))
            .map_err(|e| format!("couldn't delete the picture: {e}"))?;
        let picture = ChatBackground::Image {
            file: file.to_owned(),
        };
        let threads: Vec<String> = self.store.with_threads(|threads| {
            threads
                .iter()
                .filter(|t| t.background.as_ref() == Some(&picture))
                .map(|t| t.id.clone())
                .collect()
        });
        for id in &threads {
            self.store.update_thread(id, |t| t.background = None)?;
        }
        let shared = self.store.settings().chat_background == picture;
        if shared {
            self.store
                .update_settings(|s| s.chat_background = ChatBackground::None)?;
        }
        Ok(RemovedBackground {
            file: file.to_owned(),
            threads,
            shared,
        })
    }

    /// Takes back `remove_background_image`: the picture returns, and so does every background it
    /// was, unless another background was picked there since.
    pub fn restore_background_image(&self, removed: &RemovedBackground) -> Res<()> {
        let file = removed.file.as_str();
        if Path::new(file).file_name().and_then(|n| n.to_str()) != Some(file) {
            return Err(format!("{file} is not a background picture"));
        }
        std::fs::rename(
            self.removed_backgrounds_dir().join(file),
            self.backgrounds_dir.join(file),
        )
        .map_err(|e| format!("couldn't bring the picture back: {e}"))?;
        let picture = ChatBackground::Image {
            file: file.to_owned(),
        };
        for id in &removed.threads {
            self.store.update_thread(id, |t| {
                if t.background.is_none() {
                    t.background = Some(picture.clone());
                }
            })?;
        }
        if removed.shared && self.store.settings().chat_background == ChatBackground::None {
            self.store
                .update_settings(|s| s.chat_background = picture.clone())?;
        }
        Ok(())
    }

    /// Deletes for good the pictures deleted while Patchwork last ran, with their names.
    pub(crate) fn purge_removed_backgrounds(&self) {
        let dir = self.removed_backgrounds_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(file) = entry.file_name().into_string() else {
                continue;
            };
            // The same picture picked again since keeps its name.
            if !self.backgrounds_dir.join(&file).exists()
                && let Err(e) = self.store.clear_state(&name_key(&file))
            {
                log::warn!("couldn't forget the name of background {file}: {e}");
            }
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            log::warn!("couldn't delete the removed backgrounds: {e}");
        }
    }

    fn removed_backgrounds_dir(&self) -> PathBuf {
        self.backgrounds_dir.join("removed")
    }

    /// The file names in the backgrounds folder, oldest first. None yet when it doesn't exist.
    fn background_files(&self) -> Res<Vec<String>> {
        let entries = match std::fs::read_dir(&self.backgrounds_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string()),
        };
        let mut files: Vec<(std::time::SystemTime, String)> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| {
                let modified = e.metadata().and_then(|m| m.modified()).ok()?;
                Some((modified, e.file_name().into_string().ok()?))
            })
            .collect();
        files.sort();
        Ok(files.into_iter().map(|(_, file)| file).collect())
    }
}

/// Keeps a picture's bytes as they are when it fits in `MAX_SIDE`, else scales it down to fit and
/// saves it again: as a JPEG, or a PNG when it has see-through parts.
fn shrink(bytes: Vec<u8>, format: ImageFormat) -> Result<(Vec<u8>, ImageFormat), String> {
    let picture = image::load_from_memory_with_format(&bytes, format).map_err(|e| e.to_string())?;
    let (width, height) = picture.dimensions();
    if width.max(height) <= MAX_SIDE {
        return Ok((bytes, format));
    }
    let picture = picture.resize(MAX_SIDE, MAX_SIDE, FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::new());
    let format = if picture.color().has_alpha() {
        picture
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        ImageFormat::Png
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88)
            .encode_image(&picture.to_rgb8())
            .map_err(|e| e.to_string())?;
        ImageFormat::Jpeg
    };
    Ok((out.into_inner(), format))
}

/// Keeps only the file name of a picture, so a background saved with its whole path, before
/// pictures were kept by name, still finds it.
fn file_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    Ok(Path::new(&value)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&value)
        .to_owned())
}
