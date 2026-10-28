//! Fetching a page in the background: from the address typed (or linked) to a page to show,
//! across redirects, I2P jump services, Anubis bot checks, and the small web.

use super::*;
use crate::cache::CachedPage;
use crate::network::Page;

/// How a page load may use the cache
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CacheUse {
    /// Following a link or typing an address: a fresh cached copy is shown, a stale one is
    /// checked with the site
    #[default]
    Normal,
    /// Back and Forward: any cached copy, as it was
    History,
    /// Reload: always ask the site (if the cached copy allows, only whether it changed)
    Reload,
}

/// What to fetch, and how
pub(super) struct PageRequest {
    pub(super) tab_id: usize,
    pub(super) url: String,
    /// A form submitted with POST
    pub(super) form_body: Option<String>,
    pub(super) i2p_mode: bool,
    pub(super) solve_anubis: bool,
    pub(super) cache_use: CacheUse,
}

/// How a response may be cached: until when it's fresh, and its validators
#[derive(Default)]
struct CacheInfo {
    fresh_until: Option<u64>,
    etag: Option<String>,
    last_modified: Option<String>,
}

/// A fetched page, and a note for the status line after it's shown, if any
struct Fetched {
    page: Page,
    note: Option<String>,
    /// Set for responses that may be cached
    cache: Option<CacheInfo>,
    /// The site said the cached copy is still current (304 Not Modified)
    unchanged: bool,
    /// The response was a server error (5xx): the I2P proxy answers so when a site is unreachable
    server_error: bool,
}

/// The address to load for what was typed in the address bar: web addresses get https:// (or
/// http:// for .i2p sites), anything else is searched for, and clearweb http:// is upgraded
pub(super) fn address_for(input: &str, settings: &Settings, i2p_mode: bool) -> String {
    let input = input.trim();
    let address =
        if input.starts_with("http://") || input.starts_with("https://") || crate::network::is_small_web(input) {
            input.to_string()
        } else if input.contains('.') && !input.contains(' ') {
            if input.ends_with(".i2p") { format!("http://{}", input) } else { format!("https://{}", input) }
        } else {
            settings.search_address(input)
        };
    // Plain http:// stays only for this computer's own addresses and .i2p sites
    crate::network::enforce_https(&address, i2p_mode)
}

/// Everything a background fetch shares with the browser
pub(super) struct Shared {
    pub(super) network: Arc<NetworkManager>,
    pub(super) gemini_hosts: Arc<std::sync::Mutex<crate::gemini::KnownHosts>>,
    pub(super) cache: Arc<crate::cache::Cache>,
    pub(super) snapshots: Arc<crate::snapshots::Snapshots>,
    pub(super) tx: mpsc::Sender<NetworkResponse>,
}

/// "2026-10-27 13:02 UTC"
pub(super) fn when(time: u64) -> String {
    let (date, clock) = crate::history::utc_date_time(time);
    format!("{} {} UTC", date, clock)
}

/// Fetch a page and report it to the tab: Loading, then the page (or an error), then any note.
/// The cache is used as `request.cache_use` allows, and a tracked site's changed page is kept as
/// a snapshot.
pub(super) async fn fetch(request: PageRequest, shared: Shared) {
    let id = request.tab_id;
    let tx = &shared.tx;
    let _ = tx.send(NetworkResponse::Loading(id)).await;
    let private = crate::cache::is_i2p(&request.url, request.i2p_mode);
    // Form submissions are never served from (or kept in) the cache
    let cached = if request.form_body.is_none() { shared.cache.get(&request.url, private) } else { None };
    let now = crate::cache::now();
    if let Some(entry) = &cached
        && (request.cache_use == CacheUse::History || (request.cache_use == CacheUse::Normal && entry.is_fresh(now)))
    {
        return deliver(tx, id, entry.page.clone(), None).await;
    }

    let result = if crate::network::is_small_web(&request.url) {
        fetch_small_web(&request, &shared.gemini_hosts).await.map(|page| Fetched {
            page,
            note: None,
            cache: Some(CacheInfo::default()),
            unchanged: false,
            server_error: false,
        })
    } else {
        let revalidate = cached.as_deref().filter(|e| e.can_revalidate());
        fetch_web(&request, shared.network.get_client(request.i2p_mode), tx, revalidate).await
    };
    match (result, cached) {
        // Not modified: the cached copy is current again
        (Ok(fetched), Some(entry)) if fetched.unchanged => {
            let mut renewed = (*entry).clone();
            renewed.stored = now;
            renewed.fresh_until = fetched.cache.and_then(|c| c.fresh_until);
            shared.cache.put(&request.url, renewed, private);
            deliver(tx, id, entry.page.clone(), None).await;
        }
        // The site (or, for I2P, the router) says it can't serve the page, but there's a copy
        (Ok(fetched), Some(entry)) if fetched.server_error => {
            let note = format!("This is the copy saved {}: the site answered with an error", when(entry.stored));
            deliver(tx, id, entry.page.clone(), Some(note)).await;
        }
        (Ok(Fetched { page, note, cache, .. }), _) => {
            let mut note = note;
            if let Some(info) = cache.filter(|_| request.form_body.is_none()) {
                let entry = CachedPage {
                    page: page.clone(),
                    stored: now,
                    fresh_until: info.fresh_until,
                    etag: info.etag,
                    last_modified: info.last_modified,
                };
                shared.cache.put(&request.url, entry, private);
                if shared.snapshots.record(&request.url, &page, now).is_some() && note.is_none() {
                    note = Some(String::from("Saved a snapshot of this page (Z lists them)"));
                }
            }
            deliver(tx, id, page, note).await;
        }
        // The site can't be reached, but there's a copy: show it, saying from when
        (Err(e), Some(entry)) => {
            // What it is first: the status line may cut the end off
            let note = format!("This is the copy saved {}: couldn't reach the site ({})", when(entry.stored), e);
            deliver(tx, id, entry.page.clone(), Some(note)).await;
        }
        (Err(e), None) => {
            let _ = tx.send(NetworkResponse::Error(id, e)).await;
        }
    }
}

/// Show a page in the tab, then a note in its status line
async fn deliver(tx: &mpsc::Sender<NetworkResponse>, id: usize, page: Page, note: Option<String>) {
    let _ = tx.send(NetworkResponse::Success(id, page.title, page.body, page.url, page.kind)).await;
    if let Some(note) = note {
        let _ = tx.send(NetworkResponse::Info(id, note)).await;
    }
}

async fn fetch_small_web(
    request: &PageRequest,
    gemini_hosts: &std::sync::Mutex<crate::gemini::KnownHosts>,
) -> Result<Page, String> {
    if request.i2p_mode {
        // These connections don't go through the I2P proxy, so they'd reveal the visit
        return Err("Gemini and Gopher aren't available in I2P mode (press p to leave it)".to_string());
    }
    if request.url.starts_with("gemini://") {
        crate::gemini::load(&request.url, gemini_hosts, MAX_PAGE_SIZE_BYTES).await
    } else {
        crate::gopher::load(&request.url, MAX_PAGE_SIZE_BYTES).await
    }
}

/// A response's cacheability: None unless it's a successful page the site allows caching
fn cache_info(response: &reqwest::Response, now: u64) -> Option<CacheInfo> {
    if response.status() != reqwest::StatusCode::OK && response.status() != reqwest::StatusCode::NOT_MODIFIED {
        return None;
    }
    let headers = response.headers();
    let text =
        |name| headers.get(name).and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok()).map(str::to_string);
    Some(CacheInfo {
        fresh_until: crate::cache::freshness(headers, now)?,
        etag: text(reqwest::header::ETAG),
        last_modified: text(reqwest::header::LAST_MODIFIED),
    })
}

async fn fetch_web(
    request: &PageRequest,
    client: &reqwest::Client,
    tx: &mpsc::Sender<NetworkResponse>,
    revalidate: Option<&CachedPage>,
) -> Result<Fetched, String> {
    let id = request.tab_id;
    // Ask whether the cached copy changed, instead of for the page again
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(entry) = revalidate {
        let mut add = |name, value: &Option<String>| {
            if let Some(value) = value.as_deref().and_then(|v| reqwest::header::HeaderValue::from_str(v).ok()) {
                headers.insert(name, value);
            }
        };
        add(reqwest::header::IF_NONE_MATCH, &entry.etag);
        add(reqwest::header::IF_MODIFIED_SINCE, &entry.last_modified);
    }
    let mut response =
        crate::network::send_with_headers(client, &request.url, request.i2p_mode, request.form_body.clone(), headers)
            .await?;
    if revalidate.is_some() && response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let cache = cache_info(&response, crate::cache::now());
        let page = Page { title: String::new(), body: String::new(), url: String::new(), kind: PageKind::Html };
        return Ok(Fetched { page, note: None, cache, unchanged: true, server_error: false });
    }

    // An .i2p address the router doesn't know yet: ask the jump services for it
    let domain = Url::parse(&request.url).ok().and_then(|u| u.domain().map(str::to_string)).unwrap_or_default();
    if should_attempt_jump(request.i2p_mode, &domain, response.status())
        && let Ok(jumped) = attempt_jump(client, &domain, tx.clone(), id).await
    {
        response = jumped;
    }

    // An Anubis bot check is passed at most once per page, so a site that keeps asking can't make
    // rynx work forever
    let mut anubis_tried = false;
    loop {
        // Where redirects actually led; relative links resolve against this
        let url = response.url().to_string();
        let status = response.status();
        let content_type =
            response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_string);
        let class = classify_content_type(content_type.as_deref());

        // Images, PDFs and the like aren't read into memory as a page; offer a download
        if let ContentClass::Binary(mime) = &class {
            let (title, body) = crate::network::binary_notice_page(&url, mime, response.content_length());
            let page = Page { title, body, url, kind: PageKind::Html };
            let server_error = status.is_server_error();
            return Ok(Fetched { page, note: None, cache: None, unchanged: false, server_error });
        }
        // Fast rejection when the server declares the size; read_text_limited enforces the
        // limit when it doesn't
        if response.content_length().is_some_and(|len| len > MAX_PAGE_SIZE_BYTES) {
            return Err("Page too large".to_string());
        }
        let cache = cache_info(&response, crate::cache::now());
        let body = crate::network::read_text_limited(response, MAX_PAGE_SIZE_BYTES).await?;

        // An Anubis "making sure you're not a bot" page: do its proof of work natively and load
        // what it was guarding. If that fails, the check's page is shown with the reason.
        let mut problem = None;
        if anubis_tried && (status.is_client_error() || status.is_server_error()) {
            problem = Some(String::from("the site didn't accept rynx's answer"));
        } else if class == ContentClass::Html
            && let Some(challenge) = crate::anubis::detect(&body)
        {
            if anubis_tried {
                problem = Some(String::from("the site didn't accept rynx's answer"));
            } else if !request.solve_anubis {
                problem = Some(String::from("passing them is turned off in settings.conf"));
            } else {
                anubis_tried = true;
                let host = Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
                let note = format!("Passing {}'s bot check (Anubis), without JavaScript...", host);
                let _ = tx.send(NetworkResponse::Info(id, note)).await;
                match crate::anubis::pass(client, &url, challenge, request.i2p_mode).await {
                    Ok(next) => {
                        response = next;
                        continue;
                    }
                    Err(e) => problem = Some(e),
                }
            }
        }
        // A bot check's page or an MCP server's notice isn't worth keeping
        let endpoint = crate::mcp::is_endpoint_answer(content_type.as_deref(), &body);
        let cache = cache.filter(|_| problem.is_none() && !endpoint);
        let note = problem.map(|p| format!("This is a bot check (Anubis), and {}", p));
        let page = page_from_body(url, content_type.as_deref(), &class, body);
        return Ok(Fetched { page, note, cache, unchanged: false, server_error: status.is_server_error() });
    }
}

/// What a response's body is shown as: an MCP server's notice, a feed's posts, HTML, or text
fn page_from_body(url: String, content_type: Option<&str>, class: &ContentClass, body: String) -> Page {
    if crate::mcp::is_endpoint_answer(content_type, &body) {
        let body = crate::mcp::endpoint_page_html(&url);
        return Page { title: String::from("MCP server"), body, url, kind: PageKind::Html };
    }
    if crate::feeds::is_feed(content_type, &body) {
        let (title, body) = crate::feeds::feed_to_html(&body);
        return Page { title, body, url, kind: PageKind::Html };
    }
    // The page isn't parsed here; an HTML page's <title> replaces this name once it's laid out
    let title = crate::network::title_from_url(&url);
    let kind = if *class == ContentClass::Html { PageKind::Html } else { PageKind::PlainText };
    Page { title, body, url, kind }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_text_becomes_an_address() {
        let settings = Settings::default();
        assert_eq!(address_for("example.com", &settings, false), "https://example.com");
        assert_eq!(address_for("  http://example.com/a ", &settings, false), "https://example.com/a");
        assert_eq!(address_for("http://localhost:8000/", &settings, false), "http://localhost:8000/");
        assert_eq!(address_for("site.i2p", &settings, false), "http://site.i2p");
        assert_eq!(address_for("gemini://capsule.example/", &settings, false), "gemini://capsule.example/");
        assert_eq!(address_for("rust tui", &settings, false), settings.search_address("rust tui"));
    }
}
