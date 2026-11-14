//! Chat backgrounds: the one every chat shows, kept in Settings, a chat's own, and the pictures
//! picked for them or drawn by an agent, which Patchwork keeps in its own folder.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use image::{GenericImageView, ImageFormat, imageops::FilterType};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Server};

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
    /// Where it is, for the window to load it.
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

/// The longest side of a picture's blurred copy, and how far it is blurred. A blurred picture
/// has no detail to lose, and a small one is quick to blur and to stretch.
const BLUR_SIDE: u32 = 640;

/// The longest side of the picture made from a background an agent drew.
const DRAWN_SIDE: f32 = 1920.;

/// What an agent is told to draw a chat background with.
const DRAWING_FORMAT: &str = r##"A chat background is one SVG file, which Patchwork turns into a picture:

<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1600 1000">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#1b2a5c"/>
      <stop offset="1" stop-color="#5a3f7a"/>
    </linearGradient>
  </defs>
  <rect width="1600" height="1000" fill="url(#sky)"/>
  <circle cx="1240" cy="260" r="90" fill="#f3d9a4" opacity="0.5"/>
</svg>

Rules:
- A viewBox of 0 0 1600 1000, and a shape that fills all of it first, so no part is see-through.
- Shapes, paths, gradients, patterns and filters only: no text, no pictures, no scripts, no
  links to other files, no animation.
- Messages are read over it, under a layer of the window's color. Keep it soft and low in
  contrast, with large shapes rather than fine detail, and nothing that looks like writing.
- It is stretched to cover chats of any shape, so keep what matters away from the edges."##;

/// Whether `id` can name a drawing: lowercase letters, digits and `-`.
fn is_drawing(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
}

/// The `app_state` key of the name a picture was picked by.
fn name_key(file: &str) -> String {
    format!("background-name:{file}")
}

impl Server {
    /// Gives a chat a background of its own, or None to show the one every chat shares.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat's changes cannot be saved to the database.
    pub fn set_thread_background(
        &self,
        id: &str,
        background: Option<ChatBackground>,
    ) -> Result<(), Error> {
        self.store
            .update_thread(id, move |t| t.background = background)
    }

    /// Copies a picture into Patchwork's folder, so the background stays when the original is
    /// moved or deleted. Returns the copy's file name.
    ///
    /// # Errors
    ///
    /// Returns an error when the picture cannot be read, or for any reason
    /// `import_background_bytes` fails.
    pub fn import_background_image(&self, path: &str) -> Result<String, Error> {
        let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {path}: {e}"))?;
        let name = Path::new(path).file_stem().and_then(|n| n.to_str());
        self.import_background_bytes(bytes, name)
    }

    /// Saves a picture's bytes in Patchwork's folder, shrunk when it is larger than `MAX_SIDE`.
    /// The copy is named after the bytes, so the same picture picked again is kept once; `name`
    /// is what the picker calls it. Returns the copy's file name.
    ///
    /// # Errors
    ///
    /// Returns an error when the bytes are not a PNG, JPEG, WebP or GIF picture, or when the
    /// backgrounds folder or the copy cannot be written or the picture cannot be shrunk.
    pub fn import_background_bytes(
        &self,
        bytes: Vec<u8>,
        name: Option<&str>,
    ) -> Result<String, Error> {
        let not_picture = || "That is not a PNG, JPEG, WebP or GIF picture".to_owned();
        let format = image::guess_format(&bytes).map_err(|_discarded| not_picture())?;
        if ![
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::WebP,
            ImageFormat::Gif,
        ]
        .contains(&format)
        {
            return Err(not_picture().into());
        }
        let hash: String =
            Sha256::digest(&bytes)[..16]
                .iter()
                .fold(String::new(), |mut hash, b| {
                    let _ = write!(hash, "{b:02x}");
                    hash
                });
        std::fs::create_dir_all(&self.backgrounds_dir).map_err(|e| e.to_string())?;
        let existing = self.background_files()?.into_iter().find(|f| {
            Path::new(f)
                .file_stem()
                .is_some_and(|stem| stem == hash.as_str())
        });
        let file = if let Some(file) = existing {
            file
        } else {
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
        };
        if let Some(name) = name.map(str::trim).filter(|n| !n.is_empty())
            && self.store.state(&name_key(&file)).is_none()
        {
            self.store.set_state(&name_key(&file), name)?;
        }
        Ok(file)
    }

    /// The folder the background pictures are in.
    #[must_use]
    pub fn backgrounds_dir(&self) -> PathBuf {
        self.backgrounds_dir.clone()
    }

    /// The message that has an agent draw a chat background of `description`, and the id its
    /// drawing will have.
    #[must_use]
    pub fn chat_background_prompt(&self, description: &str) -> (String, String) {
        let mut slug = String::new();
        for c in description.trim().chars().take(40) {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        let slug = if slug.is_empty() { "background" } else { slug };
        let dir = self.drawings_dir();
        let id = std::iter::once(slug.to_owned())
            .chain((2..100).map(|n| format!("{slug}-{n}")))
            .find(|id| !dir.join(format!("{id}.svg")).exists())
            .unwrap_or_else(|| slug.to_owned());
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!(error:% = e; "could not make {}: {{error}}", dir.display());
        }
        let prompt = format!(
            "Draw a Patchwork chat background: {description}\n\nSave it as {} and write nothing \
             else. Read the file back and check it is valid SVG before you finish.\n\n\
             {DRAWING_FORMAT}",
            dir.join(format!("{id}.svg")).display()
        );
        (prompt, id)
    }

    /// Whether an agent has saved the drawing with this id.
    #[must_use]
    pub fn chat_background_drawn(&self, id: &str) -> bool {
        is_drawing(id) && self.drawings_dir().join(format!("{id}.svg")).is_file()
    }

    /// Turns the drawing with this id into a picture in Patchwork's folder, named after it, and
    /// deletes the drawing. Returns the picture's file name.
    ///
    /// # Errors
    ///
    /// Returns an error when there is no such drawing, when it is not an SVG that can be drawn,
    /// or for any reason `import_background_bytes` fails.
    pub fn import_drawn_background(&self, id: &str) -> Result<String, Error> {
        if !self.chat_background_drawn(id) {
            return Err("The chat has not drawn a background.".into());
        }
        let path = self.drawings_dir().join(format!("{id}.svg"));
        let unusable = || "The background the chat drew cannot be read.".to_owned();
        let text = std::fs::read_to_string(&path).map_err(|_discarded| unusable())?;
        let tree = resvg::usvg::Tree::from_str(&text, &resvg::usvg::Options::default())
            .map_err(|_discarded| unusable())?;
        let size = tree.size();
        let scale = DRAWN_SIDE / size.width().max(size.height());
        let scaled = size.to_int_size().scale_by(scale).ok_or_else(unusable)?;
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(scaled.width(), scaled.height()).ok_or_else(unusable)?;
        let fit = resvg::tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, fit, &mut pixmap.as_mut());
        let png = pixmap.encode_png().map_err(|_discarded| unusable())?;
        let file = self.import_background_bytes(png, Some(&id.replace('-', " ")))?;
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!(error:% = e; "couldn't delete the drawing {}: {{error}}", path.display());
        }
        Ok(file)
    }

    /// A small copy of a picture in the backgrounds folder, blurred to `level`, made once and
    /// kept in the cache folder. None when the picture cannot be read, as a GIF cannot.
    pub(crate) fn blurred_background(&self, file: &str, level: u8) -> Option<PathBuf> {
        let folder = crate::system::dirs().cache.join("blurred-backgrounds");
        let copy = folder.join(format!("{file}-{level}.png"));
        if copy.is_file() {
            return Some(copy);
        }
        let picture = image::open(self.backgrounds_dir.join(file)).ok()?;
        let soft = picture
            .thumbnail(BLUR_SIDE, BLUR_SIDE)
            .blur(f32::from(level));
        std::fs::create_dir_all(&folder).ok()?;
        soft.save(&copy).ok()?;
        Some(copy)
    }

    /// The pictures in Patchwork's backgrounds folder, oldest first.
    ///
    /// # Errors
    ///
    /// Returns an error when the backgrounds folder cannot be read.
    pub fn list_background_images(&self) -> Result<Vec<BackgroundImage>, Error> {
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
    ///
    /// # Errors
    ///
    /// Returns an error when `file` names a path rather than a plain file name, when the picture
    /// cannot be moved aside, or when a chat or the settings cannot be saved.
    pub fn remove_background_image(&self, file: &str) -> Result<RemovedBackground, Error> {
        // A name with a folder in it could reach a file outside the backgrounds folder.
        if Path::new(file).file_name().and_then(|n| n.to_str()) != Some(file) {
            return Err(format!("{file} is not a background picture").into());
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
    ///
    /// # Errors
    ///
    /// Returns an error when the file name contains a path, when the picture cannot be moved back
    /// into the backgrounds folder, or when a chat or the settings cannot be saved.
    pub fn restore_background_image(&self, removed: &RemovedBackground) -> Result<(), Error> {
        let file = removed.file.as_str();
        if Path::new(file).file_name().and_then(|n| n.to_str()) != Some(file) {
            return Err(format!("{file} is not a background picture").into());
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
                log::warn!(
                    file:%, error:% = e;
                    "couldn't forget the name of background {{file}}: {{error}}"
                );
            }
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            log::warn!(error:% = e; "couldn't delete the removed backgrounds: {{error}}");
        }
    }

    /// Where agents save the chat backgrounds they draw, until each becomes a picture.
    #[expect(
        clippy::unused_self,
        reason = "it reads as the other folders of a background do"
    )]
    fn drawings_dir(&self) -> PathBuf {
        crate::system::dirs().cache.join("background-drawings")
    }

    fn removed_backgrounds_dir(&self) -> PathBuf {
        self.backgrounds_dir.join("removed")
    }

    /// The file names in the backgrounds folder, oldest first. None yet when it doesn't exist.
    fn background_files(&self) -> Result<Vec<String>, Error> {
        let entries = match std::fs::read_dir(&self.backgrounds_dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.to_string().into()),
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
fn shrink(bytes: Vec<u8>, format: ImageFormat) -> Result<(Vec<u8>, ImageFormat), Error> {
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
