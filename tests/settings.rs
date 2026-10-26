//! The settings file and bookmarks.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::backend::TestBackend;
use rynx_browser::app::App;
use rynx_browser::bookmarks::{BOOKMARKS_FILE, BOOKMARKS_PAGE, Bookmarks};
use rynx_browser::event_handler::{handle_key_event, handle_network_event};
use rynx_browser::network::{NetworkResponse, PageKind};
use rynx_browser::settings::{SETTINGS_FILE, Settings};
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rynx-settings-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn settings_are_parsed() {
    let (settings, problems) = Settings::parse(
        "# comment\n\
         home_page = https://en.wikipedia.org/\n\
         search_url = \"https://html.duckduckgo.com/html/?q={query}\"\n\
         i2p_proxy = http://127.0.0.1:4445\n\
         download_dir = /tmp/dl\n",
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(settings.home_page.as_deref(), Some("https://en.wikipedia.org/"));
    assert_eq!(settings.search_address("rust & tui"), "https://html.duckduckgo.com/html/?q=rust+%26+tui");
    assert_eq!(settings.i2p_proxy, "http://127.0.0.1:4445");
    assert_eq!(settings.download_dir, Some(PathBuf::from("/tmp/dl")));
}

#[test]
fn bad_settings_are_reported_and_ignored() {
    let (settings, problems) = Settings::parse(
        "home_page = javascript:alert(1)\nsearch_url = file:///etc/passwd?q={query}\ni2p_proxy = socks5://x\ncolour = blue\njunk\n",
    );
    assert_eq!(settings, Settings::default());
    assert_eq!(problems.len(), 5, "{problems:?}");
}

#[test]
fn search_urls_without_a_placeholder_get_the_query_appended() {
    let (settings, _) = Settings::parse("search_url = https://example.com/search?q=");
    assert_eq!(settings.search_address("a b"), "https://example.com/search?q=a+b");
    // The default still searches Marginalia
    assert_eq!(Settings::default().search_address("rust"), "https://search.marginalia.nu/search?query=rust");
}

#[test]
fn a_commented_template_is_written_on_first_run() {
    let dir = temp_dir("template");
    let (settings, problems) = Settings::load_or_create(&dir);
    assert_eq!(settings, Settings::default());
    assert!(problems.is_empty());
    let template = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
    assert!(template.contains("# search_url ="));
    // Reading it back changes nothing: every setting is commented out
    assert_eq!(Settings::load_or_create(&dir), (Settings::default(), Vec::new()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bookmarks_are_saved_and_loaded() {
    let dir = temp_dir("bookmarks");
    let path = dir.join(BOOKMARKS_FILE);
    let mut bookmarks = Bookmarks::load(path.clone());
    assert_eq!(bookmarks.toggle("https://a.example/", "A <b>page</b>"), Ok(true));
    assert_eq!(bookmarks.toggle("https://b.example/", "Tab\there"), Ok(true));
    assert_eq!(bookmarks.toggle("https://a.example/", ""), Ok(false));

    let reloaded = Bookmarks::load(path);
    assert_eq!(reloaded.entries.len(), 1);
    assert_eq!(reloaded.entries[0].url, "https://b.example/");
    assert_eq!(reloaded.entries[0].title, "Tab here", "tabs would corrupt the file");

    let mut bookmarks = reloaded;
    bookmarks.toggle("https://c.example/", "A <b>page</b>").unwrap();
    let html = bookmarks.page_html();
    assert!(html.contains("A &lt;b&gt;page&lt;/b&gt;"), "titles must be escaped: {html}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() }
}

fn press(app: &mut App, code: KeyCode) {
    handle_key_event::<TestBackend>(app, key(code), 80, 24).unwrap();
}

fn test_app(settings: Settings) -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::with_settings(tx, rx, settings).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn pages_can_be_bookmarked_and_listed() {
    let mut app = test_app(Settings::default());
    // The start page isn't a web page
    press(&mut app, KeyCode::Char('b'));
    assert!(app.current_tab().status_message.contains("Only web pages"));

    let url = "https://example.com/article";
    let id = app.current_tab().id;
    app.current_tab().url_input = url.into();
    let resp = NetworkResponse::Success(id, "x".into(), "<title>An Article</title><p>text</p>".into(), url.into(), PageKind::Html);
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();
    press(&mut app, KeyCode::Char('b'));
    assert!(app.bookmarks.contains(url));

    press(&mut app, KeyCode::Char('B'));
    assert_eq!(app.current_tab().url_input, BOOKMARKS_PAGE);
    assert!(text(&mut app).contains("An Article"));
    assert_eq!(app.current_tab().link_regions[0].url, url);

    // Back returns to the article
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.current_tab().url_input, url);
}

#[tokio::test]
async fn internal_pages_can_be_typed_in_the_address_bar() {
    let mut app = test_app(Settings::default());
    app.current_tab().url_input = "about:bookmarks".into();
    app.load_url_input(false);
    assert!(text(&mut app).contains("No bookmarks yet"));
    app.current_tab().url_input = "about:help".into();
    app.load_url_input(false);
    assert!(text(&mut app).contains("Moving around"));
}

#[tokio::test]
async fn the_configured_search_engine_is_used() {
    let (settings, _) = Settings::parse("search_url = https://html.duckduckgo.com/html/?q={query}");
    let mut app = test_app(settings);
    app.current_tab().url_input = "terminal browsers".into();
    app.submit_request();
    assert_eq!(app.current_tab().url_input, "https://html.duckduckgo.com/html/?q=terminal+browsers");
}
