//! What goes to chat apps as a file: a reply too long to read as messages, the diff of what a
//! reply changed, a picture, and a screenshot of the screen.

// Rust guideline compliant 2026-10-09

use image::imageops::FilterType;

use super::net::{chunks, mime};
use crate::provider::clip;
use super::platform::Platform;
use crate::{Core, Error};
use crate::orchestration::changes::turn_files::turn_file_diff;
use crate::chats::Role;

/// The most messages a reply is split into before it is sent as a file instead.
pub(super) const MESSAGES: usize = 2;

/// The most characters of a reply's opening shown beside its file.
const CAPTION_LIMIT: usize = 300;

/// The most characters of a diff shown as a message where the chat app takes no files.
const DIFF_TEXT_LIMIT: usize = 6000;

/// What a chat app is told when it asks for a screenshot while they are off.
pub(super) const SCREENSHOTS_OFF: &str = "Screenshots are off. Turn on \u{201c}Let the agent see the screen\u{201d} in Settings → Permissions on the computer";

/// The largest file Patchwork sends to a chat app, in bytes. Discord takes 10 MiB from a bot and
/// Telegram 10 MB as a photo; staying under both lets every connector take it whole.
pub(crate) const FILE_LIMIT: usize = 8 * 1024 * 1024;

/// The quality a picture over `FILE_LIMIT` is saved again at, as a JPEG: high enough that the
/// text in a screenshot stays readable.
const JPEG_QUALITY: u8 = 85;

/// The longest side a picture is scaled down to when it is over `FILE_LIMIT` even as a JPEG.
/// Telegram shows photos at most this wide.
const SMALLER_SIDE: u32 = 2560;

impl Core {
    /// Sends `text` to `chat`, as the file `reply.md` with its opening beside it when it would
    /// take more than a couple of messages.
    pub(super) fn say_long(&self, platform: Platform, conversation: &str, text: &str) {
        if chunks(text, platform.limit()).len() <= MESSAGES {
            self.say(platform, conversation, text);
            return;
        }
        let opening = text.trim().split("\n\n").next().unwrap_or_default();
        let caption = clip(opening, CAPTION_LIMIT);
        if !self.say_file(platform, conversation, "reply.md", text, &caption) {
            self.say(platform, conversation, text);
        }
    }

    /// Sends the diff of the files the latest reply of `chat_id` changed, as `changes.patch`,
    /// and returns what to tell the chat when there is none to send.
    pub(crate) fn send_diff(
        &self,
        platform: Platform,
        conversation: &str,
        chat_id: Option<&str>,
    ) -> Option<String> {
        let Some(chat) = chat_id.and_then(|id| self.store.chat(id)) else {
            return Some("No chat yet. Your next message starts one.".to_string());
        };
        let Some(root) = chat
            .project_id
            .as_deref()
            .and_then(|id| self.project_root(id).ok())
        else {
            return Some("This chat is in no project, so it changes no files.".to_string());
        };
        let Some((turn, files)) = chat
            .messages
            .iter()
            .rev()
            .filter(|m| m.role == Role::Assistant && !m.files.is_empty())
            .find_map(|m| Some((m.turn.as_ref()?, &m.files)))
        else {
            return Some("No reply in this chat changed files.".to_string());
        };
        let mut diff = String::new();
        for path in files {
            match turn_file_diff(&root, turn, path) {
                Ok(file) => diff.push_str(&file),
                Err(error) => {
                    log::warn!(error:% = error; "could not diff a file for a chat app: {{error}}");
                }
            }
        }
        if diff.trim().is_empty() {
            return Some("Patchwork could not read the changes.".to_string());
        }
        let count = if files.len() == 1 {
            "1 file".to_string()
        } else {
            format!("{} files", files.len())
        };
        let caption = format!("{}: {count} changed", chat.title);
        if !self.say_file(platform, conversation, "changes.patch", &diff, &caption) {
            self.say(
                platform,
                conversation,
                &format!("{caption}\n\n{}", clip(&diff, DIFF_TEXT_LIMIT)),
            );
        }
        None
    }

    /// Sends the file `name` to every conversation that talks to `chat_id`, with `caption`
    /// beside it, and returns how many took it.
    ///
    /// # Errors
    ///
    /// Returns an error when no conversation talks to the chat, or none of their chat apps
    /// took the file; the error is the last chat app's.
    pub(crate) fn send_file_to_chats(
        &self,
        chat_id: &str,
        name: &str,
        bytes: &[u8],
        caption: &str,
    ) -> Result<usize, Error> {
        if bytes.len() > FILE_LIMIT {
            return Err(format!(
                "The file is larger than the {} MB chat apps take",
                FILE_LIMIT / (1024 * 1024)
            )
            .into());
        }
        let mut took = 0;
        let mut refused: Option<Error> = None;
        for bound in self.bound_conversations(chat_id) {
            let sent = self
                .gateway
                .connector(bound.platform)
                .ok_or_else(|| Error::from("That chat app is not connected"))
                .and_then(|c| c.send_file(&bound.conversation, name, bytes, caption));
            match sent {
                Ok(()) => took += 1,
                Err(error) => refused = Some(error),
            }
        }
        match (took, refused) {
            (0, Some(error)) => Err(error),
            (0, None) => Err("No chat app conversation talks to this chat".into()),
            (took, _) => Ok(took),
        }
    }

    /// Sends the picture `bytes` to every conversation that talks to `chat_id`, as the file
    /// `stem` with the extension of what it is, and returns the file's name and how many
    /// conversations took it. A picture over `FILE_LIMIT` is made smaller first.
    ///
    /// # Errors
    ///
    /// Returns an error when `bytes` are not a picture chat apps show, it cannot be made small
    /// enough, or no conversation took it.
    pub(crate) fn send_picture_to_chats(
        &self,
        chat_id: &str,
        stem: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<(String, usize), Error> {
        let (bytes, extension) = fit_picture(bytes)?;
        let name = format!("{stem}.{extension}");
        let took = self.send_file_to_chats(chat_id, &name, &bytes, caption)?;
        Ok((name, took))
    }

    /// A screenshot of the screen for a chat app, as PNG, while Settings → Permissions lets the
    /// agent see the screen. Computer use is a setting of its own and need not be on.
    ///
    /// # Errors
    ///
    /// Returns an error when seeing the screen is off, or the screen cannot be captured.
    pub(crate) fn agent_screenshot(&self) -> Result<crate::computer::Screenshot, Error> {
        if !self.store.settings().see_screen {
            return Err(SCREENSHOTS_OFF.into());
        }
        crate::computer::look()
    }

    /// Takes a screenshot and sends it to the conversation `conversation`, and says what
    /// happened. Called off the chat app's thread, since the desktop may ask the person before
    /// the first screenshot.
    pub(super) fn send_screenshot(&self, platform: Platform, conversation: &str) {
        let shot = self
            .agent_screenshot()
            .and_then(|shot| fit_picture(shot.png));
        let sent = shot.and_then(|(bytes, extension)| {
            let connector = self
                .gateway
                .connector(platform)
                .ok_or_else(|| Error::from("That chat app is not connected"))?;
            let name = format!("screenshot.{extension}");
            connector.send_file(conversation, &name, &bytes, "The screen now")
        });
        if let Err(error) = sent {
            self.say(
                platform,
                conversation,
                &format!("Patchwork could not send a screenshot: {error}"),
            );
        }
    }

    /// Sends `text` to `chat` as the file `name` with `caption` beside it, and says whether the
    /// chat app took it.
    fn say_file(
        &self,
        platform: Platform,
        conversation: &str,
        name: &str,
        text: &str,
        caption: &str,
    ) -> bool {
        let Some(connector) = self.gateway.connector(platform) else {
            return false;
        };
        match connector.send_file(conversation, name, text.as_bytes(), caption) {
            Ok(()) => true,
            Err(error) => {
                log::debug!(platform = platform.id(), error:% = error; "could not send a file to {{platform}}: {{error}}");
                false
            }
        }
    }
}

/// A picture as it is when it fits in `FILE_LIMIT`, else saved again as a JPEG, and scaled down
/// to `SMALLER_SIDE` when that is not enough; with the extension of what it now is.
///
/// # Errors
///
/// Returns an error when `bytes` are not a PNG, JPEG, GIF or WebP picture, or are over the limit
/// and cannot be made small enough.
fn fit_picture(bytes: Vec<u8>) -> Result<(Vec<u8>, &'static str), Error> {
    let extension = match mime(&bytes) {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return Err("That is not a PNG, JPEG, GIF or WebP picture".into()),
    };
    if bytes.len() <= FILE_LIMIT {
        return Ok((bytes, extension));
    }
    if extension == "gif" {
        // Saving it again would keep only its first frame.
        return Err("The GIF is larger than chat apps take".into());
    }
    let picture =
        image::load_from_memory(&bytes).map_err(|e| format!("Could not read the picture: {e}"))?;
    drop(bytes);
    let jpeg = to_jpeg(&picture)?;
    if jpeg.len() <= FILE_LIMIT {
        return Ok((jpeg, "jpg"));
    }
    let smaller = to_jpeg(&picture.resize(SMALLER_SIDE, SMALLER_SIDE, FilterType::Triangle))?;
    if smaller.len() <= FILE_LIMIT {
        return Ok((smaller, "jpg"));
    }
    Err("The picture is too large to send, even made smaller".into())
}

/// `picture` saved as a JPEG at `JPEG_QUALITY`.
fn to_jpeg(picture: &image::DynamicImage) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode_image(&picture.to_rgb8())
        .map_err(|e| format!("Could not make the picture smaller: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of `side` by `side` pixels of noise, which compresses badly.
    fn noisy_png(side: u32) -> Vec<u8> {
        let mut seed: u32 = 1;
        let picture = image::RgbImage::from_fn(side, side, |_, _| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [r, g, b, _] = seed.to_le_bytes();
            image::Rgb([r, g, b])
        });
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("a PNG encodes");
        png
    }

    #[test]
    fn small_picture_goes_as_it_is() {
        let png = noisy_png(16);
        let (sent, extension) = fit_picture(png.clone()).expect("a small PNG fits");
        assert_eq!(extension, "png");
        assert_eq!(sent, png);
    }

    #[test]
    fn large_picture_is_made_a_jpeg_under_the_limit() {
        let png = noisy_png(2000);
        assert!(
            png.len() > FILE_LIMIT,
            "the test picture must start over the limit"
        );
        let (sent, extension) = fit_picture(png).expect("a large PNG is made smaller");
        assert_eq!(extension, "jpg");
        assert!(sent.len() <= FILE_LIMIT);
        assert_eq!(mime(&sent), "image/jpeg");
    }

    #[test]
    fn text_is_not_a_picture() {
        fit_picture(b"not a picture".to_vec()).expect_err("text is not a picture");
    }

    #[test]
    fn png_named_bytes_are_not_trusted() {
        // Only the bytes decide, so a key file renamed to .png is still refused.
        fit_picture(b"-----BEGIN OPENSSH PRIVATE KEY-----".to_vec())
            .expect_err("a key is not a picture");
    }
}
