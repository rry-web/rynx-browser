//! Gemini and Gopher, against small local servers.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use rynx::app::App;
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::gemini::{self, KnownHosts};
use rynx::gopher;
use rynx::network::{NetworkResponse, PageKind};
use rynx::renderer::{RenderJob, render_page};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const LIMIT: u64 = 1024 * 1024;

fn rendered_text(html: &str) -> String {
    let job = RenderJob { source: Arc::from(html), kind: PageKind::Html, source_view: false, width: 80 };
    let page = render_page(&job);
    page.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

// --- Gemtext ---

#[test]
fn gemtext_is_converted() {
    let text = "# Capsule\n\nHello <b>world</b>\n=> /about About me\n=> gemini://other.example/\n* one\n* two\n> quoted\n```art\n  /\\_/\\\n```\n## Sub";
    let (title, html) = gemini::gemtext_to_html(text);
    assert_eq!(title.as_deref(), Some("Capsule"));
    assert!(html.contains("Hello &lt;b&gt;world"), "text must be escaped: {html}");
    assert!(html.contains("<a href=\"/about\">About me</a>"));
    assert!(html.contains("<a href=\"gemini://other.example/\">gemini://other.example/</a>"));
    assert!(html.contains("<ul><li>one</li><li>two</li></ul>"));
    assert!(html.contains("<pre>  /\\_/\\\n</pre>"));

    let shown = rendered_text(&html);
    assert!(shown.contains("⇒ About me\n⇒ gemini://other.example/"), "links stay one per line: {shown}");
    assert!(shown.contains("  /\\_/\\"), "preformatted text keeps its spacing: {shown}");
}

#[test]
fn unclosed_gemtext_blocks_are_closed() {
    let (_, html) = gemini::gemtext_to_html("* item\n```\ncode");
    assert!(html.ends_with("<ul><li>item</li></ul><pre>code\n</pre>"), "{html}");
}

// --- Gopher addresses and menus ---

#[test]
fn gopher_addresses_are_parsed() {
    let t = gopher::parse("gopher://example.org/").unwrap();
    assert_eq!((t.host.as_str(), t.port, t.item_type, t.selector.as_str()), ("example.org", 70, '1', ""));
    let t = gopher::parse("gopher://example.org:7070/0/docs/a%20file.txt").unwrap();
    assert_eq!((t.port, t.item_type, t.selector.as_str()), (7070, '0', "/docs/a file.txt"));
    let t = gopher::parse("gopher://example.org/7/search%09rust%20tui").unwrap();
    assert_eq!((t.selector.as_str(), t.query.as_deref()), ("/search", Some("rust tui")));
}

#[test]
fn gopher_selectors_cannot_smuggle_commands() {
    for url in [
        "gopher://127.0.0.1:6379/1FLUSHALL%0D%0ASET%20x%201",
        "gopher://example.org/0a%0Ab",
        "gopher://example.org/0a%00b",
        "gopher://example.org/7s%09q%0D%0Ax",
    ] {
        assert!(gopher::parse(url).is_err(), "{url} should be refused");
    }
}

#[test]
fn gopher_menus_are_converted() {
    let menu = "iWelcome to <my> hole\t\terror.host\t1\r\n\
                1Phlog\t/phlog\texample.org\t70\r\n\
                0About\t/about.txt\texample.org\t7070\r\n\
                7Search\t/search\texample.org\t70\r\n\
                hWeb site\tURL:https://example.com/\texample.org\t70\r\n\
                9Archive\t/a b.zip\texample.org\t70\r\n\
                .\r\n\
                1After the end\t/x\texample.org\t70\r\n";
    let html = gopher::menu_to_html(menu);
    assert!(html.contains("Welcome to &lt;my&gt; hole"));
    assert!(html.contains("[DIR] <a href=\"gopher://example.org/1/phlog\">Phlog</a>"));
    assert!(html.contains("<a href=\"gopher://example.org:7070/0/about.txt\">About</a>"));
    assert!(html.contains("[ ? ] <a href=\"gopher://example.org/7/search\">"));
    assert!(html.contains("[WWW] <a href=\"https://example.com/\">"));
    assert!(html.contains("gopher://example.org/9/a%20b.zip"));
    assert!(!html.contains("After the end"), "the menu ends at the . line");
    assert!(rendered_text(&html).contains("[DIR] Phlog"));
}

// --- A local Gopher server ---

/// Serve each connection with `respond(request line)`, forever
async fn gopher_server(respond: fn(&str) -> String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut byte = [0u8];
                while socket.read(&mut byte).await.unwrap_or(0) == 1 && byte[0] != b'\n' {
                    request.push(byte[0]);
                }
                let request = String::from_utf8_lossy(&request).trim_end_matches('\r').to_string();
                let _ = socket.write_all(respond(&request).as_bytes()).await;
            });
        }
    });
    port
}

fn gopher_hole(request: &str) -> String {
    match request {
        "" => "iHello\t\t\t\r\n0Read me\t/readme\t127.0.0.1\t70\r\n.\r\n".to_string(),
        "/readme" => "Line one\r\n..dotted\r\n.\r\n".to_string(),
        "/search\trust tui" => "iYou searched for rust tui\t\t\t\r\n.\r\n".to_string(),
        other => format!("3Unknown selector {other}\t\t\t\r\n.\r\n"),
    }
}

#[tokio::test]
async fn gopher_pages_load() {
    let port = gopher_server(gopher_hole).await;
    let menu = gopher::load(&format!("gopher://127.0.0.1:{port}/"), LIMIT).await.unwrap();
    assert_eq!(menu.kind, PageKind::Html);
    assert!(menu.body.contains("Hello"));

    let text = gopher::load(&format!("gopher://127.0.0.1:{port}/0/readme"), LIMIT).await.unwrap();
    assert_eq!(text.kind, PageKind::PlainText);
    assert_eq!(text.body, "Line one\n.dotted\n");

    // A search item without words asks for them first, without connecting
    let ask = gopher::load(&format!("gopher://127.0.0.1:{port}/7/search"), LIMIT).await.unwrap();
    assert!(ask.body.contains("<form"));
    let found = gopher::load(&format!("gopher://127.0.0.1:{port}/7/search%09rust%20tui"), LIMIT).await.unwrap();
    assert!(found.body.contains("You searched for rust tui"));
}

// --- A local Gemini server ---

struct Certificate {
    der: rustls::pki_types::CertificateDer<'static>,
    key: Vec<u8>,
}

fn self_signed() -> Certificate {
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    Certificate { der: made.cert.der().clone(), key: made.signing_key.serialize_der() }
}

/// Serve Gemini with `certificate`, answering each request with `respond(request URL)`
async fn gemini_server(certificate: Certificate, respond: fn(&str) -> String) -> u16 {
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(certificate.key.into());
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.der], key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(socket).await else { return };
                let mut request = Vec::new();
                let mut byte = [0u8];
                while tls.read(&mut byte).await.unwrap_or(0) == 1 && byte[0] != b'\n' {
                    request.push(byte[0]);
                }
                let request = String::from_utf8_lossy(&request).trim_end_matches('\r').to_string();
                let _ = tls.write_all(respond(&request).as_bytes()).await;
                // Like many servers, close without a TLS close_notify
                let (socket, _) = tls.into_inner();
                drop(socket);
            });
        }
    });
    port
}

fn capsule(request: &str) -> String {
    let path = request.splitn(4, '/').nth(3).unwrap_or("");
    match path {
        "" => "20 text/gemini\r\n# Home\n=> /next Next page\n".to_string(),
        "moved" => "31 /\r\n".to_string(),
        "away" => "30 https://example.com/\r\n".to_string(),
        "ask" => "10 Your name?\r\n".to_string(),
        p if p.starts_with("ask?") => format!("20 text/gemini\r\nHello {}\n", &p[4..]),
        "plain" => "20 text/plain; charset=utf-8\r\ncaf\u{e9}\n".to_string(),
        "image" => "20 image/png\r\n\u{89}PNG".to_string(),
        "cert" => "60 Certificate needed\r\n".to_string(),
        _ => "51 No such page\r\n".to_string(),
    }
}

#[tokio::test]
async fn gemini_pages_load() {
    let port = gemini_server(self_signed(), capsule).await;
    let hosts = Mutex::new(KnownHosts::default());
    let base = format!("gemini://127.0.0.1:{port}/");

    let home = gemini::load(&base, &hosts, LIMIT).await.unwrap();
    assert_eq!(home.title, "Home");
    assert!(home.body.contains("<a href=\"/next\">Next page</a>"));

    let moved = gemini::load(&format!("{base}moved"), &hosts, LIMIT).await.unwrap();
    assert_eq!(moved.url, base, "redirects are followed");
    let away = gemini::load(&format!("{base}away"), &hosts, LIMIT).await.unwrap_err();
    assert!(away.contains("Blocked redirect"), "{away}");

    let ask = gemini::load(&format!("{base}ask"), &hosts, LIMIT).await.unwrap();
    assert!(ask.body.contains("Your name?") && ask.body.contains("<form"));
    let answered = gemini::load(&format!("{base}ask?Ray%20R"), &hosts, LIMIT).await.unwrap();
    assert!(answered.body.contains("Hello Ray%20R"));

    let plain = gemini::load(&format!("{base}plain"), &hosts, LIMIT).await.unwrap();
    assert_eq!((plain.kind, plain.body.trim()), (PageKind::PlainText, "caf\u{e9}"));
    let image = gemini::load(&format!("{base}image"), &hosts, LIMIT).await.unwrap();
    assert!(image.body.contains("can't be shown"));

    let missing = gemini::load(&format!("{base}nothing"), &hosts, LIMIT).await.unwrap_err();
    assert_eq!(missing, "Not found (51): No such page");
    let cert = gemini::load(&format!("{base}cert"), &hosts, LIMIT).await.unwrap_err();
    assert!(cert.contains("client certificate"), "{cert}");
}

#[tokio::test]
async fn gemini_certificates_are_trusted_on_first_use() {
    let first = self_signed();
    let port = gemini_server(first, capsule).await;
    let impostor_port = gemini_server(self_signed(), capsule).await;
    let hosts = Mutex::new(KnownHosts::default());

    // First visit: remembered
    gemini::load(&format!("gemini://127.0.0.1:{port}/"), &hosts, LIMIT).await.unwrap();
    assert!(hosts.lock().unwrap().get(&format!("127.0.0.1:{port}")).is_some());
    // Same certificate again: fine
    gemini::load(&format!("gemini://127.0.0.1:{port}/"), &hosts, LIMIT).await.unwrap();

    // Pretend the impostor's address was visited before with the first certificate
    let known = hosts.lock().unwrap().get(&format!("127.0.0.1:{port}")).unwrap().to_string();
    hosts.lock().unwrap().set(&format!("127.0.0.1:{impostor_port}"), &known);
    let err = gemini::load(&format!("gemini://127.0.0.1:{impostor_port}/"), &hosts, LIMIT).await.unwrap_err();
    assert!(err.contains("certificate has changed"), "{err}");
    assert_eq!(hosts.lock().unwrap().get(&format!("127.0.0.1:{impostor_port}")), Some(known.as_str()), "pin kept");
}

#[test]
fn known_hosts_are_saved() {
    let dir = std::env::temp_dir().join(format!("rynx-gemini-hosts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join(gemini::KNOWN_HOSTS_FILE);
    KnownHosts::load(path.clone()).set("capsule.example:1965", "abc123");
    assert_eq!(KnownHosts::load(path).get("capsule.example:1965"), Some("abc123"));
    let _ = std::fs::remove_dir_all(&dir);
}

// --- In the browser ---

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() }
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

/// Show `html` as if it had been loaded from `url`
fn show(app: &mut App, url: &str, html: &str) {
    let id = app.current_tab().id;
    app.current_tab().url_input = url.into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), url.into(), PageKind::Html);
    handle_network_event(app, resp, 80).unwrap();
}

/// Handle network events until the page has loaded or failed
async fn settle(app: &mut App) {
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(20), app.rx.recv()).await.unwrap().unwrap();
        let done = matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(app, resp, 80).unwrap();
        if done {
            return;
        }
    }
}

#[tokio::test]
async fn addresses_are_typed_and_searches_submitted() {
    let port = gopher_server(gopher_hole).await;
    let mut app = test_app();
    app.current_tab().url_input = format!("gopher://127.0.0.1:{port}/7/search");
    app.submit_request();
    settle(&mut app).await;
    assert!(app.current_tab().url_input.starts_with("gopher://"), "not treated as a search or https");

    // The search field is selected first: type into it and press Enter
    press(&mut app, KeyCode::Enter);
    for c in "rust tui".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.current_tab().url_input, format!("gopher://127.0.0.1:{port}/7/search%09rust%20tui"));
    settle(&mut app).await;
    let text: Vec<String> = app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect();
    assert!(text.join("\n").contains("You searched for rust tui"), "{text:?}");
}

fn press(app: &mut App, code: KeyCode) {
    handle_key_event(app, key(code), 80, 24).unwrap();
}

#[tokio::test]
async fn public_pages_cannot_link_to_local_gopher_services() {
    let mut app = test_app();
    show(&mut app, "https://evil.example/", "<a href=\"gopher://127.0.0.1:6379/1INFO\">free stuff</a>");
    app.follow_link("gopher://127.0.0.1:6379/1INFO".into());
    assert!(app.current_tab().status_message.contains("Blocked"));
    assert_eq!(app.current_tab().url_input, "https://evil.example/");
    // Public Gopher and Gemini links are fine
    assert!(!rynx::network::unsupported_link_scheme("gemini://capsule.example/").is_some());
}

#[tokio::test]
async fn small_web_is_refused_in_i2p_mode() {
    let mut app = test_app();
    app.i2p_mode = true;
    app.current_tab().url_input = "gemini://capsule.example/".into();
    app.submit_request();
    settle(&mut app).await;
    assert!(app.current_tab().status_message.contains("I2P") || app.current_tab().page_title == "Error");
}

#[test]
fn local_addresses_are_recognised_in_every_scheme() {
    use rynx::network::is_local_network_url;
    for url in [
        "gopher://127.0.0.1:6379/",
        "gopher://2130706433/",
        "gopher://0x7f.0.0.1/",
        "gemini://192.168.1.1/",
        "gemini://[::1]/",
        "gopher://localhost/",
    ] {
        assert!(is_local_network_url(url), "{url} is local");
    }
    assert!(!is_local_network_url("gemini://geminiprotocol.net/"));
}

#[tokio::test]
#[ignore = "needs the internet"]
async fn live_gemini_capsule() {
    let hosts = Mutex::new(KnownHosts::default());
    let page = gemini::load("gemini://geminiprotocol.net/", &hosts, LIMIT).await.unwrap();
    assert!(page.body.contains("<a href="), "{}", page.body);
    println!("{}: {} bytes\n{}", page.title, page.body.len(), rendered_text(&page.body));
}

#[tokio::test]
#[ignore = "needs the internet"]
async fn live_gopher_hole() {
    let page = gopher::load("gopher://gopher.floodgap.com/", LIMIT).await.unwrap();
    assert!(page.body.contains("[DIR]"), "{}", page.body);
    println!("{}: {} bytes\n{}", page.title, page.body.len(), rendered_text(&page.body));
}
