//! Looking up names, refusing public names that point at this computer or the local network.
//!
//! A page can't send the browser to a local address directly (links, redirects, images and forms
//! from public pages to 192.168.x and the like are blocked), but a public name can point at one:
//! evil.example's DNS can answer 192.168.1.1, or answer differently a second time (DNS
//! rebinding). So every name is looked up here, at connection time, and a public name whose
//! addresses include a local one is refused. The connection then goes to exactly the addresses
//! checked, so a changed answer can't slip in between.
//!
//! Names that are local by their form (localhost, printer.local, router.lan, nas.home.arpa,
//! *.internal, single-word names) are looked up as usual: opening them yourself works, and public
//! pages can't send you to them (network::is_local_network_url covers them). Names on a company
//! network that point at internal addresses can be allowed with the local_names setting.

use std::net::{IpAddr, SocketAddr};
use std::sync::RwLock;

/// Names allowed to point at local addresses (the local_names setting), lower case
static ALLOWED: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Endings of names that are local by their form
const LOCAL_ENDINGS: &[&str] = &[".local", ".lan", ".home.arpa", ".internal", ".localdomain", ".localhost", ".home"];

/// Set the names allowed to point at local addresses (and their subdomains)
pub fn allow_local_names(names: &[String]) {
    let names = names.iter().map(|n| n.trim().trim_end_matches('.').to_ascii_lowercase()).filter(|n| !n.is_empty());
    *ALLOWED.write().unwrap_or_else(|e| e.into_inner()) = names.collect();
}

/// Whether `name` is local by its form: localhost, a single word, or a local-network ending
pub fn is_local_name(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    name == "localhost"
        || (!name.contains('.') && !name.contains(':') && !name.is_empty())
        || LOCAL_ENDINGS.iter().any(|ending| name.ends_with(ending))
}

/// Whether an address is on this computer or the local network
pub fn is_local_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // 100.64.0.0/10, carrier-grade NAT (and Tailscale)
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
                || v6.to_ipv4_mapped().is_some_and(|v4| is_local_address(IpAddr::V4(v4)))
        }
    }
}

fn allowed(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    ALLOWED
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|allowed| name == *allowed || name.strip_suffix(allowed.as_str()).is_some_and(|rest| rest.ends_with('.')))
}

/// Check a name's addresses: an error if a public name points at a local address
pub fn check(name: &str, addresses: &[SocketAddr]) -> Result<(), String> {
    if is_local_name(name) || allowed(name) {
        return Ok(());
    }
    match addresses.iter().find(|a| is_local_address(a.ip())) {
        Some(local) => Err(format!(
            "{} points at {}, on this computer or your local network, so rynx didn't connect \
             (if it's a name on your own network, add it to local_names in settings)",
            name,
            local.ip()
        )),
        None => Ok(()),
    }
}

/// Look `name` up and check its addresses
pub async fn resolve(name: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    // Addresses written as such aren't names; their own checks apply
    if let Ok(ip) = name.trim_matches(['[', ']']).parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addresses: Vec<SocketAddr> =
        tokio::net::lookup_host((name, port)).await.map_err(|e| format!("Couldn't find {}: {}", name, e))?.collect();
    check(name, &addresses)?;
    Ok(addresses)
}

/// Connect to `name`, looked up and checked
pub async fn connect(name: &str, port: u16) -> Result<tokio::net::TcpStream, String> {
    let addresses = resolve(name, port).await?;
    tokio::net::TcpStream::connect(&addresses[..]).await.map_err(|e| e.to_string())
}

/// The resolver for the web clients
#[derive(Debug, Default)]
pub struct Resolver;

impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let name = name.as_str().to_string();
        Box::pin(async move {
            let addresses =
                resolve(&name, 0).await.map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}
