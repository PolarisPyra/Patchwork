//! The license key that unlocks a paid plan, and what it unlocks now. The plans and the keys
//! themselves are in `commercial/crates/licensing`. A key belongs to an account: it is taken only
//! while signed in to the account it was issued to, and its plan holds only while signed in to it.

use serde::Serialize;
use ts_rs::TS;

pub use patchwork_licensing::{Feature, License, Plan};

use crate::accounts::{signed_in_email, trial_key};
use crate::{Res, Server};

/// The plan in effect, and the key it comes from, for Settings → Account.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "license.ts")]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatus {
    pub plan: Plan,
    /// The paid features `plan` includes.
    pub features: Vec<Feature>,
    /// Who the key was issued to; None without a key.
    pub email: Option<String>,
    /// When the key runs out, or ran out, in milliseconds since 1970.
    pub expires_at: Option<u64>,
    /// The key has run out, so the plan is Free again.
    pub expired: bool,
    /// This install is not signed in to the key's account, so the plan is Free until it is.
    pub paused: bool,
}

/// The `app_state` key the license key is kept under.
const KEY: &str = "license-key";

impl Server {
    pub fn license_status(&self) -> LicenseStatus {
        status(self.license().as_ref())
    }

    /// Takes a license key in place of any before it. A key Patchwork did not sign, or one issued
    /// to another account than the one signed in, is refused, and the one before stays.
    pub fn set_license_key(&self, key: &str) -> Res<LicenseStatus> {
        let license = patchwork_licensing::read_key(key).map_err(|e| e.to_string())?;
        let email = signed_in_email().ok_or("Sign in to add a license key")?;
        if !email.eq_ignore_ascii_case(&license.email) {
            return Err(format!(
                "This key is for {}. Sign in with that account to use it.",
                license.email
            ));
        }
        self.store.set_state(KEY, key.trim())?;
        log::info!(
            "license key added: {:?} plan, order {}",
            license.plan,
            license.id
        );
        Ok(status(Some(&license)))
    }

    /// Starts the account's free trial: 14 days of Basic, as a license key from the account
    /// service, which gives each account one.
    pub fn start_trial(&self) -> Res<LicenseStatus> {
        let key = trial_key()?;
        let status = self.set_license_key(&key)?;
        log::info!("free trial started");
        Ok(status)
    }

    pub fn remove_license_key(&self) -> Res<LicenseStatus> {
        self.store.clear_state(KEY)?;
        log::info!("license key removed");
        Ok(status(None))
    }

    /// Whether the plan in effect includes `feature`.
    pub(crate) fn allows(&self, feature: Feature) -> bool {
        self.license_status().features.contains(&feature)
    }

    fn license(&self) -> Option<License> {
        let key = self.store.state(KEY)?;
        patchwork_licensing::read_key(&key).ok()
    }
}

fn status(license: Option<&License>) -> LicenseStatus {
    let now = crate::system::time::now_ms() / 1000;
    let paused = license.is_some_and(|l| {
        signed_in_email().is_none_or(|email| !email.eq_ignore_ascii_case(&l.email))
    });
    let plan = match license {
        Some(l) if !paused => l.plan_at(now),
        _ => Plan::Free,
    };
    LicenseStatus {
        plan,
        features: plan.features().to_vec(),
        email: license.map(|l| l.email.clone()),
        expires_at: license.map(|l| l.expires.saturating_mul(1000)),
        expired: license.is_some_and(|l| l.expires <= now),
        paused,
    }
}
