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
//!
//! With the dns setting, other names are looked up over encrypted DNS (DNS over HTTPS) instead of
//! the system's resolver, so the network and its DNS server don't see or change them; there's no
//! falling back to the system's, which would show them after all. Local names still go to the
//! system, since only it knows them.

use hickory_resolver::TokioResolver;
use hickory_resolver::config::{NameServerConfig, ResolverConfig, ServerGroup};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, RwLock};

/// Names are looked up encrypted through Quad9 unless the dns setting says otherwise: a
/// non-profit, in Switzerland, that keeps no addresses of who asks
pub const DEFAULT_DNS: &str = "quad9";

/// Encrypted DNS providers to choose from: (setting value, name, servers)
pub const DNS_PROVIDERS: &[(&str, &str, ServerGroup<'static>)] = &[
    ("quad9", "Quad9 (Switzerland, non-profit, blocks malware domains)", hickory_resolver::config::QUAD9),
    ("mullvad", "Mullvad (Sweden, no logs)", MULLVAD),
    ("cloudflare", "Cloudflare (1.1.1.1)", hickory_resolver::config::CLOUDFLARE),
];

const MULLVAD: ServerGroup<'static> = ServerGroup {
    ips: &[IpAddr::V4(Ipv4Addr::new(194, 242, 2, 2)), IpAddr::V6(Ipv6Addr::new(0x2a07, 0xe340, 0, 0, 0, 0, 0, 2))],
    server_name: "dns.mullvad.net",
    path: "/dns-query",
};

/// The encrypted DNS in use: the setting's value, and the resolver, made on first use (a custom
/// provider's own name is looked up then, through the system, once)
struct Encrypted {
    setting: String,
    resolver: tokio::sync::OnceCell<Result<Arc<TokioResolver>, String>>,
}

static ENCRYPTED: RwLock<Option<Arc<Encrypted>>> = RwLock::new(None);

/// Whether `value` is a dns setting: system, a provider's name, or a DNS-over-HTTPS address
pub fn is_dns_setting(value: &str) -> bool {
    value == "system"
        || DNS_PROVIDERS.iter().any(|(name, ..)| *name == value)
        || url::Url::parse(value).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some())
}

/// Look names up as the dns setting says: "system", or encrypted through a provider
pub fn use_dns(setting: &str) {
    let encrypted = (setting != "system" && is_dns_setting(setting))
        .then(|| Arc::new(Encrypted { setting: setting.to_string(), resolver: tokio::sync::OnceCell::new() }));
    *ENCRYPTED.write().unwrap_or_else(|e| e.into_inner()) = encrypted;
}

/// The encrypted DNS provider in use, by its setting, if any
pub fn encrypted_provider() -> Option<String> {
    ENCRYPTED.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|e| e.setting.clone())
}

/// The encrypted DNS resolver, if the setting asks for one and it could be made
pub async fn encrypted() -> Option<Arc<TokioResolver>> {
    encrypted_resolver().await.and_then(Result::ok)
}

async fn encrypted_resolver() -> Option<Result<Arc<TokioResolver>, String>> {
    let encrypted = ENCRYPTED.read().unwrap_or_else(|e| e.into_inner()).clone()?;
    Some(encrypted.resolver.get_or_init(|| make_resolver(encrypted.setting.clone())).await.clone())
}

async fn make_resolver(setting: String) -> Result<Arc<TokioResolver>, String> {
    let servers: Vec<NameServerConfig> = match DNS_PROVIDERS.iter().find(|(name, ..)| *name == setting) {
        Some((_, _, group)) => group.https().collect(),
        None => {
            let url = url::Url::parse(&setting).map_err(|e| e.to_string())?;
            let host = url.host_str().ok_or("the encrypted DNS address has no host")?.to_string();
            let port = url.port_or_known_default().unwrap_or(443);
            let ips: Vec<IpAddr> = match host.trim_matches(['[', ']']).parse::<IpAddr>() {
                Ok(ip) => vec![ip],
                Err(_) => tokio::net::lookup_host((host.as_str(), port))
                    .await
                    .map_err(|e| format!("couldn't find the encrypted DNS server {}: {}", host, e))?
                    .map(|a| a.ip())
                    .collect(),
            };
            let path: Arc<str> = Arc::from(url.path());
            ips.into_iter()
                .map(|ip| {
                    let mut server = NameServerConfig::https(ip, Arc::from(host.as_str()), Some(Arc::clone(&path)));
                    for connection in &mut server.connections {
                        connection.port = port;
                    }
                    server
                })
                .collect()
        }
    };
    let config = ResolverConfig::from_name_servers(servers);
    let tls = crate::network::NetworkManager::tls_config(true).map_err(|e| e.to_string())?;
    let mut builder = TokioResolver::builder_with_config(config, Default::default()).with_tls_config(tls);
    // A blocked provider is said so in seconds, not after several long tries: the answer to
    // a name normally comes well within this
    let options = builder.options_mut();
    options.timeout = std::time::Duration::from_secs(4);
    options.attempts = 1;
    options.num_concurrent_reqs = 2;
    let resolver = builder.build().map_err(|e| e.to_string())?;
    Ok(Arc::new(resolver))
}

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
    let addresses: Vec<SocketAddr> = match encrypted_resolver().await.filter(|_| !is_local_name(name)) {
        Some(Ok(resolver)) => resolver
            .lookup_ip(format!("{}.", name.trim_end_matches('.')))
            .await
            .map_err(|e| {
                // A name that doesn't exist, or a provider that can't be reached (some networks
                // block encrypted DNS, and sign-in pages on public Wi-Fi need their own)
                if e.is_no_records_found() {
                    format!("Couldn't find {}: there's no such name", name)
                } else {
                    format!(
                        "Couldn't find {} through encrypted DNS ({}): {}. If this network blocks encrypted \
                         DNS, or its sign-in page needs its own, choose System for DNS in settings (S)",
                        name,
                        encrypted_provider().unwrap_or_default(),
                        e
                    )
                }
            })?
            .iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect(),
        Some(Err(e)) => return Err(format!("Encrypted DNS isn't working, so rynx didn't look up {}: {}", name, e)),
        None => {
            tokio::net::lookup_host((name, port)).await.map_err(|e| format!("Couldn't find {}: {}", name, e))?.collect()
        }
    };
    if addresses.is_empty() {
        return Err(format!("Couldn't find {}", name));
    }
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
