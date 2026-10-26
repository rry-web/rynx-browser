use crate::constants::{
    BROWSING_TIMEOUT_SECS, DOWNLOAD_TIMEOUT_SECS, I2P_PROXY_URL, I2P_USER_AGENT, JUMP_SERVICES,
    MAX_REDIRECTS, USER_AGENT,
};
use reqwest::{Client, StatusCode};
use std::time::Duration;
use tokio::sync::mpsc;
use url::{Host, Url};

pub enum NetworkResponse {
    Success(usize, String, String, String, PageKind), // tab_id, title, body, final URL, kind
    Error(usize, String),
    Loading(usize),
    Info(usize, String),
    // Variant for downloads
    DownloadProgress(usize, u64, Option<u64>),
    DownloadFinished(usize, String), // tab_id, filename
    DownloadError(usize, String),    // tab_id, message; leaves the page untouched
    /// A page laid out by a background render: tab_id, content version it was made from
    Rendered(usize, u64, Box<crate::renderer::RenderedPage>),
    /// A server's tools: tab_id, the tools page address, server, tools
    ToolsListed(usize, String, String, Result<Vec<crate::mcp::Tool>, String>),
    /// A tool finished: tab_id, server, tool title, result
    ToolFinished(usize, String, String, Result<serde_json::Value, String>),
}

/// A fetched page, ready to show
#[derive(Debug)]
pub struct Page {
    pub title: String,
    pub body: String,
    /// Where it was finally loaded from, after redirects
    pub url: String,
    pub kind: PageKind,
}

/// How a page's body is shown
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageKind {
    Html,
    /// Plain text, JSON, source code...: shown as text with its lines kept
    PlainText,
}

/// What a response's Content-Type says it holds
#[derive(Debug, PartialEq)]
pub enum ContentClass {
    Html,
    Text,
    /// Not displayable as a page (images, PDFs, archives); carries the MIME type
    Binary(String),
}

pub fn classify_content_type(content_type: Option<&str>) -> ContentClass {
    let mime = content_type
        .and_then(|ct| ct.split(';').next())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        // A missing type is treated as HTML, as before
        "" | "text/html" | "application/xhtml+xml" => ContentClass::Html,
        m if m.starts_with("text/") => ContentClass::Text,
        "application/json" | "application/xml" | "application/javascript"
        | "application/ecmascript" | "application/x-javascript" | "application/x-sh"
        | "application/toml" | "application/yaml" | "application/x-yaml" | "application/sql" => {
            ContentClass::Text
        }
        m if m.ends_with("+json") || m.ends_with("+xml") => ContentClass::Text,
        _ => ContentClass::Binary(mime),
    }
}

/// Escape text for inclusion in generated HTML, so it can't inject markup or links
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A short name for a page that has no <title>: its file name, or else its host
pub fn title_from_url(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return url.to_string();
    };
    let segment = parsed
        .path_segments()
        .and_then(|mut segments| segments.rfind(|s| !s.is_empty()))
        .map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned());
    segment
        .or_else(|| parsed.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}

fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["bytes", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} bytes", bytes)
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

/// A page standing in for a file that can't be shown, offering it for download.
/// Returns (title, html).
pub fn binary_notice_page(url: &str, mime: &str, size: Option<u64>) -> (String, String) {
    let name = title_from_url(url);
    let size = size.map(|s| format!(", {}", format_size(s))).unwrap_or_default();
    let html = format!(
        "<h1>{name}</h1><p>This is a <b>{mime}</b> file{size}, which can't be shown as a page.</p>\
         <p><a href=\"{url}\">Download {name}</a></p>\
         <p>Select the link and press <b>d</b> to save it to your Downloads folder.</p>",
        name = escape_html(&name),
        mime = escape_html(mime),
        size = escape_html(&size),
        url = escape_html(url),
    );
    (name, html)
}

/// The #fragment of a URL, if it has one
pub fn fragment_of(url: &str) -> Option<String> {
    Url::parse(url).ok()?.fragment().map(str::to_string)
}

/// The scheme of a link the browser can't open (mailto:, javascript:, file:, data:, ...).
/// Such links used to be treated as typed text, leaking them to unrelated hosts or search.
pub fn unsupported_link_scheme(url: &str) -> Option<String> {
    let scheme = Url::parse(url).ok()?.scheme().to_string();
    (!matches!(scheme.as_str(), "http" | "https" | "gemini" | "gopher")).then_some(scheme)
}

/// Whether `url` uses one of the small-web protocols (Gemini or Gopher) rather than the web
pub fn is_small_web(url: &str) -> bool {
    url.starts_with("gemini://") || url.starts_with("gopher://")
}

/// Whether `url` is on this machine or the local network (localhost, 127.x, 10.x, 192.168.x,
/// link-local and similar addresses)
pub fn is_local_network_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| is_local_network_host(&u))
}

/// The real site of an address that hides it behind a user name or password:
/// "evil.example" for https://paypal.com@evil.example/. None for ordinary addresses.
pub fn hidden_destination(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    if parsed.username().is_empty() && parsed.password().is_none() {
        return None;
    }
    parsed.host_str().map(str::to_string)
}

/// `url` without any user name or password, e.g. before it's written to disk
pub fn without_credentials(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) if !parsed.username().is_empty() || parsed.password().is_some() => {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        _ => url.to_string(),
    }
}

/// `url` with any password replaced by dots, for showing on screen
pub fn mask_password(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) if parsed.password().is_some() => {
            let _ = parsed.set_password(Some("••••"));
            // The dots are percent-encoded in the serialized URL; show them as dots
            parsed.to_string().replacen("%E2%80%A2%E2%80%A2%E2%80%A2%E2%80%A2", "••••", 1)
        }
        _ => url.to_string(),
    }
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
        proxy_url: &str,
    ) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
        let mut builder = Client::builder().user_agent(user_agent).timeout(timeout);

        // The OS verifier fetches revocation data directly, outside the I2P proxy, which would
        // reveal the site being visited; I2P clients use the bundled webpki roots instead
        builder = builder.use_preconfigured_tls(Self::tls_config(!use_proxy)?);

        if use_proxy {
            let proxy = reqwest::Proxy::http(proxy_url)?;
            builder = builder.proxy(proxy);
        }

        // Redirects are followed by get_following_redirects so every hop is checked and upgraded
        builder = builder.redirect(reqwest::redirect::Policy::none());

        Ok(builder.build()?)
    }

    /// TLS settings for every client. With `use_os_verifier`, certificates are checked by the
    /// operating system, which uses its own trust store (kept current by OS updates) and, on macOS
    /// and Windows, checks revocation. Otherwise they're checked against the bundled webpki roots.
    /// reqwest uses the config as-is, so the TLS 1.2 minimum and ALPN are set here.
    fn tls_config(
        use_os_verifier: bool,
    ) -> Result<rustls::ClientConfig, Box<dyn std::error::Error + Send + Sync>> {
        use rustls_platform_verifier::BuilderVerifierExt;

        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        let builder = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?;
        let mut config = if use_os_verifier {
            builder.with_platform_verifier()?.with_no_client_auth()
        } else {
            let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            builder.with_root_certificates(roots).with_no_client_auth()
        };
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(config)
    }

    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_i2p_proxy(I2P_PROXY_URL)
    }

    /// Clients for browsing and downloading, with I2P traffic sent through `i2p_proxy`
    pub fn with_i2p_proxy(i2p_proxy: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_settings(i2p_proxy, USER_AGENT)
    }

    /// Clients introducing themselves as `user_agent` on the clearweb, with I2P traffic sent
    /// through `i2p_proxy`
    pub fn with_settings(i2p_proxy: &str, user_agent: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Our clients carry their own TLS config, but a reqwest Client built without one panics
        // when no process-wide crypto provider is installed. Already-installed is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Create all four clients using the build_client helper method
        let client = Self::build_client(
            user_agent,
            Duration::from_secs(BROWSING_TIMEOUT_SECS),
            false,
            i2p_proxy,
        )?;
        let i2p_client = Self::build_client(
            I2P_USER_AGENT,
            Duration::from_secs(BROWSING_TIMEOUT_SECS),
            true,
            i2p_proxy,
        )?;
        let download_client = Self::build_client(
            user_agent,
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            false,
            i2p_proxy,
        )?;
        let i2p_download_client = Self::build_client(
            I2P_USER_AGENT,
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            true,
            i2p_proxy,
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

/// Loopback hosts, which are reachable over plain HTTP for local development
/// The URL's host, with IP addresses recognised for every scheme. For schemes other than the
/// web's (gopher://, gemini://), the url crate keeps hosts like 127.0.0.1, or its disguises like
/// 2130706433, as plain names.
fn host_of(url: &Url) -> Option<Host<String>> {
    match url.host()? {
        Host::Domain(d) => Some(Host::parse(d).unwrap_or_else(|_| Host::Domain(d.to_string()))),
        host => Some(host.to_owned()),
    }
}

fn is_loopback_host(url: &Url) -> bool {
    match host_of(url) {
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
    match host_of(url) {
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
    send_following_redirects(client, url, i2p_mode, None).await
}

/// GET a URL, or POST a form body to it, following redirects like get_following_redirects. A
/// redirect turns a POST into a GET (as browsers do), except 307/308, which repeat the POST.
pub async fn send_following_redirects(
    client: &Client,
    url: &str,
    i2p_mode: bool,
    mut form_body: Option<String>,
) -> Result<reqwest::Response, String> {
    let mut current =
        Url::parse(&enforce_https(url, i2p_mode)).map_err(|e| format!("Invalid URL: {}", e))?;
    for _ in 0..=MAX_REDIRECTS {
        // Without the I2P proxy the name would be looked up through ordinary DNS, telling the
        // resolver which eepsite is being visited
        if !i2p_mode && is_i2p_host(&current) {
            return Err("Enable I2P mode (press p) to visit .i2p sites".to_string());
        }
        let request = match &form_body {
            Some(body) => client
                .post(current.clone())
                .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(body.clone()),
            None => client.get(current.clone()),
        };
        let response = request.send().await.map_err(|e| e.to_string())?;
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
        if !matches!(response.status().as_u16(), 307 | 308) {
            form_body = None;
        }
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

#[cfg(test)]
mod tests {
    use super::NetworkManager;

    /// A client with the I2P clients' TLS config but no proxy, so it can be checked without a router
    fn bundled_roots_client() -> reqwest::Client {
        reqwest::Client::builder()
            .use_preconfigured_tls(NetworkManager::tls_config(false).unwrap())
            .build()
            .unwrap()
    }

    // Live checks: run with `cargo test --lib -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn i2p_tls_config_connects_over_http2() {
        let resp = bundled_roots_client().get("https://www.rust-lang.org/").send().await.unwrap();
        assert_eq!(resp.version(), reqwest::Version::HTTP_2);
    }

    #[tokio::test]
    #[ignore]
    async fn i2p_tls_config_rejects_bad_certificates_and_old_tls() {
        let client = bundled_roots_client();
        for url in [
            "https://expired.badssl.com/",
            "https://self-signed.badssl.com/",
            "https://wrong.host.badssl.com/",
            "https://tls-v1-1.badssl.com:1011/",
        ] {
            assert!(client.get(url).send().await.is_err(), "{url} was accepted");
        }
    }
}
