//! Gemini (gemini://): a small protocol over TLS, with its own line-based text format.
//!
//! Most Gemini servers use self-signed certificates, so they're trusted on first use: the first
//! certificate seen for a host is remembered, and a different one later is refused unless it's
//! signed by a certificate authority. Pages are converted to HTML for the renderer.

use crate::network::{Page, PageKind, escape_html};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

pub const DEFAULT_PORT: u16 = 1965;

/// Certificates seen for each Gemini host, in the config folder
pub const KNOWN_HOSTS_FILE: &str = "gemini_known_hosts.txt";

const MAX_REDIRECTS: usize = 5;

/// Longest request the protocol allows, without the CRLF
const MAX_REQUEST_BYTES: usize = 1024;

/// The certificate fingerprint (SHA-256) remembered for each "host:port"
#[derive(Debug, Default)]
pub struct KnownHosts {
    pins: BTreeMap<String, String>,
    /// Where they're saved; None keeps them in memory only (no config folder, or tests)
    path: Option<PathBuf>,
}

impl KnownHosts {
    /// One "host:port<TAB>fingerprint" per line
    pub fn load(path: PathBuf) -> KnownHosts {
        let pins = std::fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .filter_map(|line| {
                        let (host, pin) = line.split_once('\t')?;
                        Some((host.trim().to_string(), pin.trim().to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        KnownHosts { pins, path: Some(path) }
    }

    pub fn get(&self, host: &str) -> Option<&str> {
        self.pins.get(host).map(String::as_str)
    }

    /// Remember `pin` for `host`, saving the file atomically
    pub fn set(&mut self, host: &str, pin: &str) {
        if self.get(host) == Some(pin) {
            return;
        }
        self.pins.insert(host.to_string(), pin.to_string());
        let Some(path) = &self.path else {
            return;
        };
        let text: String = self.pins.iter().map(|(h, p)| format!("{}\t{}\n", h, p)).collect();
        let temporary = path.with_extension("txt.tmp");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&temporary, text).and_then(|_| std::fs::rename(&temporary, path));
    }
}

fn fingerprint(certificate: &CertificateDer) -> String {
    ring::digest::digest(&ring::digest::SHA256, certificate.as_ref())
        .as_ref()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

/// Trust on first use: accepts the certificate remembered for this host, or any certificate for
/// a host seen for the first time, or a certificate authority-signed one. Handshake signatures
/// are always checked, so the server must hold the certificate's key.
#[derive(Debug)]
struct TofuVerifier {
    /// The fingerprint remembered for this host, if any
    known: Option<String>,
    /// The fingerprint of the certificate presented, kept once the handshake completes
    seen: Mutex<Option<String>>,
    /// Set when a remembered certificate was replaced by one that isn't CA-signed
    changed: Mutex<bool>,
    webpki: Arc<WebPkiServerVerifier>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let pin = fingerprint(end_entity);
        let accepted = match &self.known {
            None => true,
            Some(known) if *known == pin => true,
            // Certificates signed by an authority are renewed every few months; that's expected
            Some(_) => {
                self.webpki.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now).is_ok()
            }
        };
        if !accepted {
            *self.changed.lock().unwrap_or_else(|e| e.into_inner()) = true;
            return Err(rustls::Error::General("certificate changed".into()));
        }
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(pin);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// A Gemini response: two-digit status, its meta text, and the body for a success
struct Response {
    status: u8,
    meta: String,
    body: Vec<u8>,
}

/// Send one request and read the whole response
async fn request(url: &Url, known_hosts: &Mutex<KnownHosts>, limit: u64) -> Result<Response, String> {
    let host = url.host_str().ok_or("Invalid Gemini address: no host")?.to_string();
    let port = url.port().unwrap_or(DEFAULT_PORT);
    let host_key = format!("{}:{}", host, port);
    let mut line = url.clone();
    line.set_fragment(None);
    let line = line.to_string();
    if line.len() > MAX_REQUEST_BYTES {
        return Err("Gemini address too long".to_string());
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
        .build()
        .map_err(|e| e.to_string())?;
    let known = known_hosts.lock().unwrap_or_else(|e| e.into_inner()).get(&host_key).map(str::to_string);
    let verifier = Arc::new(TofuVerifier {
        known,
        seen: Mutex::new(None),
        changed: Mutex::new(false),
        webpki,
        provider: Arc::clone(&provider),
    });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::clone(&verifier) as Arc<dyn ServerCertVerifier>)
        .with_no_client_auth();
    // Hosts are sent without IPv6 brackets
    let name = ServerName::try_from(host.trim_matches(['[', ']']).to_string())
        .map_err(|_| format!("Invalid Gemini host {}", host))?;

    let exchange = async {
        let tcp = tokio::net::TcpStream::connect((host.trim_matches(['[', ']']), port))
            .await
            .map_err(|e| format!("Couldn't connect to {}: {}", host_key, e))?;
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| {
            if *verifier.changed.lock().unwrap_or_else(|e| e.into_inner()) {
                certificate_changed_message(&host_key)
            } else {
                format!("Secure connection to {} failed: {}", host_key, e)
            }
        })?;
        if let Some(pin) = verifier.seen.lock().unwrap_or_else(|e| e.into_inner()).as_deref() {
            known_hosts.lock().unwrap_or_else(|e| e.into_inner()).set(&host_key, pin);
        }
        tls.write_all(format!("{}\r\n", line).as_bytes()).await.map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        // Many servers close the connection without a TLS close_notify; what arrived is complete
        // as far as the protocol can tell, so an unexpected end of stream is accepted
        match (&mut tls).take(limit + 1).read_to_end(&mut data).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !data.is_empty() => {}
            Err(e) => return Err(format!("Reading from {} failed: {}", host_key, e)),
        }
        if data.len() as u64 > limit {
            return Err("Page too large".to_string());
        }
        Ok::<_, String>(data)
    };
    let timeout = std::time::Duration::from_secs(crate::constants::BROWSING_TIMEOUT_SECS);
    let data = tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| format!("{} took too long to respond", host_key))??;
    parse_response(&data)
}

fn certificate_changed_message(host: &str) -> String {
    format!(
        "{}'s certificate has changed since your last visit, and the new one isn't signed by a \
         certificate authority. Someone may be intercepting the connection. If the site says it \
         changed its certificate, remove its line from {} in the settings folder.",
        host, KNOWN_HOSTS_FILE
    )
}

fn parse_response(data: &[u8]) -> Result<Response, String> {
    let end = data
        .iter()
        .position(|&b| b == b'\n')
        .filter(|&end| end <= MAX_REQUEST_BYTES + 3)
        .ok_or("Invalid Gemini response: no header")?;
    let header = String::from_utf8_lossy(&data[..end]);
    let header = header.trim_end_matches('\r');
    let (code, meta) = header.split_once([' ', '\t']).unwrap_or((header, ""));
    let status = match code.as_bytes() {
        [a @ b'1'..=b'6', b @ b'0'..=b'9'] => (a - b'0') * 10 + (b - b'0'),
        _ => return Err(format!("Invalid Gemini response: \"{}\"", code.chars().take(20).collect::<String>())),
    };
    Ok(Response { status, meta: meta.trim().to_string(), body: data[end + 1..].to_vec() })
}

/// Fetch a Gemini page, following redirects, as a page for the renderer
pub async fn load(url: &str, known_hosts: &Mutex<KnownHosts>, limit: u64) -> Result<Page, String> {
    let mut current = Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
    for _ in 0..=MAX_REDIRECTS {
        if !current.username().is_empty() || current.password().is_some() {
            return Err("Gemini addresses can't contain a user name or password".to_string());
        }
        let response = request(&current, known_hosts, limit).await?;
        match response.status {
            10..=19 => return Ok(input_page(&current, &response.meta, response.status == 11)),
            20..=29 => return Ok(success_page(&current, &response.meta, &response.body)),
            30..=39 => {
                let next = current
                    .join(&response.meta)
                    .map_err(|_| format!("Invalid redirect location: {}", response.meta))?;
                if next.scheme() != "gemini" {
                    return Err(format!("Blocked redirect from Gemini to {}", next));
                }
                if crate::network::is_local_network_url(next.as_str())
                    && !crate::network::is_local_network_url(current.as_str())
                {
                    return Err("Blocked redirect to local network (SSRF Protection)".to_string());
                }
                current = next;
            }
            40..=59 => {
                let kind = match response.status {
                    44 => "Slow down: the server asks you to wait",
                    51 => "Not found",
                    52 => "Gone",
                    53 => "Proxy request refused",
                    59 => "Bad request",
                    40..=49 => "Temporary failure",
                    _ => "Permanent failure",
                };
                let detail = if response.meta.is_empty() { String::new() } else { format!(": {}", response.meta) };
                return Err(format!("{} ({}){}", kind, response.status, detail));
            }
            _ => return Err("This page needs a client certificate, which rynx doesn't support yet".to_string()),
        }
    }
    Err("Too many redirects".to_string())
}

/// A page asking for input (status 10, or 11 for a password), which is sent as the query
fn input_page(url: &Url, prompt: &str, sensitive: bool) -> Page {
    let mut action = url.clone();
    action.set_query(None);
    action.set_fragment(None);
    let prompt = if prompt.is_empty() { "Enter text" } else { prompt };
    let field = if sensitive { "password" } else { "text" };
    let body = format!(
        "<title>{title}</title><h1>{title}</h1><p>{prompt}</p>\
         <form action=\"{action}\"><input type=\"{field}\" name=\"q\" size=\"60\"> \
         <input type=\"submit\" value=\"Send\"></form>",
        title = escape_html(url.host_str().unwrap_or("Gemini")),
        prompt = escape_html(prompt),
        action = escape_html(action.as_str()),
    );
    Page { title: crate::network::title_from_url(url.as_str()), body, url: url.to_string(), kind: PageKind::Html }
}

fn success_page(url: &Url, meta: &str, body: &[u8]) -> Page {
    let meta = if meta.is_empty() { "text/gemini; charset=utf-8" } else { meta };
    let mime = meta.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let charset = meta
        .split(';')
        .skip(1)
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
        .map(|(_, v)| v.trim().trim_matches('"'));
    let encoding = charset.and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes())).unwrap_or(encoding_rs::UTF_8);
    let page_url = url.to_string();
    let title = crate::network::title_from_url(&page_url);
    match mime.as_str() {
        "text/gemini" => {
            let (text, _, _) = encoding.decode(body);
            let (heading, html) = gemtext_to_html(&text);
            Page { title: heading.unwrap_or(title), body: html, url: page_url, kind: PageKind::Html }
        }
        "text/html" => Page { title, body: encoding.decode(body).0.into_owned(), url: page_url, kind: PageKind::Html },
        m if crate::network::classify_content_type(Some(m)) == crate::network::ContentClass::Text => {
            Page { title, body: encoding.decode(body).0.into_owned(), url: page_url, kind: PageKind::PlainText }
        }
        _ => {
            let body = format!(
                "<title>{name}</title><h1>{name}</h1><p>This is a file of type <code>{mime}</code> \
                 ({size} bytes), which can't be shown as a page. Downloading from Gemini isn't \
                 supported yet.</p>",
                name = escape_html(&title),
                mime = escape_html(&mime),
                size = body.len(),
            );
            Page { title, body, url: page_url, kind: PageKind::Html }
        }
    }
}

/// Convert gemtext to HTML, returning the first heading as the title. Every line is one block:
/// text, a link (`=>`), a heading (`#`), a list item (`*`), a quote (`>`), or preformatted
/// text between lines starting with ```.
pub fn gemtext_to_html(text: &str) -> (Option<String>, String) {
    let mut html = String::new();
    let mut title = None;
    let mut preformatted = false;
    let mut in_list = false;
    for line in text.lines() {
        if line.starts_with("```") {
            if in_list {
                html.push_str("</ul>");
                in_list = false;
            }
            html.push_str(if preformatted { "</pre>" } else { "<pre>" });
            preformatted = !preformatted;
            continue;
        }
        if preformatted {
            html.push_str(&escape_html(line));
            html.push('\n');
            continue;
        }
        let item = line.strip_prefix("* ");
        if item.is_none() && in_list {
            html.push_str("</ul>");
            in_list = false;
        }
        if let Some(item) = item {
            if !in_list {
                html.push_str("<ul>");
                in_list = true;
            }
            html.push_str(&format!("<li>{}</li>", escape_html(item.trim())));
        } else if let Some(link) = line.strip_prefix("=>") {
            let link = link.trim();
            let (target, label) = link.split_once(char::is_whitespace).unwrap_or((link, ""));
            let label = label.trim();
            let label = if label.is_empty() { target } else { label };
            html.push_str(&format!("<div>⇒ <a href=\"{}\">{}</a></div>", escape_html(target), escape_html(label)));
        } else if let Some((level, heading)) = heading(line) {
            if title.is_none() && !heading.is_empty() {
                title = Some(heading.to_string());
            }
            html.push_str(&format!("<h{0}>{1}</h{0}>", level, escape_html(heading)));
        } else if let Some(quote) = line.strip_prefix('>') {
            html.push_str(&format!("<blockquote>{}</blockquote>", escape_html(quote.trim())));
        } else if line.trim().is_empty() {
            html.push_str("<br>");
        } else {
            html.push_str(&format!("<div>{}</div>", escape_html(line)));
        }
    }
    if in_list {
        html.push_str("</ul>");
    }
    if preformatted {
        html.push_str("</pre>");
    }
    (title, html)
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    (1..=3).contains(&level).then(|| (level, line[level..].trim()))
}
