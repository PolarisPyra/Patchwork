//! The latest release as latest.json describes it, and its download, checked as it arrives.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::Error;

const LATEST_RELEASE: &str =
    "https://github.com/PolarisPyra/Patchwork/releases/latest/download/latest.json";
/// The page a copy that cannot install updates sends the user to for the download.
pub const RELEASES_URL: &str = "https://github.com/PolarisPyra/Patchwork/releases/latest";
/// The release signing key's public half. The Release workflow reads it from here to check each
/// signature before it publishes.
const SIGNING_KEY: &str = "RWRXprrQ+eDX1VTXegp1CNjQ85EUGvB2gw6QQR/8pzPzVHjoIsayrxzc";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// The latest release, in the shape of GitHub's release API. The Release workflow publishes it.
#[derive(Debug, Deserialize)]
pub(super) struct Release {
    tag_name: String,
    /// The release's section of CHANGELOG.md.
    #[serde(default)]
    pub(super) body: Option<String>,
    /// Tags pulled after they went out broken. A copy running one of them goes back to this
    /// release, even though it is older.
    #[serde(default)]
    withdrawn: Vec<String>,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    /// Absent from releases that only named their files.
    browser_download_url: Option<String>,
    /// `sha256:<hex>`.
    digest: Option<String>,
    /// The file's minisign signature, as the text of its `.minisig` file.
    signature: Option<String>,
}

/// One file of a release and what it must match.
pub(super) struct Download<'a> {
    url: &'a str,
    sha256: &'a str,
    signature: Signature,
}

fn is_own_asset(name: &str) -> bool {
    let kind = if cfg!(windows) {
        "exe"
    } else if cfg!(target_os = "macos") {
        "dmg"
    } else {
        "AppImage"
    };
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(kind))
}

/// `26.11.32` as its numbers, which compare the way versions do.
fn numbers(version: &str) -> Option<Vec<u64>> {
    version.split('.').map(|part| part.parse().ok()).collect()
}

impl Release {
    pub(super) fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    /// Whether a copy running `current` should move to this release: it is newer, or `current`
    /// was withdrawn.
    pub(super) fn replaces(&self, current: &str) -> bool {
        let newer = matches!(
            (numbers(self.version()), numbers(current)),
            (Some(latest), Some(current)) if latest > current
        );
        newer
            || self
                .withdrawn
                .iter()
                .any(|tag| tag.trim_start_matches('v') == current)
    }

    /// This system's file, with the checksum and signature the release publishes for it.
    pub(super) fn download(&self) -> Result<Download<'_>, Error> {
        let asset = self
            .assets
            .iter()
            .find(|asset| is_own_asset(&asset.name))
            .ok_or("The release has no download for this system.")?;
        let url = asset
            .browser_download_url
            .as_deref()
            .ok_or("The release does not say where its download is.")?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or("The release does not publish this download's checksum.")?;
        let signature = asset
            .signature
            .as_deref()
            .ok_or("The release does not publish this download's signature.")?;
        let signature = Signature::decode(signature)
            .map_err(|e| format!("The release has an unreadable signature: {e}"))?;
        Ok(Download {
            url,
            sha256,
            signature,
        })
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!("Patchwork/{}", crate::VERSION))
        .build()
        .into()
}

/// Asks GitHub for the latest release. Blocks.
pub(super) fn latest() -> Result<Release, Error> {
    let mut response = agent(CHECK_TIMEOUT)
        .get(LATEST_RELEASE)
        .call()
        .map_err(|e| format!("Could not reach the update server: {e}"))?;
    response
        .body_mut()
        .read_json()
        .map_err(|e| format!("The update server sent an unreadable release: {e}").into())
}

impl Download<'_> {
    /// Downloads the file into `folder` and checks its SHA-256 and signature, calling `progress`
    /// with the bytes so far and the total at each whole percent. A file that fails a check is
    /// removed. Blocks.
    pub(super) fn fetch(
        &self,
        folder: &Path,
        progress: &dyn Fn(u64, Option<u64>),
    ) -> Result<PathBuf, Error> {
        std::fs::create_dir_all(folder).map_err(|e| format!("Could not save the download: {e}"))?;
        let name = self.url.rsplit('/').next().unwrap_or("patchwork-update");
        let path = folder.join(name);
        let result = self.fetch_into(&path, progress);
        if result.is_err() {
            let _ = std::fs::remove_file(&path);
        }
        result.map(|()| path)
    }

    fn fetch_into(&self, path: &Path, progress: &dyn Fn(u64, Option<u64>)) -> Result<(), Error> {
        let key = PublicKey::from_base64(SIGNING_KEY).map_err(|e| e.to_string())?;
        let mut verifier = key
            .verify_stream(&self.signature)
            .map_err(|e| format!("The release has an unusable signature: {e}"))?;
        progress(0, None);
        let mut response = agent(DOWNLOAD_TIMEOUT)
            .get(self.url)
            .call()
            .map_err(|e| format!("The download did not start: {e}"))?;
        let total = response.body().content_length();
        let mut file =
            std::fs::File::create(path).map_err(|e| format!("Could not save the download: {e}"))?;
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let (mut downloaded, mut shown) = (0u64, 0u64);
        // Each whole percent, or each MiB when the size is unknown.
        let step = total.map_or(1 << 20, |t| (t / 100).max(1));
        loop {
            let read = reader
                .read(&mut buf)
                .map_err(|e| format!("The download stopped: {e}"))?;
            if read == 0 {
                break;
            }
            let chunk = &buf[..read];
            hasher.update(chunk);
            verifier.update(chunk);
            file.write_all(chunk)
                .map_err(|e| format!("Could not save the download: {e}"))?;
            downloaded += read as u64;
            if downloaded / step != shown / step {
                shown = downloaded;
                progress(downloaded, total);
            }
        }
        file.sync_all()
            .map_err(|e| format!("Could not save the download: {e}"))?;
        let got = hasher
            .finalize()
            .iter()
            .fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            });
        if !got.eq_ignore_ascii_case(self.sha256) {
            return Err("The download does not match its checksum.".into());
        }
        verifier
            .finalize()
            .map_err(|_mismatch| "The download is not signed with Patchwork's release key.".into())
    }
}
