//! Hostile pages and links (SEC in TESTING.md)

use super::harness::*;

fn started(name: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings("");
    let rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    (home, rynx)
}

const EVIL: &str = "<title>Evil\x1b]0;TITLE CHANGED\x07</title><p>Before\x1b]52;c;cHduZWQ=\x07 after\x1b[2J</p>\
<p><a href=\"mailto:someone@example.com\">mail link</a> <a href=\"javascript:alert(1)\">js link</a> \
<a href=\"http://secret-site.i2p/\">i2p link</a></p>";

/// SEC-01: escape codes in a page can't set the title, write the clipboard or clear the screen
#[test]
fn sec_01_escape_codes_are_neutralised() {
    let site = Site::new(&[("/evil", EVIL)]);
    let (_home, mut rynx) = started("evil");
    rynx.open(&site.url("/evil"));
    rynx.wait_for("Before]52;c;cHduZWQ= after[2J");
    let (titles, clipboard) = rynx.requests();
    assert!(!titles.iter().any(|t| t.contains("TITLE CHANGED")), "the page set the title: {titles:?}");
    assert!(clipboard.is_empty(), "the page wrote the clipboard: {clipboard:?}");
    let output = rynx.output();
    assert!(!output.contains("\x1b]52"), "a clipboard escape reached the terminal");
    assert!(!output.contains("\x1b]0;TITLE"), "a title escape reached the terminal");
    rynx.quit();
}

/// SEC-02: the selected link's full destination is shown, relative links resolved
#[test]
fn sec_02_link_destinations_are_shown() {
    let site = Site::new(&[("/docs/page", "<p><a href=\"../other/place?x=1\">Somewhere</a></p>")]);
    let (_home, mut rynx) = started("destinations");
    rynx.open(&site.url("/docs/page"));
    rynx.wait_for("Somewhere");
    rynx.wait_for(&format!("Link: {}", site.url("/other/place?x=1")));
    rynx.quit();
}

/// SEC-03, SEC-04: mailto and javascript links are refused, by Enter, t and a click; an .i2p
/// link outside I2P mode says to turn it on
#[test]
fn sec_03_04_non_web_and_i2p_links() {
    let site = Site::new(&[("/evil", EVIL)]);
    let (_home, mut rynx) = started("non-web");
    rynx.open(&site.url("/evil"));
    rynx.wait_for("mail link");
    rynx.keys("\r");
    rynx.wait_for("Unsupported link type: mailto");
    rynx.keys("\tt");
    rynx.wait_for("Unsupported link type: javascript");
    assert!(!rynx.rows()[1].contains("│  Evil  │"), "a tab was opened: {}", rynx.rows()[1]);
    rynx.click_text("mail link");
    rynx.wait_for("Unsupported link type: mailto");
    assert_eq!(site.requests("/evil"), 1, "something was fetched");
    rynx.click_text("i2p link");
    rynx.wait_for("Enable I2P mode (press p) to visit .i2p sites");
    rynx.quit();
}

/// SEC-05: a compression bomb is refused, without the memory it would take
#[test]
fn sec_05_compression_bomb() {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(b"<p>").unwrap();
    for _ in 0..(20 * 1024) {
        encoder.write_all(&b"a ".repeat(512)).unwrap();
    }
    let bomb = encoder.finish().unwrap();
    assert!(bomb.len() < 100_000);
    let site = Site::new(&[]);
    site.respond(
        "/bomb",
        wiremock::ResponseTemplate::new(200)
            .insert_header("content-encoding", "gzip")
            .insert_header("content-type", "text/html")
            .set_body_bytes(bomb),
    );
    let (_home, mut rynx) = started("bomb");
    rynx.open(&site.url("/bomb"));
    rynx.wait_for("Page too large");
    assert!(rynx.memory_mb() < 150, "{} MB in use", rynx.memory_mb());
    rynx.quit();
}

/// SEC-06: a page in windows-1252 shows its accents
#[test]
fn sec_06_non_utf8_pages() {
    let site = Site::new(&[]);
    site.respond(
        "/old",
        wiremock::ResponseTemplate::new(200)
            .insert_header("content-type", "text/html; charset=windows-1252")
            .set_body_bytes(b"<p>Caf\xe9 cr\xe8me \x80 5</p>".to_vec()),
    );
    let (_home, mut rynx) = started("charset");
    rynx.open(&site.url("/old"));
    rynx.wait_for("Café crème € 5");
    rynx.quit();
}

/// SEC-10: a look-alike name is flagged; a genuine international one shown readably
#[test]
fn sec_10_lookalike_names() {
    let site = Site::new(&[(
        "/",
        "<p><a href=\"https://xn--pple-43d.com/\">Apple</a></p><p><a href=\"https://xn--bcher-kva.de/\">Books</a></p>",
    )]);
    let (_home, mut rynx) = started("lookalike");
    rynx.open(&site.url("/"));
    rynx.wait_for("Apple");
    rynx.wait_for("Link: https://xn--pple-43d.com/ ⚠ look-alike name, written to pass for аpple.com");
    rynx.keys("\t");
    rynx.wait_for("Link: https://bücher.de/");
    assert!(!rynx.text().contains("look-alike"));
    rynx.quit();
}
