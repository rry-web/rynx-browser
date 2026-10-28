//! How pages are laid out, the cache and snapshots, images (RENDER, CACHE-03/04, IMG-03 in
//! TESTING.md, with local pages in place of the real sites they name)

use super::harness::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn started_with(name: &str, settings: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings(settings);
    let rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    (home, rynx)
}

/// RENDER-01: inline code reads as part of its sentence; a code block keeps its lines
#[test]
fn render_01_code() {
    let site = Site::new(&[(
        "/",
        "<p>Call <code>Vec::new</code> to make one, then <code>push</code> items.</p>\
         <pre>fn main() {\n    let v = vec![1, 2];\n    println!(\"{v:?}\");\n}</pre>",
    )]);
    let (_home, mut rynx) = started_with("code", "");
    rynx.open(&site.url("/"));
    rynx.wait_for("Call Vec::new to make one, then push items.");
    let rows = rynx.rows();
    assert!(rows.iter().any(|r| r.contains("│    let v = vec![1, 2];")), "the code block lost its indentation");
    rynx.quit();
}

/// RENDER-02, RENDER-03: a data table in columns with a header rule; narrow, as Header: value
#[test]
fn render_02_03_tables() {
    let site = Site::new(&[(
        "/",
        "<table><tr><th>Language</th><th>Year</th><th>Paradigm</th></tr>\
         <tr><td>Rust</td><td>2015</td><td>Multi-paradigm systems programming language</td></tr>\
         <tr><td>Go</td><td>2009</td><td>Concurrent, garbage collected</td></tr></table>",
    )]);
    let (_home, mut rynx) = started_with("tables", "");
    rynx.open(&site.url("/"));
    rynx.wait_for("Language");
    let text = rynx.text();
    assert!(text.contains("Language │ Year │ Paradigm"), "{}", rynx.screen());
    assert!(rynx.rows().iter().any(|r| r.contains("─┼─")), "no header rule");
    rynx.resize(30, 30);
    rynx.settle();
    rynx.wait_for("Language: Rust");
    rynx.wait_for("Year: 2009");
    rynx.resize(ROWS, COLS);
    rynx.quit();
}

/// RENDER-05: numbered lists line up past 9, nested bullets change, quotes have a bar
#[test]
fn render_05_lists_and_quotes() {
    let items: String = (1..=11).map(|i| format!("<li>Item number {i}</li>")).collect();
    let site = Site::new(&[(
        "/",
        &format!(
            "<ol>{items}</ol><ul><li>Outer<ul><li>Middle<ul><li>Inner</li></ul></li></ul></li></ul><blockquote>Quoted words</blockquote>"
        ),
    )]);
    let (_home, mut rynx) = started_with("lists", "");
    rynx.open(&site.url("/"));
    rynx.wait_for("Item number 11");
    let rows = rynx.rows();
    let column = |text: &str| rows.iter().find_map(|r| r.find(text)).unwrap();
    assert_eq!(column(" 9. Item number 9"), column("10. Item number 10"), "numbers aren't right-aligned");
    assert!(
        rynx.text().contains("• Outer") && rynx.text().contains("◦ Middle") && rynx.text().contains("▪ Inner"),
        "{}",
        rynx.screen()
    );
    assert!(rows.iter().any(|r| r.contains("│ Quoted words")), "no quote bar");
    rynx.quit();
}

/// RENDER-06: a text file as written, its markup literal, its addresses links without the full stop
#[test]
fn render_06_plain_text() {
    let site = Site::new(&[]);
    site.respond(
        "/notes.txt",
        wiremock::ResponseTemplate::new(200)
            .set_body_raw("<b>not markup</b>\n\n    indented line\nSee https://example.com/docs.\n", "text/plain"),
    );
    let (_home, mut rynx) = started_with("plain-text", "");
    rynx.open(&site.url("/notes.txt"));
    rynx.wait_for("<b>not markup</b>");
    assert!(rynx.rows().iter().any(|r| r.contains("│    indented line")));
    rynx.wait_for("Link: https://example.com/docs ");
    rynx.quit();
}

/// RENDER-07: JSON keeps its lines and indentation
#[test]
fn render_07_json() {
    let site = Site::new(&[]);
    site.respond(
        "/data.json",
        wiremock::ResponseTemplate::new(200)
            .set_body_raw("{\n  \"slideshow\": {\n    \"title\": \"Sample\"\n  }\n}\n", "application/json"),
    );
    let (_home, mut rynx) = started_with("json", "");
    rynx.open(&site.url("/data.json"));
    rynx.wait_for("\"slideshow\"");
    assert!(rynx.rows().iter().any(|r| r.contains("│    \"title\": \"Sample\"")), "{}", rynx.screen());
    rynx.quit();
}

/// RENDER-08: a file that can't be shown is named, with its type and a way to save it
#[test]
fn render_08_non_text_files() {
    let site = Site::new(&[]);
    site.respond(
        "/icon.icns",
        wiremock::ResponseTemplate::new(200).set_body_raw(vec![0u8, 1, 2, 3, 255, 254], "image/x-icns"),
    );
    let (_home, mut rynx) = started_with("non-text", "");
    rynx.open(&site.url("/icon.icns"));
    rynx.wait_for("This is a image/x-icns file");
    rynx.wait_for("can't be shown as a page");
    rynx.keys("d");
    rynx.wait_for("icon.icns");
    rynx.keys("n");
    rynx.quit();
}

/// RENDER-09: text runs to within a column of the right border
#[test]
fn render_09_full_width() {
    let site = Site::new(&[("/", &format!("<p>{}</p>", "lorem ipsum dolor sit amet ".repeat(30)))]);
    let (_home, mut rynx) = started_with("full-width", "");
    rynx.open(&site.url("/"));
    rynx.wait_for("lorem ipsum");
    let longest = rynx
        .rows()
        .iter()
        .filter(|r| r.contains("lorem"))
        .map(|r| r.trim_end_matches(['│', ' ']).chars().count())
        .max()
        .unwrap();
    assert!(longest >= COLS as usize - 8, "lines stop at column {longest} of {COLS}");
    rynx.quit();
}

/// RENDER-10, RENDER-11: <noscript> shows as text; site menus are compact lines, still Tab-able
#[test]
fn render_10_11_noscript_and_menus() {
    let site = Site::new(&[(
        "/",
        "<nav><ul><li><a href=\"/\">Home</a></li><li><a href=\"/news\">News</a></li><li><a href=\"/sport\">Sport</a></li>\
         <li><a href=\"/weather\">Weather</a></li></ul></nav>\
         <noscript><p>This site works best with JavaScript.</p></noscript><h1>Front page</h1>\
         <h2>See also</h2><ul><li>First related article</li><li>Second related article</li></ul>",
    )]);
    let (_home, mut rynx) = started_with("menus", "");
    rynx.open(&site.url("/"));
    rynx.wait_for("Front page");
    rynx.wait_for("This site works best with JavaScript.");
    assert!(!rynx.text().contains("<p>"));
    rynx.wait_for("Home · News · Sport · Weather");
    rynx.wait_for("• First related article");
    rynx.keys("\t\t");
    rynx.wait_for(&format!("Link: {}", site.url("/sport")));
    rynx.quit();
}

/// CACHE-03: with the disk cache on, a page visited before opens after a restart with its site
/// gone; the settings page shows the space used, and Clear cache empties it
#[test]
fn cache_03_disk_cache() {
    let home = Home::new("disk-cache");
    home.settings("cache = on\n");
    let address;
    {
        let site = Site::new(&[("/kept", "<title>Kept</title><h1>A page worth keeping</h1>")]);
        address = site.url("/kept");
        let mut rynx = Rynx::start(&home);
        rynx.wait_for("RYNX BROWSER");
        rynx.open(&address);
        rynx.wait_for("A page worth keeping");
        rynx.quit();
    }
    // The site is gone now
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&address);
    rynx.wait_for("A page worth keeping");
    rynx.wait_for("Status: This is the copy saved");
    rynx.keys("S");
    rynx.scroll_to("Clear cache");
    rynx.click_text("Clear cache");
    rynx.wait_for("Cache cleared");
    rynx.quit();
    let mut after = Rynx::start(&home);
    after.wait_for("RYNX BROWSER");
    after.open(&address);
    after.wait_gone("A page worth keeping");
    after.quit();
}

/// A page whose text changes with each visit
struct Changing(Arc<AtomicUsize>);

impl wiremock::Respond for Changing {
    fn respond(&self, _: &wiremock::Request) -> wiremock::ResponseTemplate {
        let visit = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        let html = format!("<title>News</title><h1>Front page, edition {visit}</h1>");
        wiremock::ResponseTemplate::new(200).set_body_raw(html.into_bytes(), "text/html")
    }
}

/// CACHE-04: a tracked site's changed pages are kept as dated snapshots, which open as they were
#[test]
fn cache_04_snapshots() {
    let site = Site::new(&[]);
    site.mount(wiremock::Mock::given(wiremock::matchers::path("/news")).respond_with(Changing(Arc::default())));
    let (_home, mut rynx) = started_with("snapshots", "");
    rynx.open(&site.url("/news"));
    rynx.wait_for("Front page, edition 1");
    rynx.keys("T");
    rynx.wait_for("Tracking 127.0.0.1");
    rynx.keys("r");
    rynx.wait_for("Front page, edition 2");
    rynx.wait_for("Saved a snapshot of this page");
    rynx.keys("Z");
    rynx.wait_for("Snapshots");
    // The oldest is listed last
    let snapshots: Vec<(u16, u16)> = rynx
        .rows()
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains("20") && r.contains(':') && !r.contains("http"))
        .map(|(i, r)| (i as u16, r.find("20").unwrap() as u16))
        .collect();
    assert!(!snapshots.is_empty(), "no dated snapshots listed:\n{}", rynx.screen());
    let (row, col) = *snapshots.last().unwrap();
    rynx.click(row, col);
    rynx.wait_for("Snapshot from");
    rynx.wait_for("Front page, edition");
    rynx.keys("r");
    rynx.wait_for("Front page, edition 3");
    rynx.quit();
}

fn png() -> Vec<u8> {
    let image = image::RgbImage::from_fn(32, 32, |x, y| image::Rgb([(x * 8) as u8, (y * 8) as u8, 128]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// IMG-03: set to load the page's own images, those load by themselves, others wait for I
#[test]
fn img_03_own_images_load_by_themselves() {
    let other = Site::new(&[]);
    other.respond("/other.png", wiremock::ResponseTemplate::new(200).set_body_raw(png(), "image/png"));
    let other_address = other.url("/other.png").replace("127.0.0.1", "localhost");
    let site = Site::new(&[(
        "/",
        &format!(
            "<h1>Pictures</h1><p><img src=\"/own.png\" alt=\"Own picture\"></p><p><img src=\"{other_address}\" alt=\"Other picture\"></p>"
        ),
    )]);
    site.respond("/own.png", wiremock::ResponseTemplate::new(200).set_body_raw(png(), "image/png"));
    let (_home, mut rynx) = started_with("own-images", "images = site\n");
    rynx.open(&site.url("/"));
    rynx.wait_for("Other picture");
    rynx.settle();
    assert_eq!(site.requests("/own.png"), 1, "the page's own image didn't load");
    assert_eq!(other.requests("/other.png"), 0, "another site's image loaded by itself");
    rynx.keys("I");
    for _ in 0..50 {
        if other.requests("/other.png") > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(other.requests("/other.png"), 1, "I didn't load the other image");
    rynx.quit();
}

/// A gallery of loaded images is laid out side by side, captions under each
#[test]
fn image_galleries_side_by_side() {
    let site = Site::new(&[(
        "/",
        "<h1>Pictures</h1><ul class=gallery><li><img src=/a.png alt=Alpha></li><li><img src=/b.png alt=Beta></li>\
         <li><img src=/c.png alt=Gamma></li></ul><p>After the gallery</p>",
    )]);
    for name in ["/a.png", "/b.png", "/c.png"] {
        site.respond(name, wiremock::ResponseTemplate::new(200).set_body_raw(png(), "image/png"));
    }
    let (_home, mut rynx) = started_with("gallery", "images = site\n");
    rynx.open(&site.url("/"));
    rynx.wait_for("After the gallery");
    rynx.settle();
    let row = rynx.rows().into_iter().find(|r| r.contains("[Alpha]")).expect("no captions");
    assert!(row.contains("[Beta]") && row.contains("[Gamma]"), "not side by side: {row:?}");
    rynx.quit();
}
