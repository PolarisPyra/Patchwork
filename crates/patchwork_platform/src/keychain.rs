//! The system credential store: the Secret Service on Linux, the Keychain on macOS and the
//! Credential Manager on Windows. A secret is named by a service and a name within it.
//!
//! On Linux the items keep the attributes earlier versions wrote through the `keyring` crate
//! (`service`, `username`, and `target` set to `default`), so each version finds the other's.

// Rust guideline compliant 2026-10-09

use crate::Error;

/// The secret saved as `name` under `service`, or `None` when there is none.
///
/// # Errors
///
/// Returns an error when the system has no credential store, or it cannot be read.
pub fn get(service: &str, name: &str) -> Result<Option<String>, Error> {
    backend::get(service, name)
}

/// Saves `secret` as `name` under `service`, replacing what was there.
///
/// # Errors
///
/// Returns an error when the system has no credential store, or it cannot be written.
pub fn set(service: &str, name: &str, secret: &str) -> Result<(), Error> {
    backend::set(service, name, secret)
}

/// Removes the secret saved as `name` under `service`. One that is not there is already gone.
///
/// # Errors
///
/// Returns an error when the system has no credential store, or it cannot be written.
pub fn delete(service: &str, name: &str) -> Result<(), Error> {
    backend::delete(service, name)
}

#[cfg(all(feature = "keychain", target_os = "linux"))]
mod backend {
    use crate::Error;

    /// What `keyring` searched on. Its `application` attribute and label are only shown.
    fn attributes<'a>(service: &'a str, name: &'a str) -> [(&'a str, &'a str); 2] {
        [("service", service), ("username", name)]
    }

    fn failed(e: &oo7::Error) -> Error {
        format!("The system keychain failed: {e}").into()
    }

    /// The default collection, unlocked, which may ask the user for its password.
    async fn keyring() -> Result<oo7::Keyring, Error> {
        let keyring = oo7::Keyring::new().await.map_err(|e| failed(&e))?;
        if keyring.is_locked().await.map_err(|e| failed(&e))? {
            keyring.unlock().await.map_err(|e| failed(&e))?;
        }
        Ok(keyring)
    }

    pub(super) fn get(service: &str, name: &str) -> Result<Option<String>, Error> {
        async_io::block_on(async {
            let keyring = keyring().await?;
            let items = keyring
                .search_items(&attributes(service, name))
                .await
                .map_err(|e| failed(&e))?;
            let Some(item) = items.first() else {
                return Ok(None);
            };
            let secret = item.secret().await.map_err(|e| failed(&e))?;
            String::from_utf8(secret.as_bytes().to_vec())
                .map(Some)
                .map_err(|_utf8| "The secret in the system keychain is not text".into())
        })
    }

    pub(super) fn set(service: &str, name: &str, secret: &str) -> Result<(), Error> {
        async_io::block_on(async {
            let keyring = keyring().await?;
            let items = keyring
                .search_items(&attributes(service, name))
                .await
                .map_err(|e| failed(&e))?;
            if let Some(item) = items.first() {
                return item
                    .set_secret(secret.to_string())
                    .await
                    .map_err(|e| failed(&e));
            }
            let label = format!("{name}@{service}");
            let attributes = [
                ("service", service),
                ("username", name),
                ("target", "default"),
                ("application", service),
            ];
            keyring
                .create_item(&label, &attributes, secret.to_string(), true)
                .await
                .map_err(|e| failed(&e))
        })
    }

    pub(super) fn delete(service: &str, name: &str) -> Result<(), Error> {
        async_io::block_on(async {
            let keyring = keyring().await?;
            let items = keyring
                .search_items(&attributes(service, name))
                .await
                .map_err(|e| failed(&e))?;
            for item in items {
                item.delete().await.map_err(|e| failed(&e))?;
            }
            Ok(())
        })
    }
}

#[cfg(all(feature = "keychain", not(target_os = "linux")))]
mod backend {
    use crate::Error;

    fn failed(e: &keyring::Error) -> Error {
        format!("The system keychain failed: {e}").into()
    }

    fn entry(service: &str, name: &str) -> Result<keyring::Entry, Error> {
        keyring::Entry::new(service, name).map_err(|e| failed(&e))
    }

    pub(super) fn get(service: &str, name: &str) -> Result<Option<String>, Error> {
        match entry(service, name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(failed(&e)),
        }
    }

    pub(super) fn set(service: &str, name: &str, secret: &str) -> Result<(), Error> {
        entry(service, name)?
            .set_password(secret)
            .map_err(|e| failed(&e))
    }

    pub(super) fn delete(service: &str, name: &str) -> Result<(), Error> {
        match entry(service, name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(failed(&e)),
        }
    }
}

/// A build without the credential store, such as the helper copied to SSH hosts.
#[cfg(not(feature = "keychain"))]
mod backend {
    use crate::Error;

    fn none() -> Error {
        "This build has no system keychain".into()
    }

    pub(super) fn get(_service: &str, _name: &str) -> Result<Option<String>, Error> {
        Err(none())
    }

    pub(super) fn set(_service: &str, _name: &str, _secret: &str) -> Result<(), Error> {
        Err(none())
    }

    pub(super) fn delete(_service: &str, _name: &str) -> Result<(), Error> {
        Err(none())
    }
}
