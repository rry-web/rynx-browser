//! Gemini, Gopher and feeds, against local servers (SMALL in TESTING.md)

use super::harness::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::Arc;

fn started(name: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings("");
    let rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    (home, rynx)
}

/// A Gemini server on localhost, with a self-signed certificate, answering each request (its
/// URL) with `respond`; kept running while it's held
struct Gemini {
    port: u16,
    _runtime: tokio::runtime::Runtime,
}

impl Gemini {
    fn start(respond: fn(&str) -> String) -> Gemini {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let made = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(made.signing_key.serialize_der().into());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![made.cert.der().clone()], key)
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        runtime.spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else { return };
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
                    let _ = tls.shutdown().await;
                });
            }
        });
        Gemini { port, _runtime: runtime }
    }

    fn url(&self, path: &str) -> String {
        format!("gemini://localhost:{}{}", self.port, path)
    }
}

fn capsule(request: &str) -> String {
    let path = request.splitn(4, '/').nth(3).unwrap_or("");
    match path {
        "" => "20 text/gemini\r\n# Capsule home\nWelcome to the capsule.\n=> /next The next page\n=> /search Search\n"
            .to_string(),
        "next" => "20 text/gemini\r\n# Next page\nYou followed the link.\n".to_string(),
        "search" => "10 Search for what?\r\n".to_string(),
        p if p.starts_with("search?") => format!("20 text/gemini\r\n# Results\nYou searched for {}\n", &p[7..]),
        _ => "51 Not found\r\n".to_string(),
    }
}

/// SMALL-01, SMALL-07: a Gemini page, its links one per line with ⇒, followed and returned
/// from; bookmarked and in history by its heading
#[test]
fn small_01_07_gemini_pages() {
    let gemini = Gemini::start(capsule);
    let (_home, mut rynx) = started("gemini");
    rynx.open(&gemini.url("/"));
    rynx.wait_for("Capsule home");
    rynx.wait_for("⇒ The next page");
    rynx.wait_for("⇒ Search");
    rynx.keys("\r");
    rynx.wait_for("You followed the link.");
    rynx.keys("\u{7f}");
    rynx.wait_for("Welcome to the capsule.");
    rynx.keys("b");
    rynx.keys("B");
    rynx.wait_for("Capsule home");
    rynx.click_text("Capsule home");
    rynx.wait_for("Welcome to the capsule.");
    rynx.keys("s");
    rynx.wait_for("Next page");
    rynx.quit();
}

/// SMALL-02: a Gemini input prompt asks with a field, and sends the words in the address
#[test]
fn small_02_gemini_input() {
    let gemini = Gemini::start(capsule);
    let (_home, mut rynx) = started("gemini-input");
    rynx.open(&gemini.url("/search"));
    rynx.wait_for("Search for what?");
    rynx.keys("\r");
    rynx.type_text("small web");
    rynx.keys("\r");
    rynx.wait_for("You searched for small%20web");
    rynx.wait_for("/search?small%20web");
    rynx.quit();
}

/// SMALL-03: a Gemini site's certificate is remembered, and a changed one refused
#[test]
fn small_03_gemini_certificates() {
    let gemini = Gemini::start(capsule);
    let (home, mut rynx) = started("gemini-tofu");
    rynx.open(&gemini.url("/"));
    rynx.wait_for("Capsule home");
    rynx.quit();
    let known = home.read_config("gemini_known_hosts.txt").expect("no known hosts file");
    let line = known
        .lines()
        .find(|l| l.contains(&format!("localhost:{}", gemini.port)))
        .expect("the host wasn't remembered")
        .to_string();
    // host:port, a tab, the fingerprint: the fingerprint zeroed
    let (host, fingerprint) = line.split_once('\t').expect("host and fingerprint");
    let changed = format!("{}\t{}", host, "0".repeat(fingerprint.len()));
    std::fs::write(home.config().join("gemini_known_hosts.txt"), known.replace(&line, &changed)).unwrap();
    let mut again = Rynx::start(&home);
    again.wait_for("RYNX BROWSER");
    again.open(&gemini.url("/"));
    again.wait_for("certificate has changed");
    assert!(!again.text().contains("Welcome to the capsule."));
    again.quit();
}

/// A Gopher server answering each selector with `respond`
fn gopher(respond: fn(&str, u16) -> String) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut line = String::new();
            let _ = BufReader::new(&stream).read_line(&mut line);
            let _ = stream.write_all(respond(line.trim_end_matches(['\r', '\n']), port).as_bytes());
        }
    });
    port
}

fn hole(selector: &str, port: u16) -> String {
    match selector {
        "" => format!(
            "i  ____  _ __       \t\t\t\r\ni |  _ \\| '_ \\      \t\t\t\r\n1Phlog\t/phlog\t127.0.0.1\t{port}\r\n\
             0About this hole\t/about.txt\t127.0.0.1\t{port}\r\n7Search the hole\t/search\t127.0.0.1\t{port}\r\n.\r\n"
        ),
        "/about.txt" => "This hole is a test.\r\n  Indented line.\r\n.\r\n".to_string(),
        "/search\tgophers" => "iFound 1 result for gophers\t\t\t\r\n.\r\n".to_string(),
        _ => "3Not found\t\t\t\r\n.\r\n".to_string(),
    }
}

/// SMALL-04: a Gopher menu with its art lined up and items marked; text and search items
#[test]
fn small_04_gopher() {
    let port = gopher(hole);
    let (_home, mut rynx) = started("gopher");
    rynx.open(&format!("gopher://127.0.0.1:{port}/"));
    rynx.wait_for("[DIR] Phlog");
    rynx.wait_for("[TXT] About this hole");
    rynx.wait_for("[ ? ] Search the hole");
    let rows = rynx.rows();
    let art: Vec<usize> = rows.iter().filter_map(|r| r.find("|  _ \\").or_else(|| r.find("____"))).collect();
    assert_eq!(art.len(), 2, "{}", rynx.screen());
    rynx.click_text("About this hole");
    rynx.wait_for("This hole is a test.");
    assert!(rynx.rows().iter().any(|r| r.contains("│  Indented line.")));
    rynx.keys("\u{7f}");
    rynx.wait_for("Search the hole");
    rynx.click_text("Search the hole");
    rynx.wait_for("Search");
    rynx.keys("\r");
    rynx.type_text("gophers");
    rynx.keys("\r");
    rynx.wait_for("Found 1 result for gophers");
    rynx.quit();
}

/// SMALL-05: Gemini and Gopher aren't fetched in I2P mode
#[test]
fn small_05_not_in_i2p_mode() {
    let gemini = Gemini::start(capsule);
    let (_home, mut rynx) = started("small-i2p");
    rynx.keys("p");
    rynx.wait_for("I2P mode on");
    rynx.open(&gemini.url("/"));
    rynx.wait_for("Gemini and Gopher aren't available in I2P mode");
    rynx.quit();
}

const FEED: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel><title>The Test Blog</title>
<item><title>First post</title><pubDate>Mon, 26 Oct 2026 10:00:00 GMT</pubDate><description>The first post's text.</description><link>http://example.com/1</link></item>
<item><title>Second post</title><pubDate>Tue, 27 Oct 2026 10:00:00 GMT</pubDate><description>More text.</description><link>http://example.com/2</link></item>
</channel></rss>"#;

/// SMALL-06: a feed is shown as its posts; a page offering one says so first
#[test]
fn small_06_feeds() {
    let site = Site::new(&[]);
    site.respond("/feed.xml", wiremock::ResponseTemplate::new(200).set_body_raw(FEED, "application/rss+xml"));
    site.page("/", "<head><title>Blog</title><link rel=\"alternate\" type=\"application/rss+xml\" href=\"/feed.xml\" title=\"Posts\"></head><h1>The blog</h1>");
    let (_home, mut rynx) = started("feeds");
    rynx.open(&site.url("/feed.xml"));
    rynx.wait_for("The Test Blog");
    rynx.wait_for("Feed with 2 items");
    rynx.wait_for("First post");
    rynx.wait_for("26 Oct 2026");
    rynx.wait_for("The first post's text.");
    rynx.open(&site.url("/"));
    rynx.wait_for("The blog");
    // The page's first line, below the tab and address bars
    let first = rynx.rows()[7].clone();
    assert!(first.contains("Feeds:"), "the first line is {first:?}");
    rynx.quit();
}
