//! Gopher (gopher://): menus of links and plain-text documents, over unencrypted TCP.
//!
//! Addresses follow RFC 4266: gopher://host:port/<type><selector>, with a search query after
//! %09. Menus are converted to HTML for the renderer.

use crate::network::{Page, PageKind, escape_html};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

pub const DEFAULT_PORT: u16 = 70;

/// Characters escaped in selectors when building gopher:// addresses
const SELECTOR: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'%').add(b'<').add(b'>').add(b'?').add(b'`');

/// A gopher address taken apart
#[derive(Debug, PartialEq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub item_type: char,
    pub selector: String,
    pub query: Option<String>,
}

/// Split a gopher:// address into what's sent and how the answer is shown
pub fn parse(url: &str) -> Result<Target, String> {
    let url = Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
    let host = url.host_str().ok_or("Invalid Gopher address: no host")?.trim_matches(['[', ']']).to_string();
    let port = url.port().unwrap_or(DEFAULT_PORT);
    // The path is "/<type><selector>"; the query part of the URL belongs to the selector too
    let mut path = url.path().strip_prefix('/').unwrap_or(url.path()).to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let path = percent_decode_str(&path).decode_utf8_lossy().into_owned();
    let mut chars = path.chars();
    let item_type = chars.next().unwrap_or('1');
    let rest: String = chars.collect();
    let (selector, query) = match rest.split_once('\t') {
        Some((selector, query)) => (selector.to_string(), Some(query.to_string())),
        None => (rest, None),
    };
    // A selector is sent as-is, so line breaks in it would let a page send any command to the
    // server; gopher links have been used to attack local services (e.g. Redis) this way
    let unsafe_text = |s: &str| s.chars().any(|c| matches!(c, '\r' | '\n' | '\0') || (c == '\t'));
    if unsafe_text(&selector) || query.as_deref().is_some_and(unsafe_text) {
        return Err("Blocked: this Gopher address contains control characters".to_string());
    }
    Ok(Target { host, port, item_type, selector, query })
}

/// The gopher:// address of a menu item
pub fn item_url(item_type: char, selector: &str, host: &str, port: u16) -> String {
    let host = if host.contains(':') { format!("[{}]", host) } else { host.to_string() };
    let port = if port == DEFAULT_PORT { String::new() } else { format!(":{}", port) };
    format!("gopher://{}{}/{}{}", host, port, item_type, utf8_percent_encode(selector, SELECTOR))
}

/// Fetch a gopher page as a page for the renderer
pub async fn load(url: &str, limit: u64) -> Result<Page, String> {
    let target = parse(url)?;
    // A search with nothing to search for yet: ask for the words first
    if target.item_type == '7' && target.query.is_none() {
        return Ok(search_page(url));
    }
    let request = match &target.query {
        Some(query) => format!("{}\t{}\r\n", target.selector, query),
        None => format!("{}\r\n", target.selector),
    };
    let host_port = format!("{}:{}", target.host, target.port);
    let exchange = async {
        let mut stream = tokio::net::TcpStream::connect((target.host.as_str(), target.port))
            .await
            .map_err(|e| format!("Couldn't connect to {}: {}", host_port, e))?;
        stream.write_all(request.as_bytes()).await.map_err(|e| e.to_string())?;
        let mut data = Vec::new();
        (&mut stream)
            .take(limit + 1)
            .read_to_end(&mut data)
            .await
            .map_err(|e| format!("Reading from {} failed: {}", host_port, e))?;
        if data.len() as u64 > limit {
            return Err("Page too large".to_string());
        }
        Ok::<_, String>(data)
    };
    let timeout = std::time::Duration::from_secs(crate::constants::BROWSING_TIMEOUT_SECS);
    let data = tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| format!("{} took too long to respond", host_port))??;
    let text = String::from_utf8_lossy(&data);
    let title = crate::network::title_from_url(url);
    let page = |body, kind| Page { title: title.clone(), body, url: url.to_string(), kind };
    Ok(match target.item_type {
        '1' | '7' => page(menu_to_html(&text), PageKind::Html),
        '0' => page(text_document(&text), PageKind::PlainText),
        'h' => page(text.into_owned(), PageKind::Html),
        other => page(
            format!(
                "<title>{name}</title><h1>{name}</h1><p>This is a Gopher item of type \
                 <code>{other}</code> ({size} bytes), which can't be shown as a page. Downloading \
                 from Gopher isn't supported yet.</p>",
                name = escape_html(&title),
                other = escape_html(&other.to_string()),
                size = data.len(),
            ),
            PageKind::Html,
        ),
    })
}

/// A text document without the "." line that ends it, and with doubled leading dots undone
fn text_document(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line == "." {
            break;
        }
        out.push_str(line.strip_prefix("..").map(|l| format!(".{}", l)).as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

/// A page asking for search words for a type 7 item, sent as "<selector><TAB><words>"
fn search_page(url: &str) -> Page {
    let title = crate::network::title_from_url(url);
    let body = format!(
        "<title>Search</title><h1>Search</h1><p>{}</p>\
         <form action=\"{}\"><input type=\"text\" name=\"q\" size=\"60\"> \
         <input type=\"submit\" value=\"Search\"></form>",
        escape_html(&title),
        escape_html(url),
    );
    Page { title, body, url: url.to_string(), kind: PageKind::Html }
}

/// What a menu item is, shown before its name
fn type_label(item_type: char) -> &'static str {
    match item_type {
        '0' => "[TXT]",
        '1' => "[DIR]",
        '7' => "[ ? ]",
        'h' => "[WWW]",
        'g' | 'I' | 'p' => "[IMG]",
        's' | '<' => "[SND]",
        ';' => "[VID]",
        'd' => "[DOC]",
        '8' | 'T' => "[TEL]",
        _ => "[BIN]",
    }
}

/// Convert a gopher menu to HTML. Menus are often laid out as fixed-width art, so it's kept as
/// preformatted text, with links where the items are.
pub fn menu_to_html(text: &str) -> String {
    let mut html = String::from("<pre>");
    for line in text.lines() {
        if line == "." {
            break;
        }
        let mut chars = line.chars();
        let Some(item_type) = chars.next() else {
            html.push('\n');
            continue;
        };
        let fields: Vec<&str> = chars.as_str().split('\t').collect();
        let display = fields[0];
        match item_type {
            'i' | '3' => html.push_str(&format!("      {}\n", escape_html(display))),
            '8' | 'T' | '2' | '+' => {
                html.push_str(&format!("{} {} (not supported)\n", type_label(item_type), escape_html(display)));
            }
            _ if fields.len() >= 3 => {
                let selector = fields[1];
                let host = fields[2];
                let port = fields.get(3).and_then(|p| p.trim().parse().ok()).unwrap_or(DEFAULT_PORT);
                // Links to the web are written "URL:<address>"
                let href = match selector.strip_prefix("URL:") {
                    Some(web) if item_type == 'h' => web.to_string(),
                    _ => item_url(item_type, selector, host, port),
                };
                html.push_str(&format!(
                    "{} <a href=\"{}\">{}</a>\n",
                    type_label(item_type),
                    escape_html(&href),
                    escape_html(display)
                ));
            }
            // Not a menu line after all
            _ => html.push_str(&format!("{}\n", escape_html(line))),
        }
    }
    html.push_str("</pre>");
    html
}
