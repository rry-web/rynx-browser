//! Fetching a page in the background: from the address typed (or linked) to a page to show,
//! across redirects, I2P jump services, Anubis bot checks, and the small web.

use super::*;
use crate::network::Page;

/// What to fetch, and how
pub(super) struct PageRequest {
    pub(super) tab_id: usize,
    pub(super) url: String,
    /// A form submitted with POST
    pub(super) form_body: Option<String>,
    pub(super) i2p_mode: bool,
    pub(super) solve_anubis: bool,
}

/// A fetched page, and a note for the status line after it's shown, if any
struct Fetched {
    page: Page,
    note: Option<String>,
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

/// Fetch a page and report it to the tab: Loading, then the page (or an error), then any note
pub(super) async fn fetch(
    request: PageRequest,
    network: Arc<NetworkManager>,
    gemini_hosts: Arc<std::sync::Mutex<crate::gemini::KnownHosts>>,
    tx: mpsc::Sender<NetworkResponse>,
) {
    let id = request.tab_id;
    let _ = tx.send(NetworkResponse::Loading(id)).await;
    let result = if crate::network::is_small_web(&request.url) {
        fetch_small_web(&request, &gemini_hosts).await.map(|page| Fetched { page, note: None })
    } else {
        fetch_web(&request, network.get_client(request.i2p_mode), &tx).await
    };
    match result {
        Ok(Fetched { page, note }) => {
            let _ = tx.send(NetworkResponse::Success(id, page.title, page.body, page.url, page.kind)).await;
            if let Some(note) = note {
                let _ = tx.send(NetworkResponse::Info(id, note)).await;
            }
        }
        Err(e) => {
            let _ = tx.send(NetworkResponse::Error(id, e)).await;
        }
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

async fn fetch_web(
    request: &PageRequest,
    client: &reqwest::Client,
    tx: &mpsc::Sender<NetworkResponse>,
) -> Result<Fetched, String> {
    let id = request.tab_id;
    let mut response =
        crate::network::send_following_redirects(client, &request.url, request.i2p_mode, request.form_body.clone())
            .await?;

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
            return Ok(Fetched { page: Page { title, body, url, kind: PageKind::Html }, note: None });
        }
        // Fast rejection when the server declares the size; read_text_limited enforces the
        // limit when it doesn't
        if response.content_length().is_some_and(|len| len > MAX_PAGE_SIZE_BYTES) {
            return Err("Page too large".to_string());
        }
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
        let note = problem.map(|p| format!("This is a bot check (Anubis), and {}", p));
        return Ok(Fetched { page: page_from_body(url, content_type.as_deref(), &class, body), note });
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
