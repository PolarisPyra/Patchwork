//! Whether an API provider runs on the user's own machine or network, or is a cloud service. The
//! answer the user picks is stored on the provider and always wins; this only suggests one, while a
//! provider is being added and once for a provider saved before the choice existed. A URL cannot
//! prove where a server runs (a local server can sit behind a real domain, a cloud gateway behind
//! `localhost`), so the suggestion leans on what an address can show: loopback and private
//! addresses, names only a local network resolves, and the hosts of known cloud APIs.

use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use ts_rs::TS;

/// Where a provider looks like it runs, and why, shown beside the suggestion in the provider form.
#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "provider.ts")]
pub struct Locality {
    pub local: bool,
    pub reason: String,
}

/// Endings of the hosts of cloud model APIs. A host equal to one, or a subdomain of one, is cloud.
const CLOUD_HOSTS: &[&str] = &[
    "openai.com",
    "anthropic.com",
    "deepseek.com",
    "openrouter.ai",
    "opencode.ai",
    "groq.com",
    "together.xyz",
    "together.ai",
    "mistral.ai",
    "googleapis.com",
    "x.ai",
    "fireworks.ai",
    "perplexity.ai",
    "cohere.com",
    "cohere.ai",
    "cerebras.ai",
    "deepinfra.com",
    "azure.com",
    "amazonaws.com",
    "moonshot.ai",
    "moonshot.cn",
    "z.ai",
    "bigmodel.cn",
    "siliconflow.cn",
    "siliconflow.com",
    "novita.ai",
    "hyperbolic.xyz",
    "nvidia.com",
    "huggingface.co",
    "sambanova.ai",
    "minimax.io",
    "minimaxi.com",
    "dashscope.aliyuncs.com",
];

/// Endings of names that only resolve inside a local network: mDNS, router and home domains, the
/// reserved `.internal`, and Tailscale's MagicDNS.
const LOCAL_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "lan",
    "home",
    "home.arpa",
    "internal",
    "localdomain",
    "intranet",
    "ts.net",
];

/// How long a name may take to resolve before the suggestion gives up on it.
const RESOLVE_LIMIT: Duration = Duration::from_secs(2);

/// Suggests where the provider at `base_url` runs. With `resolve`, a name that is neither known
/// local nor known cloud is looked up, and is local when every address it resolves to is; without
/// it, such a name is taken as cloud, the side that asks for a key.
pub fn classify(base_url: &str, resolve: bool) -> Locality {
    let Ok(url) = url::Url::parse(base_url.trim()) else {
        return cloud("Not a URL yet");
    };
    match url.host() {
        Some(url::Host::Ipv4(ip)) => by_address(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => by_address(IpAddr::V6(ip)),
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            by_name(&name, url.port_or_known_default().unwrap_or(443), resolve)
        }
        None => cloud("Not a URL yet"),
    }
}

fn local(reason: impl Into<String>) -> Locality {
    Locality {
        local: true,
        reason: reason.into(),
    }
}

fn cloud(reason: impl Into<String>) -> Locality {
    Locality {
        local: false,
        reason: reason.into(),
    }
}

fn by_address(ip: IpAddr) -> Locality {
    if is_local_ip(ip) {
        local(format!("{ip} is on this computer or your network"))
    } else {
        cloud(format!("{ip} is a public address"))
    }
}

fn by_name(name: &str, port: u16, resolve: bool) -> Locality {
    let ends_with = |suffix: &str| name == suffix || name.ends_with(&format!(".{suffix}"));
    if let Some(known) = CLOUD_HOSTS.iter().find(|s| ends_with(s)) {
        return cloud(format!("{known} is a cloud API"));
    }
    if LOCAL_SUFFIXES.iter().any(|s| ends_with(s)) {
        return local(format!("{name} is a local network name"));
    }
    // A name without a dot is a machine on the local network, such as a router-assigned hostname.
    if !name.contains('.') {
        return local(format!("{name} is a local network name"));
    }
    if !resolve {
        return cloud(format!("{name} is a public name"));
    }
    match lookup(name, port) {
        Some(ips) if !ips.is_empty() && ips.iter().all(|ip| is_local_ip(*ip)) => {
            local(format!("{name} points to {} on your network", ips[0]))
        }
        Some(ips) if !ips.is_empty() => cloud(format!("{name} points to a public address")),
        _ => cloud(format!("{name} does not resolve")),
    }
}

/// The addresses `name` resolves to, or `None` when the lookup fails or takes too long. The system
/// resolver has no timeout of its own, so it runs on a thread that is left behind when it is slow.
fn lookup(name: &str, port: u16) -> Option<Vec<IpAddr>> {
    let (tx, rx) = mpsc::channel();
    let target = format!("{name}:{port}");
    std::thread::spawn(move || {
        let ips = target
            .to_socket_addrs()
            .map(|addrs| addrs.map(|a| a.ip()).collect::<Vec<_>>());
        let _ = tx.send(ips);
    });
    rx.recv_timeout(RESOLVE_LIMIT).ok()?.ok()
}

/// Loopback, private, link-local and shared (carrier-grade NAT, which Tailscale uses) addresses,
/// and their IPv6 counterparts: unique local and link-local. An IPv4 address mapped into IPv6 is
/// judged as the IPv4 address.
fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_local_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local_v4(v4);
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

fn is_local_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || (a == 100 && (64..128).contains(&b))
}
