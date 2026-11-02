use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use patchwork_server::Res;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;

use super::install;
use super::{UpdateState, Updates};

const LATEST_RELEASE: &str = "https://updates.patchworkeditor.com/latest.json";
const SIGNING_KEY: &str = "RWRXprrQ+eDX1VTXegp1CNjQ85EUGvB2gw6QQR/8pzPzVHjoIsayrxzc";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Deserialize)]
pub(super) struct Release {
    pub(super) tag_name: String,
    /// The release's notes; releases before 0.10.16 have none.
    #[serde(default)]
    pub(super) body: Option<String>,
    /// Tags pulled after they went out broken. A copy running one of them goes back to this
    /// release, even though it is older.
    #[serde(default)]
    pub(super) withdrawn: Vec<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`.
    digest: Option<String>,
    /// The file's minisign signature, as the text of its `.sig` file.
    signature: Option<String>,
}
fn is_own_asset(name: &str) -> bool {
    if cfg!(windows) {
        name.ends_with("-setup.exe")
    } else if cfg!(target_os = "macos") {
        name == "Patchwork.app.tar.gz"
    } else {
        name.ends_with(".AppImage")
    }
}
fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!("Patchwork/{}", patchwork_server::VERSION))
        .build()
        .into()
}

impl Updates {
    pub(super) fn download_and_install(
        &self,
        app: &AppHandle,
        release: &Release,
        version: &str,
    ) -> Res<()> {
        let asset = release
            .assets
            .iter()
            .find(|asset| is_own_asset(&asset.name))
            .ok_or("The release has no download for this system.")?;
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or("The release does not publish this download's checksum.")?;
        let signature = asset
            .signature
            .as_deref()
            .ok_or("The release does not publish this download's signature.")?;
        let signature = minisign_verify::Signature::decode(signature)
            .map_err(|e| format!("The release has an unreadable signature: {e}"))?;
        let file = self.download(
            app,
            &asset.browser_download_url,
            sha256,
            &signature,
            version,
        )?;
        install::install(file, self)
    }

    /// Downloads `url` into the temporary folder and checks its SHA-256 and signature.
    fn download(
        &self,
        app: &AppHandle,
        url: &str,
        sha256: &str,
        signature: &minisign_verify::Signature,
        version: &str,
    ) -> Res<PathBuf> {
        let key =
            minisign_verify::PublicKey::from_base64(SIGNING_KEY).map_err(|e| e.to_string())?;
        let mut verifier = key
            .verify_stream(signature)
            .map_err(|e| format!("The release has an unusable signature: {e}"))?;
        self.set(
            app,
            UpdateState::Downloading {
                version: version.into(),
                downloaded: 0,
                total: None,
            },
        );
        let mut response = agent(DOWNLOAD_TIMEOUT)
            .get(url)
            .call()
            .map_err(|e| format!("The download did not start: {e}"))?;
        let total = response.body().content_length();
        let name = url.rsplit('/').next().unwrap_or("patchwork-update");
        let path = std::env::temp_dir().join(format!("patchwork-{version}-{name}"));
        let result = (|| {
            let mut file = std::fs::File::create(&path)
                .map_err(|e| format!("Could not save the download: {e}"))?;
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
                hasher.update(&buf[..read]);
                verifier.update(&buf[..read]);
                file.write_all(&buf[..read])
                    .map_err(|e| format!("Could not save the download: {e}"))?;
                downloaded += read as u64;
                if downloaded / step != shown / step {
                    shown = downloaded;
                    self.set(
                        app,
                        UpdateState::Downloading {
                            version: version.into(),
                            downloaded,
                            total,
                        },
                    );
                }
            }
            file.sync_all().map_err(|e| e.to_string())?;
            let got: String = hasher
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if !got.eq_ignore_ascii_case(sha256) {
                return Err("The download does not match its checksum.".to_string());
            }
            verifier.finalize().map_err(|_| {
                "The download is not signed with Patchwork's release key.".to_string()
            })?;
            Ok(())
        })();
        match result {
            Ok(()) => Ok(path),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        }
    }
}

pub(super) fn latest_release() -> Res<Release> {
    let mut response = agent(CHECK_TIMEOUT)
        .get(LATEST_RELEASE)
        .call()
        .map_err(|e| format!("Could not reach the update server: {e}"))?;
    response
        .body_mut()
        .read_json()
        .map_err(|e| format!("The update server sent an unreadable release:{e}"))
}
