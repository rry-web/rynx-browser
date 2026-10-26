use crate::constants::{
    BROWSING_TIMEOUT_SECS, DOWNLOAD_TIMEOUT_SECS, I2P_PROXY_URL, JUMP_SERVICES, MAX_REDIRECTS,
    USER_AGENT,
};
use crate::models::PageMetadata;
use reqwest::{Client, StatusCode};
use scraper::{Html, Selector};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use url::{Host, Url};

pub enum NetworkResponse {
    Success(usize, String, String, String), // tab_id, title, html, final URL after redirects
    Error(usize, String),
    Loading(usize),
    Info(usize, String),
    // Variant for downloads
    DownloadProgress(usize, u64, Option<u64>),
    DownloadFinished(usize, String), // tab_id, filename
    DownloadError(usize, String),    // tab_id, message; leaves the page untouched
}

/// The #fragment of a URL, if it has one
pub fn fragment_of(url: &str) -> Option<String> {
    Url::parse(url).ok()?.fragment().map(str::to_string)
}

/// The scheme of a link the browser can't open (mailto:, javascript:, file:, data:, ...).
/// Such links used to be treated as typed text, leaking them to unrelated hosts or search.
pub fn unsupported_link_scheme(url: &str) -> Option<String> {
    let scheme = Url::parse(url).ok()?.scheme().to_string();
    (scheme != "http" && scheme != "https").then_some(scheme)
}

/// Resolve relative URLs against a base URL
pub fn resolve_url(base: &str, target: &str) -> String {
    // If target is already a full URL (e.g. https://google.com), return it immediately
    if let Ok(url) = Url::parse(target) {
        return url.to_string();
    }

    // Handle internal pages or empty bases
    if base.is_empty() || base.starts_with("about:") || base == "New Tab" {
        // If we are on a help page, relative links can't be resolved,
        // so we treat the target as a potential new absolute URL or search query.
        return target.to_string();
    }

    // Try standard joining
    match Url::parse(base) {
        Ok(base_url) => {
            match base_url.join(target) {
                Ok(joined) => joined.to_string(),
                Err(_) => target.to_string(), // Fallback to target string if join fails
            }
        }
        Err(_) => target.to_string(), // Fallback if base is unparseable
    }
}

pub struct NetworkManager {
    client: Client,
    i2p_client: Client,
    download_client: Client,
    i2p_download_client: Client,
}

impl NetworkManager {
    /// Private helper method to build a reqwest client with consistent configuration
    fn build_client(
        user_agent: &str,
        timeout: Duration,
        use_proxy: bool,
        include_headers: bool,
    ) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
        let mut builder = Client::builder().user_agent(user_agent).timeout(timeout);

        // Enforce TLS 1.2 as minimum for security (only for non-I2P clients)
        if !use_proxy {
            builder = builder
                .use_rustls_tls()
                .min_tls_version(reqwest::tls::Version::TLS_1_2);
        }

        if include_headers {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("Referer", reqwest::header::HeaderValue::from_static(""));
            builder = builder.default_headers(headers);
        }

        if use_proxy {
            let proxy = reqwest::Proxy::http(I2P_PROXY_URL)?;
            builder = builder.proxy(proxy);
        }

        // Redirects are followed by get_following_redirects so every hop is checked and upgraded
        builder = builder.redirect(reqwest::redirect::Policy::none());

        Ok(builder.build()?)
    }

    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create all four clients using the build_client helper method
        let client = Self::build_client(
            USER_AGENT,
            Duration::from_secs(BROWSING_TIMEOUT_SECS),
            false,
            true,
        )?;
        let i2p_client = Self::build_client(
            USER_AGENT,
            Duration::from_secs(BROWSING_TIMEOUT_SECS),
            true,
            true,
        )?;
        let download_client = Self::build_client(
            USER_AGENT,
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            false,
            false,
        )?;
        let i2p_download_client = Self::build_client(
            USER_AGENT,
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            true,
            false,
        )?;

        Ok(Self {
            client,
            i2p_client,
            download_client,
            i2p_download_client,
        })
    }

    pub fn get_client(&self, i2p_mode: bool) -> &Client {
        if i2p_mode {
            &self.i2p_client
        } else {
            &self.client
        }
    }

    pub fn get_download_client(&self, i2p_mode: bool) -> &Client {
        if i2p_mode {
            &self.i2p_download_client
        } else {
            &self.download_client
        }
    }
}

pub fn parse_html_metadata(html: &str) -> PageMetadata {
    let document = Html::parse_document(html);
    static TITLE_SELECTOR: OnceLock<Selector> = OnceLock::new();
    let title_selector = TITLE_SELECTOR.get_or_init(|| Selector::parse("title").unwrap());

    let title = document
        .select(title_selector)
        .next()
        .map(|element| {
            element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "No Title".to_string());

    PageMetadata { title }
}

/// Loopback hosts, which are reachable over plain HTTP for local development
fn is_loopback_host(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost") || d.ends_with(".localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Hosts on this machine or the local network, which a public page must not redirect into
fn is_local_network_host(url: &Url) -> bool {
    if is_loopback_host(url) {
        return true;
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => {
            ip.is_private() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast()
        }
        Some(Host::Ipv6(ip)) => {
            let first = ip.segments()[0];
            ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
                || ip.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                })
        }
        _ => false,
    }
}

fn is_i2p_host(url: &Url) -> bool {
    url.host_str().is_some_and(|h| h.ends_with(".i2p"))
}

/// Upgrade http:// to https:// for clearweb hosts. Loopback addresses and .i2p sites stay on
/// http, and nothing is upgraded in I2P mode, where traffic goes through the I2P proxy.
pub fn enforce_https(url: &str, i2p_mode: bool) -> String {
    match Url::parse(url) {
        Ok(mut parsed)
            if !i2p_mode
                && parsed.scheme() == "http"
                && !is_i2p_host(&parsed)
                && !is_loopback_host(&parsed) =>
        {
            // Only the scheme changes; set_scheme between http and https always succeeds
            let _ = parsed.set_scheme("https");
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

/// Reject redirects to non-web schemes, or from a public site into the local network (SSRF)
pub fn check_redirect(from: &Url, to: &Url) -> Result<(), String> {
    if to.scheme() != "http" && to.scheme() != "https" {
        return Err(format!("Blocked redirect to unsupported scheme: {}", to.scheme()));
    }
    if is_local_network_host(to) && !is_local_network_host(from) {
        return Err("Blocked redirect to local network (SSRF Protection)".to_string());
    }
    Ok(())
}

/// GET a URL, following redirects manually so every hop is upgraded to HTTPS and SSRF-checked
pub async fn get_following_redirects(
    client: &Client,
    url: &str,
    i2p_mode: bool,
) -> Result<reqwest::Response, String> {
    let mut current =
        Url::parse(&enforce_https(url, i2p_mode)).map_err(|e| format!("Invalid URL: {}", e))?;
    for _ in 0..=MAX_REDIRECTS {
        // Without the I2P proxy the name would be looked up through ordinary DNS, telling the
        // resolver which eepsite is being visited
        if !i2p_mode && is_i2p_host(&current) {
            return Err("Enable I2P mode (press p) to visit .i2p sites".to_string());
        }
        let response = client
            .get(current.clone())
            .send()
            .await
            .map_err(|e| e.to_string())?;
        // A 3xx without Location (e.g. 304) is returned as-is
        let location = match response.headers().get(reqwest::header::LOCATION) {
            Some(location) if response.status().is_redirection() => location,
            _ => return Ok(response),
        };
        let location = location
            .to_str()
            .map_err(|_| "Redirect location is not valid text".to_string())?;
        let next = current
            .join(location)
            .map_err(|e| format!("Invalid redirect location: {}", e))?;
        let next = Url::parse(&enforce_https(next.as_str(), i2p_mode))
            .map_err(|e| format!("Invalid redirect location: {}", e))?;
        check_redirect(&current, &next)?;
        current = next;
    }
    Err("Too many redirects".to_string())
}

/// Jump services only know I2P addresses, so only consult them for .i2p sites in I2P mode
pub fn should_attempt_jump(use_i2p: bool, domain: &str, status: StatusCode) -> bool {
    use_i2p
        && domain.ends_with(".i2p")
        && (status == StatusCode::INTERNAL_SERVER_ERROR
            || status == StatusCode::SERVICE_UNAVAILABLE)
}

pub async fn attempt_jump(
    client: &Client,
    target_domain: &str,
    tx: mpsc::Sender<NetworkResponse>,
    id: usize,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    attempt_jump_with(client, JUMP_SERVICES, target_domain, tx, id).await
}

/// Try each jump service in order, moving on when one is unreachable or has no entry
pub async fn attempt_jump_with(
    client: &Client,
    services: &[&str],
    target_domain: &str,
    tx: mpsc::Sender<NetworkResponse>,
    id: usize,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let _ = tx
        .send(NetworkResponse::Info(
            id,
            "Address not found. Requesting Jump Helper...".to_string(),
        ))
        .await;
    for service_base in services {
        let jump_url = format!("{}{}", service_base, target_domain);
        let _ = tx
            .send(NetworkResponse::Info(
                id,
                format!("Contacting jump service: {}", service_base),
            ))
            .await;
        match get_following_redirects(client, &jump_url, true).await {
            Ok(response) if response.status() == StatusCode::OK => return Ok(response),
            // Unreachable or no entry; try the next service
            Ok(_) | Err(_) => continue,
        }
    }
    Err("All jump services failed.".into())
}

/// The charset parameter of a Content-Type header, e.g. "text/html; charset=ISO-8859-1"
fn charset_from_content_type(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"'))
    })
}

/// Read a response body as text without buffering more than `limit` bytes. Content-Length
/// can't enforce this: it is absent for chunked responses and for compressed ones, where a
/// small download can expand enormously. Decodes using the declared charset, like resp.text().
pub async fn read_text_limited(mut resp: reqwest::Response, limit: u64) -> Result<String, String> {
    let encoding = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(charset_from_content_type)
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);

    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if (body.len() + chunk.len()) as u64 > limit {
            return Err(format!("Page too large (over {} MB)", limit / (1024 * 1024)));
        }
        body.extend_from_slice(&chunk);
    }
    // decode() also honours a byte order mark, as resp.text() does
    let (text, _, _) = encoding.decode(&body);
    Ok(text.into_owned())
}

/// Detects file extensions based on magic bytes (file signatures)
pub fn sniff_extension(h: &[u8]) -> Option<&'static str> {
    if h.starts_with(&[0x89, 0x50, 0x4E, 0x47]) { Some("png") }
    else if h.starts_with(&[0xFF, 0xD8, 0xFF]) { Some("jpg") }
    else if h.starts_with(&[0x25, 0x50, 0x44, 0x46]) { Some("pdf") }
    else if h.starts_with(&[0x50, 0x4B, 0x03, 0x04]) { Some("zip") }
    else if h.starts_with(&[0x4D, 0x5A]) { Some("exe") }
    else if h.starts_with(b"GIF8") { Some("gif") }
    else { None }
}
