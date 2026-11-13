//! Phones over Wi-Fi: the ones adb finds on the local network, pairing with the code a phone
//! shows under Developer options → Wireless debugging, and connecting to one already paired.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::time::{Duration, Instant};

use super::devices::ADB_LIMIT;
use super::tool::{self, Output, tool_command};
use super::types::{Sdk, WifiDevice};
use crate::{Error, Server};

/// The service a phone announces while it shows a pairing code.
const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp";

/// The service a phone with wireless debugging on announces for paired computers.
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp";

/// How long a phone just paired is looked for on the network before it is left to connect on
/// its own.
const ANNOUNCE_WAIT: Duration = Duration::from_secs(6);

/// The phones in the output of `adb mdns services`, whose lines read
/// `adb-R58M123ABC-x7Yz1q _adb-tls-connect._tcp 192.168.1.20:37251`.
pub(super) fn parse_services(listing: &str) -> Vec<WifiDevice> {
    listing
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (name, service, address) = (parts.next()?, parts.next()?, parts.next()?);
            let pairing = match service.trim_end_matches('.') {
                PAIRING_SERVICE => true,
                CONNECT_SERVICE => false,
                _ => return None,
            };
            Some(WifiDevice {
                name: name.to_string(),
                address: address.to_string(),
                pairing,
            })
        })
        .collect()
}

/// `address` when it reads as a host and a port, as `192.168.1.20:37251`.
fn checked(address: &str) -> Result<&str, Error> {
    let address = address.trim();
    let port = address.rsplit_once(':').map(|(_, port)| port);
    let plain = address
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'));
    if plain && port.is_some_and(|port| port.parse::<u16>().is_ok()) {
        Ok(address)
    } else {
        Err("Enter the address the phone shows, as 192.168.1.20:37251.".into())
    }
}

fn adb(sdk: &Sdk, args: &[&str]) -> Result<Output, Error> {
    let mut cmd = tool_command(sdk, sdk.adb()?);
    cmd.args(args);
    tool::output(cmd, ADB_LIMIT)
}

fn services(sdk: &Sdk) -> Result<Vec<WifiDevice>, Error> {
    Ok(parse_services(&adb(sdk, &["mdns", "services"])?.stdout))
}

fn connect(sdk: &Sdk, address: &str) -> Result<(), Error> {
    let out = adb(sdk, &["connect", address])?;
    // adb exits zero whether or not it connected.
    if out.ok && out.stdout.contains("connected to") {
        return Ok(());
    }
    Err(Error::Tool {
        message: format!(
            "adb could not connect to {address}. Check that the phone is on this network with \
             Wireless debugging on, and that the address is the one on its Wireless debugging \
             screen."
        ),
        detail: out.all(),
    })
}

impl Server {
    /// The phones adb finds on the local network with wireless debugging on.
    ///
    /// # Errors
    ///
    /// Returns an error when the SDK has no adb, or adb fails.
    pub fn android_wifi_devices(&self, project: &Path) -> Result<Vec<WifiDevice>, Error> {
        services(&self.android_sdk(project)?)
    }

    /// Pairs with the phone at `address` using the `code` it shows, then connects to it when it
    /// announces itself. A phone that does not is connected by adb once it does.
    ///
    /// # Errors
    ///
    /// Returns an error when the address or the code is malformed, or the phone refuses them.
    pub fn android_pair(&self, project: &Path, address: &str, code: &str) -> Result<(), Error> {
        let address = checked(address)?;
        let code = code.trim();
        if code.is_empty() || !code.chars().all(|c| c.is_ascii_digit()) {
            return Err("Enter the six-digit code the phone shows.".into());
        }
        let sdk = self.android_sdk(project)?;
        let out = adb(&sdk, &["pair", address, code])?;
        if !out.stdout.contains("Successfully paired") {
            return Err(Error::Tool {
                message: "The phone refused the pairing. The code and the address change each \
                          time the phone shows them: enter the ones on its screen now."
                    .into(),
                detail: out.all(),
            });
        }
        let host = address.rsplit_once(':').map_or(address, |(host, _)| host);
        let deadline = Instant::now() + ANNOUNCE_WAIT;
        while Instant::now() < deadline {
            let found = services(&sdk)
                .unwrap_or_default()
                .into_iter()
                .find(|device| {
                    !device.pairing && device.address.rsplit_once(':').map(|(h, _)| h) == Some(host)
                });
            if let Some(device) = found {
                return connect(&sdk, &device.address);
            }
            // How often adb is asked what the network announced.
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(())
    }

    /// Connects to a paired phone at `address`.
    ///
    /// # Errors
    ///
    /// Returns an error when the address is malformed, or the phone does not answer.
    pub fn android_connect(&self, project: &Path, address: &str) -> Result<(), Error> {
        connect(&self.android_sdk(project)?, checked(address)?)
    }
}
