//! Real sites from TESTING.md, fetched and laid out as rynx does, checked for the basics: they
//! change their pages, and this notices when rynx stops coping. They need the internet, so
//! they're ignored by default (CI runs them weekly): `cargo test --test live_sites -- --ignored`

use rynx::network::{NetworkManager, PageKind, get_following_redirects, read_text_limited};
use rynx::renderer::{RenderJob, render_page};

const LIMIT: u64 = 10 * 1024 * 1024;

/// A page fetched and laid out at `width`: its final address and its lines
async fn page(url: &str, width: u16, reader: bool) -> (String, Vec<String>) {
    let manager = NetworkManager::new().unwrap();
    let response =
        get_following_redirects(manager.get_client(false), url, false).await.unwrap_or_else(|e| panic!("{url}: {e}"));
    let final_url = response.url().to_string();
    assert!(response.status().is_success(), "{url}: {}", response.status());
    let html = read_text_limited(response, LIMIT).await.unwrap();
    let job = RenderJob {
        source: html.into(),
        kind: PageKind::Html,
        source_view: false,
        reader,
        image_sizes: Default::default(),
        width,
    };
    let lines = render_page(&job).lines.iter().map(|l| l.to_string()).collect();
    (final_url, lines)
}

fn joined(lines: &[String]) -> String {
    lines.join("\n")
}

/// RENDER-01: inline code reads as part of its sentence; code blocks keep their indentation
#[tokio::test]
#[ignore]
async fn render_01_rust_documentation() {
    let (_, lines) = page("https://doc.rust-lang.org/std/vec/struct.Vec.html", 120, false).await;
    let text = joined(&lines);
    assert!(text.contains("Vec::new"), "no Vec::new");
    // A line of code with its indentation (the examples are inside fn main or blocks)
    assert!(lines.iter().any(|l| l.starts_with("    ") && l.contains(';')), "no indented code line");
    // Inline code doesn't break a sentence into one word per line
    let short = lines.iter().filter(|l| !l.trim().is_empty() && l.split_whitespace().count() == 1).count();
    assert!(short * 4 < lines.len(), "{short} of {} lines are a single word", lines.len());
}

/// RENDER-02, READ-02: a Wikipedia article's tables in columns; reader mode keeps the article only
#[tokio::test]
#[ignore]
async fn render_02_read_02_wikipedia() {
    let url = "https://en.wikipedia.org/wiki/Rust_(programming_language)";
    let (_, lines) = page(url, 120, false).await;
    let text = joined(&lines);
    assert!(text.contains(" │ "), "no table columns");
    assert!(text.contains("─┼─") || text.contains("─┼"), "no header rule");
    let (_, reader) = page(url, 120, true).await;
    let article = joined(&reader);
    assert!(article.contains("Rust") && article.contains("memory safety"), "the article is missing");
    assert!(reader.len() < lines.len(), "reader mode didn't drop anything");
    assert!(!article.contains("Main menu") && !article.contains("Jump to content"), "site menus in reader mode");
}

/// RENDER-04: Hacker News' layout table reads as its header and ranked titles
#[tokio::test]
#[ignore]
async fn render_04_hacker_news() {
    let (_, lines) = page("https://news.ycombinator.com/", 120, false).await;
    let text = joined(&lines);
    assert!(text.contains("Hacker News new | past"), "the header's words are glued or missing");
    assert!(text.contains("points") && text.contains("comments"), "no points/comments lines");
}

/// RENDER-11: site menus as compact lines
#[tokio::test]
#[ignore]
async fn render_11_site_menus() {
    let (_, lines) = page("https://en.wikipedia.org/", 120, false).await;
    assert!(lines.iter().any(|l| l.matches(" · ").count() >= 3), "no compact menu line");
}

/// NAV-03: a redirect to http:// is followed over https://
#[tokio::test]
#[ignore]
async fn nav_03_redirects_upgraded() {
    let (final_url, lines) = page("https://www.gutenberg.org/ebooks/2701.html.images", 100, false).await;
    assert!(final_url.starts_with("https://www.gutenberg.org/cache/epub/2701/"), "{final_url}");
    assert!(joined(&lines).contains("Moby"));
}

/// NAV-09, DL-07: an http://-only site isn't loaded over plain HTTP
#[tokio::test]
#[ignore]
async fn nav_09_http_only_sites_arent_downgraded() {
    let manager = NetworkManager::new().unwrap();
    let result = get_following_redirects(manager.get_client(false), "http://neverssl.com/", false).await;
    match result {
        Err(_) => {}
        Ok(response) => assert_eq!(response.url().scheme(), "https", "loaded over plain HTTP"),
    }
}

/// SET-11: git.kernel.org lets rynx's honest identity through, without an Anubis check
#[tokio::test]
#[ignore]
async fn set_11_honest_identity_passes() {
    let (_, lines) = page("https://git.kernel.org/", 120, false).await;
    let text = joined(&lines);
    assert!(!text.contains("Making sure you're not a bot"), "an Anubis check");
    assert!(text.contains("linux"), "no repository list");
}

/// SET-16: the Internet Archive answers for an address
#[tokio::test]
#[ignore]
async fn set_16_archived_copy() {
    let manager = NetworkManager::new().unwrap();
    let response = get_following_redirects(
        manager.get_client(false),
        "https://archive.org/wayback/available?url=example.com",
        false,
    )
    .await
    .unwrap();
    let body = read_text_limited(response, LIMIT).await.unwrap();
    assert!(body.contains("archived_snapshots"), "{body}");
}
