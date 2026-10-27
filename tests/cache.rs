//! The page cache and snapshots.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use reqwest::header::{HeaderMap, HeaderValue};
use rynx::app::App;
use rynx::cache::{Cache, CachedPage, freshness, key};
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::network::{NetworkResponse, Page, PageKind};
use rynx::snapshots::Snapshots;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rynx-cache-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(*name, HeaderValue::from_str(value).unwrap());
    }
    map
}

#[test]
fn freshness_follows_the_sites_rules() {
    let now = 1_000_000;
    assert_eq!(freshness(&headers(&[("cache-control", "no-store")]), now), None, "never kept");
    assert_eq!(freshness(&headers(&[("cache-control", "private, max-age=60")]), now), Some(Some(now + 60)));
    assert_eq!(freshness(&headers(&[("cache-control", "no-cache")]), now), Some(None), "kept, always checked");
    assert_eq!(freshness(&headers(&[("expires", "Thu, 01 Jan 1970 00:00:00 GMT")]), now), Some(Some(0)));
    assert_eq!(freshness(&HeaderMap::new(), now), Some(None));
    assert_eq!(key("https://a.example/p#section").as_deref(), Some("https://a.example/p"));
    assert_eq!(key("https://user:pw@a.example/"), None, "addresses with passwords aren't cached");
}

fn page(url: &str, body: &str) -> CachedPage {
    CachedPage {
        page: Page { title: "T".into(), body: body.into(), url: url.into(), kind: PageKind::Html },
        stored: 5,
        fresh_until: Some(10),
        etag: Some("\"v1\"".into()),
        last_modified: None,
    }
}

#[test]
fn the_disk_cache_is_opt_in_compressed_and_limited() {
    let dir = temp_dir("disk");
    let cache = Cache::new(Some(dir.clone()));
    cache.put("https://a.example/", page("https://a.example/", "hello"), false);
    assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().count() == 0, "nothing on disk until turned on");

    cache.configure(true, false, 1);
    cache.put("https://a.example/", page("https://a.example/", "hello"), false);
    cache.put("http://site.i2p/", page("http://site.i2p/", "secret"), true);
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "I2P pages need their own switch");
    let raw = std::fs::read(files[0].path()).unwrap();
    assert_eq!(&raw[..2], &[0x1f, 0x8b], "gzip-compressed");

    // A fresh cache (a later session) reads it back
    let later = Cache::new(Some(dir.clone()));
    later.configure(true, false, 1);
    assert_eq!(*later.get("https://a.example/#top", false).unwrap(), page("https://a.example/", "hello"));
    assert!(later.get("http://site.i2p/", true).is_none());

    // Beyond the limit, the oldest go first
    let big = "x".repeat(400 * 1024);
    for i in 0..6 {
        later.put(
            &format!("https://a.example/{i}"),
            page(&format!("https://a.example/{i}"), &format!("{big}{i}")),
            false,
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(later.disk_usage() <= 1024 * 1024, "{}", later.disk_usage());

    // A damaged file is ignored, not trusted
    for file in std::fs::read_dir(&dir).unwrap().flatten() {
        std::fs::write(file.path(), b"not gzip").unwrap();
    }
    let reread = Cache::new(Some(dir.clone()));
    reread.configure(true, false, 1);
    assert!(reread.get("https://a.example/5", false).is_none());

    reread.clear().unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- In the browser ---

/// A site whose page can change, which counts full responses and answers If-None-Match
struct Site {
    body: Arc<Mutex<String>>,
    cache_control: &'static str,
    full: Arc<Mutex<usize>>,
    not_modified: Arc<Mutex<usize>>,
}

impl Respond for Site {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = self.body.lock().unwrap().clone();
        let etag = format!("\"{}\"", body.len());
        if request.headers.get("if-none-match").is_some_and(|v| v.to_str().unwrap() == etag) {
            *self.not_modified.lock().unwrap() += 1;
            return ResponseTemplate::new(304).insert_header("etag", etag.as_str());
        }
        *self.full.lock().unwrap() += 1;
        ResponseTemplate::new(200)
            .insert_header("etag", etag.as_str())
            .insert_header("cache-control", self.cache_control)
            .set_body_raw(body, "text/html")
    }
}

struct Served {
    server: MockServer,
    body: Arc<Mutex<String>>,
    full: Arc<Mutex<usize>>,
    not_modified: Arc<Mutex<usize>>,
}

async fn site(cache_control: &'static str) -> Served {
    let server = MockServer::start().await;
    let body = Arc::new(Mutex::new(String::from("<title>Page</title><p>Version one</p><a href='/other'>other</a>")));
    let (full, not_modified) = (Arc::new(Mutex::new(0)), Arc::new(Mutex::new(0)));
    let responder = Site {
        body: Arc::clone(&body),
        cache_control,
        full: Arc::clone(&full),
        not_modified: Arc::clone(&not_modified),
    };
    Mock::given(wiremock::matchers::path("/")).respond_with(responder).mount(&server).await;
    Mock::given(wiremock::matchers::path("/other"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>Other page</p>", "text/html"))
        .mount(&server)
        .await;
    Served { server, body, full, not_modified }
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

fn press(app: &mut App, code: KeyCode) {
    let key =
        KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() };
    handle_key_event(app, key, 80, 24).unwrap();
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

/// Handle messages until the page (and any note after it) has arrived
async fn settle(app: &mut App) {
    let mut loaded = false;
    loop {
        let wait = if loaded { 200 } else { 20_000 };
        let Ok(Some(resp)) = tokio::time::timeout(std::time::Duration::from_millis(wait), app.rx.recv()).await else {
            assert!(loaded, "nothing arrived");
            return;
        };
        loaded |= matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(app, resp, 80).unwrap();
    }
}

async fn open(app: &mut App, url: &str) {
    app.follow_link(url.to_string());
    settle(app).await;
}

fn count(n: &Arc<Mutex<usize>>) -> usize {
    *n.lock().unwrap()
}

#[tokio::test]
async fn back_and_fresh_pages_come_from_the_cache() {
    let site = site("max-age=600").await;
    let mut app = test_app();
    let home = format!("{}/", site.server.uri());
    open(&mut app, &home).await;
    open(&mut app, &format!("{}/other", site.server.uri())).await;
    press(&mut app, KeyCode::Backspace);
    settle(&mut app).await;
    assert!(text(&mut app).contains("Version one"));
    assert_eq!(count(&site.full), 1, "Back shows the cached copy");

    // Fresh for ten minutes: following a link to it again doesn't fetch it either
    open(&mut app, &home).await;
    assert_eq!(count(&site.full), 1);

    // Reload always asks; unchanged, the site says so and nothing is downloaded again
    press(&mut app, KeyCode::Char('r'));
    settle(&mut app).await;
    assert_eq!((count(&site.full), count(&site.not_modified)), (1, 1));

    // Changed: reload gets the new version
    *site.body.lock().unwrap() = String::from("<p>Version two, quite a bit longer than before</p>");
    press(&mut app, KeyCode::Char('r'));
    settle(&mut app).await;
    assert!(text(&mut app).contains("Version two"));
    assert_eq!(count(&site.full), 2);
}

#[tokio::test]
async fn stale_pages_are_checked_and_no_store_is_never_kept() {
    let site = site("no-cache").await;
    let mut app = test_app();
    let home = format!("{}/", site.server.uri());
    open(&mut app, &home).await;
    open(&mut app, &home).await;
    assert_eq!((count(&site.full), count(&site.not_modified)), (1, 1), "checked, not downloaded again");

    let private = self::site("no-store").await;
    let home = format!("{}/", private.server.uri());
    open(&mut app, &home).await;
    open(&mut app, &format!("{}/other", private.server.uri())).await;
    press(&mut app, KeyCode::Backspace);
    settle(&mut app).await;
    assert_eq!(count(&private.full), 2, "no-store pages are fetched again");
}

/// A web server that answers `pages` requests, then stops listening altogether
async fn short_lived_server(pages: usize) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move {
        for _ in 0..pages {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            let body = "<p>Version one</p>";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncache-control: no-cache\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
        // The listener is dropped here: from now on connections are refused
    });
    address
}

#[tokio::test]
async fn an_unreachable_site_shows_the_saved_copy() {
    let mut app = test_app();
    let home = short_lived_server(1).await;
    open(&mut app, &home).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    open(&mut app, &home).await;
    assert!(text(&mut app).contains("Version one"));
    let status = app.current_tab().status_message.clone();
    assert!(status.contains("Couldn't reach the site") && status.contains("UTC"), "{status}");

    // The I2P proxy answers an unreachable site with a server error: the saved copy shows then too
    let site = site("no-cache").await;
    let home = format!("{}/", site.server.uri());
    open(&mut app, &home).await;
    Mock::given(wiremock::matchers::path("/"))
        .respond_with(ResponseTemplate::new(504).set_body_raw("<h1>Website unreachable</h1>", "text/html"))
        .with_priority(1)
        .mount(&site.server)
        .await;
    open(&mut app, &home).await;
    assert!(text(&mut app).contains("Version one"), "{}", text(&mut app));
    assert!(app.current_tab().status_message.contains("answered with an error"));
}

#[tokio::test]
async fn forms_are_never_cached() {
    let site = site("max-age=600").await;
    let mut app = test_app();
    let id = app.current_tab().id;
    let base = site.server.uri();
    let html = format!("<form method=post action='{base}/'><input name=q value=x><input type=submit></form>");
    app.current_tab().url_input = format!("{base}/form");
    let resp = NetworkResponse::Success(id, "t".into(), html.clone(), format!("{base}/form"), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    app.submit_form(0, None);
    settle(&mut app).await;
    let resp = NetworkResponse::Success(id, "t".into(), html, format!("{base}/form"), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    app.submit_form(0, None);
    settle(&mut app).await;
    assert_eq!(count(&site.full), 2, "each submission goes to the site");
}

// --- Snapshots ---

#[tokio::test]
async fn tracked_sites_get_a_snapshot_when_a_page_changes() {
    let dir = temp_dir("snapshots");
    let site = site("no-cache").await;
    let mut app = test_app();
    app.snapshots = Arc::new(Snapshots::load(Some(dir.join("snapshots")), Some(dir.join("tracked.txt"))));
    let home = format!("{}/", site.server.uri());

    open(&mut app, &home).await;
    assert!(app.snapshots.times(&home).is_empty(), "untracked sites get none");

    press(&mut app, KeyCode::Char('T'));
    assert!(app.current_tab().status_message.starts_with("Tracking 127.0.0.1"), "{}", app.current_tab().status_message);
    assert!(std::fs::read_to_string(dir.join("tracked.txt")).unwrap().contains("127.0.0.1"));

    press(&mut app, KeyCode::Char('r'));
    settle(&mut app).await;
    // The reload found it unchanged (304), so no snapshot; a full visit makes the first
    *site.body.lock().unwrap() =
        String::from("<title>Page</title><p>Version two of the page</p><a href='/other'>other</a>");
    press(&mut app, KeyCode::Char('r'));
    settle(&mut app).await;
    assert!(app.current_tab().status_message.contains("Saved a snapshot"), "{}", app.current_tab().status_message);
    assert_eq!(app.snapshots.times(&home).len(), 1);

    // Unchanged content adds none; changed content adds one
    std::thread::sleep(std::time::Duration::from_millis(1100));
    *site.body.lock().unwrap() =
        String::from("<title>Page</title><p>Version three, the newest one</p><a href='/other'>other</a>");
    press(&mut app, KeyCode::Char('r'));
    settle(&mut app).await;
    let times = app.snapshots.times(&home);
    assert_eq!(times.len(), 2);

    // Z lists them; the older one opens as the page was, with working links
    press(&mut app, KeyCode::Char('Z'));
    assert!(text(&mut app).contains("Snapshots of"), "{}", text(&mut app));
    let older = rynx::snapshots::snapshot_address(&home, times[1]);
    app.follow_link(older.clone());
    let shown = text(&mut app);
    assert!(shown.contains("Snapshot from") && shown.contains("Version two"), "{shown}");
    let link = app.current_tab().link_regions.iter().find(|l| l.url.ends_with("other")).unwrap().url.clone();
    assert_eq!(
        rynx::network::resolve_url(app.current_tab().link_base(), &link),
        format!("{}/other", site.server.uri())
    );

    // r on a snapshot loads the live page
    press(&mut app, KeyCode::Char('r'));
    settle(&mut app).await;
    assert!(text(&mut app).contains("Version three"));
    assert_eq!(app.current_tab().url_input, home);

    // Deleting asks twice
    app.follow_link(rynx::snapshots::SNAPSHOTS_PAGE.into());
    app.submit_form(0, None);
    assert!(app.current_tab().status_message.contains("again"));
    app.submit_form(0, None);
    assert!(app.snapshots.times(&home).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn web_pages_cannot_delete_snapshots() {
    let mut app = test_app();
    let id = app.current_tab().id;
    let html = r#"<form action="about:snapshots-delete"><input type=hidden name=site value=x.example><input type=submit></form>"#;
    app.current_tab().url_input = "https://evil.example/".into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), "https://evil.example/".into(), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    app.submit_form(0, None);
    assert!(app.current_tab().status_message.starts_with("Not sent"), "{}", app.current_tab().status_message);
}
