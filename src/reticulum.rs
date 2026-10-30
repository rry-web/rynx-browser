//! NomadNet pages on the Reticulum network, through rynx's bridge (bridges/reticulum/).
//!
//! A NomadNet address is nomadnet://<node>/<path>: the node's 32 hexadecimal digits, then a path
//! such as /page/index.mu (the default) or /file/notes.txt. rynx asks the bridge, a small program
//! on this computer using Reticulum's reference software, for http://127.0.0.1:4243/<node>/<path>,
//! with an X-Rynx header the bridge requires (so other browsers' pages can't use it), and shows
//! the page under its nomadnet:// address, so its links lead on through the bridge.

use std::sync::RwLock;
use url::Url;

pub const SCHEME: &str = "nomadnet";

/// Where the bridge listens unless settings say otherwise
pub const DEFAULT_BRIDGE: &str = "http://127.0.0.1:4243";

/// The header the bridge requires
pub const BRIDGE_HEADER: &str = "x-rynx";

/// A node's address: 32 hexadecimal digits
const NODE_DIGITS: usize = 32;

static BRIDGE: RwLock<String> = RwLock::new(String::new());

/// Set where the bridge listens (reticulum_bridge in settings)
pub fn set_bridge(address: &str) {
    *BRIDGE.write().unwrap_or_else(|e| e.into_inner()) = address.trim_end_matches('/').to_string();
}

/// Where the bridge listens
pub fn bridge() -> String {
    let set = BRIDGE.read().unwrap_or_else(|e| e.into_inner()).clone();
    if set.is_empty() { DEFAULT_BRIDGE.to_string() } else { set }
}

/// A NomadNet address as NomadNet writes it ("<node>:/page/index.mu", or the node alone) as a
/// nomadnet:// one, or None if it isn't one
pub fn from_nomadnet_form(text: &str) -> Option<String> {
    let (node, path) = text.split_once(':').unwrap_or((text, ""));
    let is_node = node.len() == NODE_DIGITS && node.chars().all(|c| c.is_ascii_hexdigit());
    let path_ok = path.is_empty() || path.starts_with('/');
    (is_node && path_ok)
        .then(|| format!("{}://{}{}", SCHEME, node.to_ascii_lowercase(), if path.is_empty() { "/" } else { path }))
}

pub fn is_nomadnet(url: &str) -> bool {
    url.get(..SCHEME.len() + 3).is_some_and(|start| start.eq_ignore_ascii_case("nomadnet://"))
}

/// The bridge's address for a nomadnet:// address, or None if it isn't a node's
pub fn bridge_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let node = parsed.host_str()?.to_ascii_lowercase();
    if parsed.scheme() != SCHEME || node.len() != NODE_DIGITS || !node.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let path = match parsed.path() {
        "" | "/" => "/page/index.mu",
        path => path,
    };
    let query = parsed.query().map(|q| format!("?{}", q)).unwrap_or_default();
    Some(format!("{}/{}{}{}", bridge(), node, path, query))
}

/// Whether a request goes to the bridge, and so carries its header
pub fn is_bridge(url: &Url) -> bool {
    Url::parse(&bridge()).is_ok_and(|b| b.origin() == url.origin())
}
