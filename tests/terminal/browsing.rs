//! Browsing: keys, the mouse, resizing, navigation, history, staying responsive (SMOKE, NAV,
//! HIST, CRASH, PERF, CACHE-01, READ-03 in TESTING.md)

use super::harness::*;
use std::time::{Duration, Instant};

fn two_pages() -> Site {
    Site::new(&[
        (
            "/a",
            "<title>Alpha</title><h1>Alpha page</h1><p><a href=\"/b\">Go to beta</a> and <a href=\"/c\">gamma</a></p>",
        ),
        ("/b", "<title>Beta</title><h1>Beta page</h1><p><a href=\"/a\">Back to alpha</a></p>"),
        ("/c", "<title>Gamma</title><h1>Gamma page</h1>"),
    ])
}

fn started(name: &str) -> (Home, Rynx) {
    let home = Home::new(name);
    home.settings("");
    let rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    (home, rynx)
}

/// SMOKE-01: tabs, edit mode, search, copy mode, source view, I2P mode, quit
#[test]
fn smoke_01_key_bindings() {
    let site = two_pages();
    let (_home, mut rynx) = started("smoke-keys");
    rynx.open(&site.url("/a"));
    rynx.wait_for("Alpha page");
    // F opens the selected link (the first, when a page opens) in a new tab; ] and [ move
    // between tabs; w closes one
    rynx.keys("F");
    rynx.wait_for("Beta page");
    assert!(rynx.rows()[1].contains("Alpha") && rynx.rows()[1].contains("Beta"), "{}", rynx.rows()[1]);
    rynx.keys("]");
    rynx.wait_for("Alpha page");
    rynx.keys("[");
    rynx.wait_for("Beta page");
    rynx.keys("w");
    rynx.wait_for("Alpha page");
    assert!(!rynx.rows()[1].contains("Beta"), "the tab wasn't closed: {}", rynx.rows()[1]);
    // t opens a new tab on the start page
    rynx.keys("t");
    rynx.wait_for("RYNX BROWSER");
    rynx.keys("w");
    rynx.wait_for("Alpha page");
    // Find: / then the words; > and < move between matches
    rynx.keys("/");
    rynx.type_text("page");
    rynx.wait_for("FIND match 1 of 1");
    rynx.keys("\r");
    rynx.wait_for("1 match for \"page\"");
    // Source view (Shift+V) and back
    rynx.keys("V");
    rynx.wait_for("<h1>Alpha page</h1>");
    rynx.keys("V");
    rynx.wait_gone("<h1>");
    // Copy mode (v), left with Esc (y would write the real clipboard)
    rynx.keys("v");
    rynx.wait_for("COPY MODE");
    rynx.keys("\u{1b}");
    rynx.wait_gone("COPY MODE");
    // I2P mode on and off
    rynx.keys("p");
    rynx.wait_for("I2P mode on");
    rynx.wait_for("I2P mode");
    rynx.keys("p");
    rynx.wait_for("I2P mode off");
    rynx.quit();
    assert!(rynx.left_terminal_as_found());
}

/// SMOKE-02: the wheel scrolls; a click follows a link; Ctrl+click opens it in a new tab
#[test]
fn smoke_02_mouse() {
    let site = Site::new(&[("/long", &long_page("Long", 200)), ("/b", "<h1>Beta page</h1>")]);
    site.page("/links", "<h1>Links</h1><p><a href=\"/b\">Click me</a></p><p><a href=\"/long\">Or me</a></p>");
    let (_home, mut rynx) = started("smoke-mouse");
    rynx.open(&site.url("/long"));
    rynx.wait_for("Paragraph 1 of Long");
    rynx.wheel(10);
    rynx.wait_gone("Paragraph 1 of Long,");
    rynx.wheel(-10);
    rynx.wait_for("Paragraph 1 of Long,");
    rynx.open(&site.url("/links"));
    rynx.wait_for("Click me");
    rynx.ctrl_click_text("Or me");
    rynx.wait_for("Paragraph 1 of Long");
    assert!(rynx.rows()[1].contains("links") && rynx.rows()[1].contains("Long"), "no new tab: {}", rynx.rows()[1]);
    rynx.keys("[");
    rynx.wait_for("Click me");
    rynx.click_text("Click me");
    rynx.wait_for("Beta page");
    rynx.quit();
}

/// SMOKE-03, PERF-04: resizing re-wraps the page, and the start page too
#[test]
fn smoke_03_resizing_rewraps() {
    let words = "word ".repeat(60);
    let site = Site::new(&[("/", &format!("<p>Start {words} end</p>"))]);
    let (_home, mut rynx) = started("resize");
    rynx.open(&site.url("/"));
    rynx.wait_for("Start word");
    let lines_with_words = |rynx: &Rynx| rynx.rows().iter().filter(|r| r.contains("word")).count();
    let wide = lines_with_words(&rynx);
    rynx.resize(40, 60);
    rynx.settle();
    assert!(lines_with_words(&rynx) > wide, "not re-wrapped:\n{}", rynx.screen());
    assert!(rynx.text().contains("end"));
    rynx.resize(ROWS, COLS);
    rynx.settle();
    assert_eq!(lines_with_words(&rynx), wide);
    // PERF-04: the start page re-wraps rather than disappearing
    rynx.keys("t");
    rynx.wait_for("RYNX BROWSER");
    rynx.resize(30, 70);
    rynx.settle();
    assert!(rynx.text().contains("RYNX BROWSER"), "{}", rynx.screen());
    rynx.quit();
}

/// CRASH-01: a link selected far down, then a short page typed in
#[test]
fn crash_01_stale_link_selection() {
    let mut links = String::new();
    for i in 0..120 {
        links.push_str(&format!("<p><a href=\"/x{i}\">Link {i}</a></p>"));
    }
    let site = Site::new(&[("/many", &links), ("/short", "<h1>Short page</h1>")]);
    let (_home, mut rynx) = started("stale");
    rynx.open(&site.url("/many"));
    rynx.wait_for("Link 0");
    rynx.keys(&"\t".repeat(100));
    rynx.open(&site.url("/short"));
    rynx.wait_for("Short page");
    rynx.keys("\t\r");
    assert!(rynx.is_running());
    rynx.quit();
}

/// CRASH-02, CRASH-05: a tiny or 2-column terminal, during a download box, then back
#[test]
fn crash_02_05_tiny_and_narrow_terminals() {
    let site = Site::new(&[("/", "<p><a href=\"/f.zip\">A file</a></p>")]);
    site.respond("/f.zip", wiremock::ResponseTemplate::new(200).set_body_raw(b"PK zip".to_vec(), "application/zip"));
    let (_home, mut rynx) = started("tiny");
    rynx.open(&site.url("/"));
    rynx.wait_for("A file");
    rynx.keys("\td");
    rynx.wait_for("f.zip");
    for (rows, cols) in [(3, 10), (1, 1), (40, 2), (2, 140)] {
        rynx.resize(rows, cols);
        assert!(rynx.is_running(), "crashed at {rows}x{cols}");
    }
    rynx.resize(ROWS, COLS);
    rynx.wait_for("f.zip");
    rynx.keys("n");
    rynx.keys("\u{1b}");
    rynx.keys("q");
    assert!(rynx.wait_exit());
}

/// CRASH-03: copy mode on an empty page's source
#[test]
fn crash_03_copy_mode_on_an_empty_page() {
    let (_home, mut rynx) = started("empty-visual");
    rynx.keys("t");
    rynx.keys("V");
    rynx.keys("v");
    rynx.keys("jkllhhjj");
    rynx.keys("\u{1b}");
    assert!(rynx.is_running());
    rynx.quit();
}

/// CRASH-04, CRASH-06: deeply nested lists and elements
#[test]
fn crash_04_06_deep_nesting() {
    let lists = format!("{}<li>deep item{}", "<ul>".repeat(60), "</ul>".repeat(60));
    let spans = format!("{}deep text{}", "<span>".repeat(20000), "</span>".repeat(20000));
    let site = Site::new(&[("/nested", &lists), ("/deep", &spans)]);
    let (_home, mut rynx) = started("deep");
    rynx.open(&site.url("/nested"));
    rynx.wait_for_within("deep item", Duration::from_secs(5));
    rynx.open(&site.url("/deep"));
    rynx.wait_for_within("deep text", Duration::from_secs(20));
    rynx.quit();
}

/// NAV-01, NAV-02: Back after typing an address; Esc cancels editing
#[test]
fn nav_01_02_back_after_typing_and_esc() {
    let site = two_pages();
    let (_home, mut rynx) = started("nav-typed");
    rynx.open(&site.url("/a"));
    rynx.wait_for("Alpha page");
    rynx.open(&site.url("/b"));
    rynx.wait_for("Beta page");
    rynx.keys("\u{7f}");
    rynx.wait_for("Alpha page");
    rynx.keys("e");
    rynx.keys("\u{15}");
    rynx.type_text("junk junk");
    rynx.keys("\u{1b}");
    rynx.wait_for(&site.url("/a"));
    assert!(!rynx.text().contains("junk"));
    rynx.quit();
}

fn book() -> String {
    let mut html = String::from("<title>Book</title><h1>Contents</h1><p><a href=\"#ch5\">Chapter five</a></p>");
    for i in 1..=5 {
        html.push_str(&format!("<h2 id=\"ch{i}\">Chapter {i} heading</h2>"));
        for j in 1..=40 {
            html.push_str(&format!("<p>Chapter {i} line {j}.</p>"));
        }
        html.push_str(&format!("<p><a href=\"/after{i}\">Link after chapter {i}</a></p>"));
    }
    html
}

/// NAV-04, NAV-05, NAV-07: chapter links jump without reloading; Back returns to the top; t
/// opens the chapter in a new tab
#[test]
fn nav_04_05_07_jumps_within_a_page() {
    let site = Site::new(&[("/book", &book())]);
    let (_home, mut rynx) = started("chapters");
    rynx.open(&site.url("/book"));
    rynx.wait_for("Chapter five");
    // The first link is selected when a page opens
    rynx.keys("\r");
    rynx.wait_for("Chapter 5 heading");
    rynx.wait_gone("Contents");
    assert!(rynx.text().contains("/book#ch5"), "{}", rynx.screen());
    assert_eq!(site.requests("/book"), 1, "the jump reloaded the page");
    // Tab goes on from the heading
    rynx.keys("\t");
    rynx.wait_for("/after5");
    rynx.keys("\u{7f}");
    rynx.wait_for("Contents");
    assert_eq!(site.requests("/book"), 1, "Back reloaded the page");
    // F on the chapter link (selected again once the page is opened anew): a new tab, at it
    rynx.open(&site.url("/book"));
    rynx.wait_for("Contents");
    rynx.keys("F");
    rynx.wait_for("Chapter 5 heading");
    assert!(rynx.rows()[1].matches("Book").count() >= 2, "{}", rynx.rows()[1]);
    rynx.quit();
}

/// NAV-06: an address with a #section starts there
#[test]
fn nav_06_fragment_on_another_page() {
    let site = Site::new(&[("/long", &long_page("Long", 200))]);
    let (_home, mut rynx) = started("fragment");
    rynx.open(&site.url("/long#p150"));
    rynx.wait_for("Paragraph 150 of Long");
    assert!(!rynx.text().contains("Paragraph 1 of Long,"));
    rynx.quit();
}

/// NAV-08: a redirect within a local server is followed, and the address bar updated
#[test]
fn nav_08_redirects_within_a_local_server() {
    let site = Site::new(&[("/dir/", "<h1>Directory listing</h1>")]);
    site.respond("/dir", wiremock::ResponseTemplate::new(301).insert_header("location", "/dir/"));
    let (_home, mut rynx) = started("redirect");
    rynx.open(&site.url("/dir"));
    rynx.wait_for("Directory listing");
    rynx.wait_for(&site.url("/dir/"));
    rynx.quit();
}

/// The paragraph numbers of the long page on screen
fn visible_paragraphs(rynx: &Rynx) -> Vec<usize> {
    rynx.rows().iter().filter_map(|r| r.split("Paragraph ").nth(1)?.split(' ').next()?.parse().ok()).collect()
}

/// HIST-01, HIST-02: Back keeps your place; Forward too; reload keeps it
#[test]
fn hist_01_02_back_forward_and_reload_keep_your_place() {
    let mut html = long_page("Long", 200);
    html.push_str("<p><a href=\"/b\">The very end link</a></p>");
    let long_b = long_page("Second", 200);
    let site = Site::new(&[("/long", &html), ("/b", &long_b)]);
    let (_home, mut rynx) = started("history-place");
    rynx.open(&site.url("/long"));
    rynx.wait_for("Paragraph 1 of Long");
    rynx.keys("G");
    rynx.wait_for("The very end link");
    let before = visible_paragraphs(&rynx);
    rynx.click_text("The very end link");
    rynx.wait_for("Paragraph 1 of Second");
    rynx.keys("\u{1b}[6~");
    rynx.keys("\u{1b}[6~");
    rynx.settle();
    let second = visible_paragraphs(&rynx);
    rynx.keys("\u{7f}");
    rynx.wait_for("The very end link");
    assert_eq!(visible_paragraphs(&rynx), before, "Back didn't return to the same place");
    rynx.keys("L");
    rynx.wait_for("of Second");
    rynx.settle();
    assert_eq!(visible_paragraphs(&rynx), second, "Forward didn't return to the same place");
    rynx.keys("r");
    std::thread::sleep(Duration::from_millis(500));
    rynx.settle();
    assert_eq!(visible_paragraphs(&rynx), second, "reload lost the place");
    rynx.quit();
}

/// HIST-03: PageDown/PageUp, Ctrl+d/Ctrl+u, G and g; Ctrl+d never downloads
#[test]
fn hist_03_paging_keys() {
    let site = Site::new(&[("/long", &long_page("Long", 300))]);
    let (_home, mut rynx) = started("paging");
    rynx.open(&site.url("/long"));
    rynx.wait_for("Paragraph 1 of Long");
    let top = |rynx: &Rynx| visible_paragraphs(rynx).first().copied().unwrap_or(0);
    rynx.keys("\u{1b}[6~");
    rynx.settle();
    let page = top(&rynx);
    // Paragraphs are a line and a gap, so a screen holds about 16
    assert!(page > 10, "PageDown moved to {page}");
    rynx.keys("\u{1b}[5~");
    rynx.settle();
    assert_eq!(top(&rynx), 1);
    rynx.keys("\u{4}");
    rynx.settle();
    let half = top(&rynx);
    assert!(half > 5 && half < page, "Ctrl+d moved to {half} (a page is {page})");
    assert!(!rynx.text().contains("Download"), "Ctrl+d started a download");
    rynx.keys("\u{15}");
    rynx.settle();
    assert_eq!(top(&rynx), 1);
    rynx.keys("G");
    rynx.wait_for("Paragraph 300 of Long");
    rynx.keys("g");
    rynx.wait_for("Paragraph 1 of Long,");
    rynx.quit();
}

/// HIST-04: links under /files/ that are pages open as pages
#[test]
fn hist_04_files_links_that_are_pages() {
    let site = Site::new(&[
        ("/", "<p><a href=\"/files/guide.html\">The guide</a> <a href=\"/download/\">Downloads</a></p>"),
        ("/files/guide.html", "<h1>The guide itself</h1>"),
        ("/download/", "<h1>The downloads page</h1>"),
    ]);
    let (_home, mut rynx) = started("files-links");
    rynx.open(&site.url("/"));
    rynx.wait_for("The guide");
    rynx.click_text("The guide");
    rynx.wait_for("The guide itself");
    rynx.keys("\u{7f}");
    rynx.wait_for("Downloads");
    rynx.click_text("Downloads");
    rynx.wait_for("The downloads page");
    rynx.quit();
}

/// PERF-01, PERF-02: a book-sized page loads quickly, scrolls, and doesn't use much memory
#[test]
fn perf_01_02_large_pages() {
    let mut html = String::from("<title>Big</title>");
    for i in 0..8000 {
        html.push_str(&format!(
            "<p>Line {i}: Call me Ishmael. Some years ago, never mind how long precisely, having little money.</p>"
        ));
    }
    let site = Site::new(&[("/big", &html)]);
    let (_home, mut rynx) = started("big");
    let start = Instant::now();
    rynx.open(&site.url("/big"));
    // A debug build is several times slower than a release one; the limit allows for that
    rynx.wait_for_within("Line 0: Call me Ishmael", Duration::from_secs(20));
    let took = start.elapsed();
    rynx.keys("G");
    rynx.wait_for("Line 7999:");
    rynx.keys("g");
    rynx.wait_for("Line 0:");
    let memory = rynx.memory_mb();
    eprintln!("a {} KB page: shown in {:?}, {} MB in use", html.len() / 1024, took, memory);
    assert!(memory < 150, "{memory} MB in use");
    rynx.quit();
}

/// PERF-03: while a slow page is laid out, the browser responds
#[test]
fn perf_03_slow_pages_dont_freeze() {
    // TESTING.md's 50,000 take over a minute in a debug build; 15,000 still take seconds
    let slow = format!("{}slow page{}", "<div>".repeat(15000), "</div>".repeat(15000));
    let site = Site::new(&[("/slow", &slow)]);
    let (_home, mut rynx) = started("slow");
    rynx.open(&site.url("/slow"));
    rynx.wait_for("Rendering");
    // A new tab appears at once, though the page isn't laid out yet
    let start = Instant::now();
    rynx.keys("t");
    rynx.wait_for_within("RYNX BROWSER", Duration::from_secs(2));
    assert!(start.elapsed() < Duration::from_secs(2));
    rynx.keys("w");
    rynx.wait_for_within("slow page", Duration::from_secs(60));
    rynx.quit();
}

/// CACHE-01: Back is instant, from the cache
#[test]
fn cache_01_back_is_instant() {
    let site = Site::new(&[]);
    for (at, html) in [("/a", "<h1>Alpha page</h1><p><a href=\"/b\">Go to beta</a></p>"), ("/b", "<h1>Beta page</h1>")]
    {
        site.respond(
            at,
            wiremock::ResponseTemplate::new(200)
                .insert_header("cache-control", "max-age=600")
                .set_body_raw(html.as_bytes().to_vec(), "text/html"),
        );
    }
    let (_home, mut rynx) = started("cache-back");
    rynx.open(&site.url("/a"));
    rynx.wait_for("Alpha page");
    rynx.click_text("Go to beta");
    rynx.wait_for("Beta page");
    rynx.keys("\u{7f}");
    rynx.wait_for("Alpha page");
    assert_eq!(site.requests("/a"), 1, "Back fetched the page again");
    rynx.quit();
}

/// READ-03: reader mode stays on for articles, and says when a page has none
#[test]
fn read_03_reader_mode_stays_on() {
    let article = |title: &str, next: &str| {
        let body =
            "<p>This is a long paragraph of the article's story, with plenty of words in it to read. </p>".repeat(12);
        format!(
            "<title>{title}</title><nav><a href=\"/\">Home</a> <a href=\"/menu\">Site menu entry</a></nav>\
             <article><h1>{title}</h1>{body}<p><a href=\"{next}\">Next story</a></p></article><footer>Footer stuff</footer>"
        )
    };
    let site = Site::new(&[("/one", &article("First story", "/two")), ("/two", &article("Second story", "/"))]);
    site.page("/", "<title>Home</title><ul><li><a href=\"/one\">One</a></li><li><a href=\"/two\">Two</a></li></ul>");
    let (_home, mut rynx) = started("reader");
    rynx.open(&site.url("/one"));
    rynx.wait_for("Site menu entry");
    rynx.keys("R");
    rynx.wait_for("READER");
    rynx.wait_gone("Site menu entry");
    assert!(!rynx.text().contains("Footer stuff"));
    rynx.click_text("Next story");
    rynx.wait_for("Second story");
    rynx.wait_gone("Site menu entry");
    rynx.open(&site.url("/"));
    rynx.wait_for("no article found here");
    rynx.keys("R");
    rynx.wait_gone("READER");
    rynx.quit();
}

/// An about: address rynx doesn't have is said so, not searched for
#[test]
fn unknown_about_pages_arent_searched() {
    let home = Home::new("about-unknown");
    home.settings("search_url = http://127.0.0.1:9/never?q={query}\n");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open("about:blank");
    rynx.wait_for("There's no page about:blank in rynx");
    assert!(!rynx.text().contains("127.0.0.1:9"));
    rynx.quit();
}

/// The Vim-style keys: n / N between matches, o and O for an address (O in a new tab), J / K
/// between tabs, x closing one and X reopening it, F opening a link in a new tab
#[test]
fn vim_style_keys() {
    let site = two_pages();
    let (_home, mut rynx) = started("vim-keys");
    rynx.keys("o");
    rynx.keys("\u{15}");
    rynx.type_text(&site.url("/a"));
    rynx.keys("\r");
    rynx.wait_for("Alpha page");
    // Find, then n and N move between matches
    rynx.keys("/");
    rynx.type_text("a");
    rynx.keys("\r");
    rynx.wait_for("matches for \"a\"");
    let first = rynx.cells_with_background(2);
    rynx.keys("n");
    rynx.settle();
    let second = rynx.cells_with_background(2);
    assert_ne!(first, second, "n didn't move to the next match");
    rynx.keys("N");
    rynx.settle();
    assert_eq!(rynx.cells_with_background(2), first, "N didn't move back");
    // F opens the selected link in a new tab
    rynx.keys("\u{1b}");
    rynx.keys("F");
    rynx.wait_for("Beta page");
    // J and K move between tabs
    rynx.keys("J");
    rynx.wait_for("Alpha page");
    rynx.keys("K");
    rynx.wait_for("Beta page");
    // x closes the tab, X brings it back with its page
    rynx.keys("x");
    rynx.wait_for("Alpha page");
    assert!(!rynx.rows()[1].contains("Beta"));
    rynx.keys("X");
    rynx.wait_for("Beta page");
    rynx.wait_for("Reopened the tab");
    // O types an address in a new tab
    rynx.keys("O");
    rynx.wait_for("EDITING ADDRESS");
    rynx.type_text(&site.url("/c"));
    rynx.keys("\r");
    rynx.wait_for("Gamma page");
    assert!(rynx.rows()[1].matches('│').count() >= 3, "no third tab: {}", rynx.rows()[1]);
    #[cfg(target_os = "linux")]
    {
        // Y copies the address; on CI there's no clipboard, which it says
        rynx.keys("Y");
        rynx.wait_for("Clipboard unavailable");
    }
    rynx.quit();
}

/// Terminals that send ^H (Ctrl+H) for Backspace, as some openSUSE setups do: it deletes in the
/// address bar and in find, and goes back on a page, instead of typing "h"
#[test]
fn ctrl_h_is_backspace() {
    let site = two_pages();
    let (_home, mut rynx) = started("ctrl-h");
    rynx.open(&site.url("/a"));
    rynx.wait_for("Alpha page");
    rynx.open(&site.url("/b"));
    rynx.wait_for("Beta page");
    rynx.keys("\x08");
    rynx.wait_for("Alpha page");

    rynx.keys("e");
    rynx.type_text("zqxwv");
    rynx.wait_for("zqxwv");
    rynx.keys("\x08\x08");
    rynx.wait_gone("zqxwv");
    assert!(rynx.text().contains("zqx"), "{}", rynx.screen());
    assert!(!rynx.text().contains("zqxh") && !rynx.text().contains("zqxwvh"), "{}", rynx.screen());
    rynx.keys("\x1b");
    rynx.wait_for("Ready");

    rynx.keys("/");
    rynx.type_text("pagex");
    rynx.wait_for("FIND");
    rynx.keys("\x08");
    rynx.wait_for("FIND match 1 of 1");
    assert!(!rynx.text().contains("pagexh"), "{}", rynx.screen());
}

/// The start page's sections sit side by side on a wide screen, and one under another on a narrow
/// one, following the window as it's resized
#[test]
fn start_page_uses_the_width() {
    let (_home, mut rynx) = started("help-columns");
    let same_row = |rynx: &Rynx| rynx.rows().iter().any(|r| r.contains("Moving around") && r.contains("Tabs"));
    rynx.resize(50, 220);
    rynx.wait_for("Find on page, copying");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !same_row(&rynx) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(same_row(&rynx), "{}", rynx.screen());
    // Everything up to the last key fits on one screen
    assert!(rynx.text().contains("This page"), "{}", rynx.screen());

    rynx.resize(40, 80);
    let deadline = Instant::now() + Duration::from_secs(10);
    while same_row(&rynx) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(!same_row(&rynx), "{}", rynx.screen());
    assert!(rynx.text().contains("Moving around"), "{}", rynx.screen());
}

/// On a narrow screen the top of the window fits itself: the status loses its "Browser - [...]"
/// frame before its words, many tabs show the current one and its number, and a long address
/// being typed shows its end
#[test]
fn the_top_fits_a_narrow_window() {
    let site = two_pages();
    let (_home, mut rynx) = started("narrow-top");
    // The frame shows whole, or not at all
    let status_fits = |rynx: &Rynx| {
        let row = rynx.rows()[6].clone();
        !row.contains("Browser - [") || row.contains(']')
    };
    rynx.resize(40, 40);
    rynx.settle();
    assert!(status_fits(&rynx), "{}", rynx.screen());
    rynx.open(&site.url("/a"));
    rynx.wait_for("Alpha page");
    assert!(status_fits(&rynx), "{}", rynx.screen());

    // Six tabs can't all fit 40 columns: the current one stays in view, with its number
    for _ in 0..5 {
        rynx.keys("t");
    }
    rynx.keys("J");
    rynx.wait_for("Tabs 5/6");
    rynx.keys("K");
    rynx.wait_for("Tabs 6/6");
    assert!(rynx.rows()[1].contains("Start") || rynx.rows()[1].contains("New Tab"), "{}", rynx.screen());

    // Typing past the edge: the end, where the typing is, stays in view
    rynx.keys("e");
    rynx.type_text("https://example.com/a/very/long/path/that/goes/past/the/edge/xyzzy");
    rynx.wait_for("xyzzy");
    assert!(rynx.rows()[4].contains("…"), "{}", rynx.screen());
    assert!(rynx.rows()[3].contains("EDITING ADDRESS"), "{}", rynx.screen());
    rynx.keys("\x1b");

    // I2P mode is always named, however narrow
    rynx.resize(40, 24);
    rynx.keys("p");
    rynx.wait_for("I2P");
    assert!(rynx.rows()[3].contains("I2P"), "{}", rynx.screen());
}
