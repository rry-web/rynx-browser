//! Scrolling, the cursor, find (SCROLL in TESTING.md)

use super::harness::*;
use std::time::{Duration, Instant};

fn on_long_page(name: &str, n: usize) -> (Home, Rynx, Site) {
    let site = Site::new(&[("/long", &long_page("Long", n))]);
    let home = Home::new(name);
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/long"));
    rynx.wait_for("Paragraph 1 of Long");
    (home, rynx, site)
}

/// The paragraph numbers on screen
fn paragraphs(rynx: &Rynx) -> Vec<usize> {
    rynx.rows().iter().filter_map(|r| r.split("Paragraph ").nth(1)?.split(' ').next()?.parse().ok()).collect()
}

/// SCROLL-02: the wheel and Down stop with the last line at the bottom; up responds at once
#[test]
fn scroll_02_scrolling_stops_at_the_end() {
    let (_home, mut rynx, _site) = on_long_page("scroll-end", 120);
    rynx.wheel(200);
    rynx.wait_for("Paragraph 120 of Long");
    rynx.settle();
    let last_row = rynx.rows().iter().rposition(|r| r.contains("Paragraph 120")).unwrap();
    // The last line is at the bottom of the page area, not scrolled away above blank space
    assert!(last_row >= (ROWS - 4) as usize, "the end is at row {last_row}:\n{}", rynx.screen());
    for _ in 0..20 {
        rynx.keys("\x1b[B");
    }
    rynx.settle();
    assert!(rynx.text().contains("Paragraph 120 of Long"));
    let start = Instant::now();
    rynx.wheel(-3);
    rynx.wait_for_within("Paragraph 11", Duration::from_secs(1));
    assert!(start.elapsed() < Duration::from_secs(1), "scrolling back up lagged");
    rynx.quit();
}

/// SCROLL-03: after the wheel, j and k move the cursor from what's on screen
#[test]
fn scroll_03_j_and_k_continue_from_the_view() {
    let (_home, mut rynx, _site) = on_long_page("scroll-jk", 200);
    rynx.wheel(40);
    rynx.settle();
    let first = paragraphs(&rynx)[0];
    assert!(first > 30, "the wheel didn't scroll ({first})");
    rynx.keys("j");
    rynx.keys("k");
    rynx.settle();
    assert_eq!(paragraphs(&rynx)[0], first, "the view jumped");
    let cursor = rynx.reversed_cells();
    assert_eq!(cursor.len(), 1, "{cursor:?}");
    rynx.quit();
}

/// SCROLL-04, SCROLL-05: in copy mode, holding j past the bottom scrolls and the selection
/// grows; the wheel extends it too
#[test]
fn scroll_04_05_copy_mode_follows_the_cursor() {
    let (_home, mut rynx, _site) = on_long_page("scroll-visual", 200);
    rynx.keys("v");
    rynx.wait_for("COPY MODE");
    rynx.keys(&"j".repeat(60));
    rynx.settle();
    assert!(paragraphs(&rynx)[0] > 5, "the view didn't follow the cursor");
    let selected = |rynx: &Rynx| {
        rynx.text()
            .split("COPY MODE ")
            .nth(1)
            .and_then(|t| t.split(' ').next())
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0)
    };
    let by_keys = selected(&rynx);
    assert!(by_keys > 1000, "{by_keys} characters selected");
    // The cursor stays where it is until scrolling would leave it off screen; then it, and
    // the selection, move with the view
    rynx.wheel(30);
    rynx.settle();
    assert!(selected(&rynx) > by_keys, "the wheel didn't extend the selection");
    rynx.keys("\u{1b}");
    rynx.quit();
}

/// SCROLL-06: a click below the page puts the cursor on its last line
#[test]
fn scroll_06_clicking_below_the_page() {
    let site = Site::new(&[("/", "<p>First line here</p><p>Last line here</p>")]);
    let home = Home::new("click-below");
    home.settings("");
    let mut rynx = Rynx::start(&home);
    rynx.wait_for("RYNX BROWSER");
    rynx.open(&site.url("/"));
    rynx.wait_for("Last line here");
    rynx.click(ROWS - 4, 20);
    rynx.settle();
    let (last_row, _) = rynx.find("Last line here").unwrap();
    // The page's last line: its text, or the blank line the page ends with. (The cursor is
    // drawn on blank lines too.)
    let cursor = rynx.reversed_cells().first().map(|c| c.0).expect("the cursor isn't shown");
    assert!(cursor == last_row || cursor == last_row + 1, "the cursor is on row {cursor}, the last line on {last_row}");
    rynx.keys("kk");
    rynx.settle();
    let moved = rynx.reversed_cells().first().map(|c| c.0).expect("the cursor isn't shown");
    assert!(moved < cursor, "k didn't move up from the last line");
    rynx.quit();
}

/// SCROLL-07: quitting in the middle of a stream of mouse movement exits at once
#[test]
fn scroll_07_quitting_while_the_mouse_moves() {
    let (_home, mut rynx, _site) = on_long_page("mouse-moving", 50);
    let mut burst = String::new();
    for i in 0..400 {
        burst.push_str(&format!("\x1b[<35;{};{}M", 10 + i % 100, 10 + i % 20));
    }
    burst.push('q');
    for i in 0..400 {
        burst.push_str(&format!("\x1b[<35;{};{}M", 10 + i % 100, 10 + i % 20));
    }
    let start = Instant::now();
    rynx.keys(&burst);
    assert!(rynx.wait_exit());
    assert!(start.elapsed() < Duration::from_secs(3), "took {:?} to quit", start.elapsed());
    assert!(rynx.left_terminal_as_found());
}

/// SCROLL-08: a search with no matches says so, and Backspace and retyping still work
#[test]
fn scroll_08_search_survives_a_typo() {
    let (_home, mut rynx, _site) = on_long_page("search-typo", 50);
    rynx.keys("/");
    rynx.type_text("Paragrapx");
    rynx.wait_for("FIND no matches");
    rynx.keys("\u{7f}");
    rynx.type_text("h 7 ");
    rynx.wait_for("FIND match 1 of 1");
    rynx.keys("\r");
    rynx.quit();
}

/// SCROLL-09: > and < move the current match (green), and the view follows it
#[test]
fn scroll_09_search_jumps() {
    let (_home, mut rynx, _site) = on_long_page("search-jumps", 150);
    rynx.keys("/");
    rynx.type_text("Long,");
    rynx.wait_for("FIND match 1 of 150");
    rynx.keys("\r");
    rynx.wait_for("150 matches");
    assert!(!rynx.cells_with_background(2).is_empty(), "no current match shown in green");
    for _ in 0..60 {
        rynx.keys(">");
    }
    rynx.settle();
    let green = rynx.cells_with_background(2);
    assert!(!green.is_empty(), "the current match scrolled out of view");
    assert!(paragraphs(&rynx)[0] > 30, "the view didn't follow: {:?}", paragraphs(&rynx));
    for _ in 0..60 {
        rynx.keys("<");
    }
    rynx.settle();
    assert!(paragraphs(&rynx).contains(&1));
    rynx.quit();
}

/// SCROLL-10: in source view, the cursor is one character, not the whole line
#[test]
fn scroll_10_source_view_cursor() {
    let (_home, mut rynx, _site) = on_long_page("source-cursor", 20);
    rynx.keys("V");
    rynx.wait_for("<h1>Long</h1>");
    rynx.keys("jjll");
    rynx.settle();
    assert_eq!(rynx.reversed_cells().len(), 1, "{:?}", rynx.reversed_cells());
    rynx.quit();
}
