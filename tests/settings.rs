//! The settings file and bookmarks.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use rynx::app::App;
use rynx::bookmarks::{BOOKMARKS_FILE, BOOKMARKS_PAGE, Bookmarks};
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::history::{HISTORY_FILE, HISTORY_PAGE, History, MAX_HISTORY};
use rynx::network::{NetworkResponse, PageKind};
use rynx::settings::{SETTINGS_FILE, Settings};
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
    assert_eq!(Settings::default().search_address("rust"), "https://old-search.marginalia.nu/search?query=rust");
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
    handle_key_event(app, key(code), 80, 24).unwrap();
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
    let resp = NetworkResponse::Success(
        id,
        "x".into(),
        "<title>An Article</title><p>text</p>".into(),
        url.into(),
        PageKind::Html,
    );
    handle_network_event(&mut app, resp, 80).unwrap();
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

#[test]
fn history_setting_is_parsed() {
    assert!(!Settings::default().save_history, "history must be opt-in");
    assert!(Settings::parse("history = on").0.save_history);
    assert!(!Settings::parse("history = off").0.save_history);
    assert_eq!(Settings::parse("history = yes please").1.len(), 1);
}

#[test]
fn history_is_saved_deduplicated_and_cleared() {
    let dir = temp_dir("history");
    let path = dir.join(HISTORY_FILE);
    let mut history = History::load(path.clone());
    history.record("https://a.example/", "A", false).unwrap();
    history.record("https://b.example/", "B\tline\nbreak", false).unwrap();
    history.record("http://site.i2p/", "Secret", true).unwrap();
    history.record("https://a.example/", "A again", false).unwrap();
    let urls: Vec<_> = history.entries.iter().map(|v| v.url.as_str()).collect();
    assert_eq!(urls, ["https://b.example/", "http://site.i2p/", "https://a.example/"]);

    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(!saved.contains(".i2p"), "I2P visits must never be written: {saved}");
    let reloaded = History::load(path.clone());
    let urls: Vec<_> = reloaded.entries.iter().map(|v| v.url.as_str()).collect();
    assert_eq!(urls, ["https://b.example/", "https://a.example/"]);
    assert_eq!(reloaded.entries[0].title, "B line break");

    let html = reloaded.page_html();
    assert!(html.find("A again").unwrap() < html.find("B line").unwrap(), "newest first: {html}");

    let mut history = reloaded;
    history.clear().unwrap();
    assert!(History::load(path).entries.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn history_is_capped() {
    let mut history = History::default();
    for i in 0..MAX_HISTORY + 5 {
        history.record(&format!("https://e.example/{i}"), "", false).unwrap();
    }
    assert_eq!(history.entries.len(), MAX_HISTORY);
    assert_eq!(history.entries[0].url, "https://e.example/5");
}

#[tokio::test]
async fn visits_are_listed_on_the_history_page_and_can_be_cleared() {
    let mut app = test_app(Settings::default());
    let url = "https://user:hunter2@example.com/page";
    let id = app.current_tab().id;
    app.current_tab().url_input = url.into();
    let resp =
        NetworkResponse::Success(id, "x".into(), "<title>A Page</title><p>text</p>".into(), url.into(), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    assert_eq!(app.history.entries.len(), 1);
    assert_eq!(app.history.entries[0].url, "https://example.com/page", "credentials are stripped");

    // D does nothing away from the history page
    press(&mut app, KeyCode::Char('D'));
    assert_eq!(app.history.entries.len(), 1);

    press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.current_tab().url_input, HISTORY_PAGE);
    assert!(text(&mut app).contains("A Page"));
    assert!(text(&mut app).contains("kept only until you quit"));
    assert_eq!(app.current_tab().link_regions[0].url, "https://example.com/page");

    // One D asks; another key cancels; two in a row clear
    press(&mut app, KeyCode::Char('D'));
    assert!(app.current_tab().status_message.contains("again"));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('D'));
    assert_eq!(app.history.entries.len(), 1);
    press(&mut app, KeyCode::Char('D'));
    assert!(app.history.entries.is_empty());
    assert!(text(&mut app).contains("No pages visited yet"));
}

#[test]
fn user_agent_setting_is_checked() {
    // Safari's, which many send, rather than one naming rynx
    assert!(Settings::default().user_agent.contains("Safari/"));
    assert!(!Settings::default().user_agent.contains("rynx"));
    let (settings, problems) = Settings::parse("user_agent = Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0");
    assert!(problems.is_empty());
    assert_eq!(settings.user_agent, "Mozilla/5.0 (X11; Linux x86_64) Firefox/140.0");
    // Anything that could break the header is refused
    for bad in ["user_agent = a\u{7f}b", "user_agent = caf\u{e9}", &format!("user_agent = {}", "x".repeat(300))] {
        let (settings, problems) = Settings::parse(bad);
        assert_eq!(problems.len(), 1, "{bad}");
        assert_eq!(settings.user_agent, Settings::default().user_agent);
    }
}

/// The headers of each request to a local server, which sends its page compressed, and the
/// page as shown
async fn requests_sent(settings: Settings, i2p: bool) -> (Vec<wiremock::http::HeaderMap>, String) {
    use std::io::Write;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(b"<p>Compressed hello</p>").unwrap();
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(gzip.finish().unwrap(), "text/html")
                .insert_header("content-encoding", "gzip"),
        )
        .mount(&server)
        .await;
    let mut settings = settings;
    // In I2P mode the local server stands in for the I2P proxy
    settings.i2p_proxy = server.uri();
    let mut app = test_app(settings);
    app.i2p_mode = i2p;
    app.current_tab().url_input = if i2p { "http://site.i2p/".into() } else { format!("{}/", server.uri()) };
    app.submit_request();
    loop {
        let resp = app.rx.recv().await.unwrap();
        let done = matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(&mut app, resp, 80).unwrap();
        if done {
            break;
        }
    }
    let requests = server.received_requests().await.unwrap();
    (requests.into_iter().map(|r| r.headers).collect(), text(&mut app))
}

fn header(headers: &wiremock::http::HeaderMap, name: &str) -> Option<String> {
    headers.get(name).map(|v| v.to_str().unwrap().to_string())
}

#[tokio::test]
async fn the_user_agent_is_sent() {
    let (default, page) = requests_sent(Settings::default(), false).await;
    assert_eq!(default.len(), 1);
    assert_eq!(header(&default[0], "user-agent").as_deref(), Some(rynx::constants::USER_AGENT));
    // With Safari's identity, Safari's other headers, and its compressed page still read
    assert_eq!(header(&default[0], "accept").as_deref(), Some(rynx::constants::SAFARI_ACCEPT));
    assert_eq!(header(&default[0], "accept-language").as_deref(), Some(rynx::constants::SAFARI_ACCEPT_LANGUAGE));
    assert_eq!(header(&default[0], "accept-encoding").as_deref(), Some(rynx::constants::SAFARI_ACCEPT_ENCODING));
    assert!(page.contains("Compressed hello"), "{page}");

    // Another identity doesn't carry Safari's headers
    let (custom, _) = Settings::parse("user_agent = Custom/1.0");
    let (sent, _) = requests_sent(custom.clone(), false).await;
    assert_eq!(header(&sent[0], "user-agent").as_deref(), Some("Custom/1.0"));
    assert_eq!(header(&sent[0], "accept-language"), None);

    // I2P mode always sends the generic one, whatever the setting, and no language
    for settings in [custom, Settings::default()] {
        let (sent, _) = requests_sent(settings, true).await;
        assert_eq!(header(&sent[0], "user-agent").as_deref(), Some(rynx::constants::I2P_USER_AGENT));
        assert_eq!(header(&sent[0], "accept-language"), None);
    }
}

// --- The settings file written back, and the settings page ---

#[test]
fn settings_files_round_trip() {
    // Every setting documented; defaults left as commented examples
    let template = Settings::default().to_file_text();
    assert!(template.contains("# network = i2p") && template.contains("# history = on"), "{template}");
    assert_eq!(Settings::parse(&template), (Settings::default(), Vec::new()));

    let (changed, problems) = Settings::parse(
        "home_page = https://example.org/\nnetwork = i2p\nhistory = on\nanubis = off\nuser_agent = Custom/1.0\n",
    );
    assert!(problems.is_empty(), "{problems:?}");
    let text = changed.to_file_text();
    assert!(text.contains("\nnetwork = i2p\n") && text.contains("\nhome_page = https://example.org/\n"), "{text}");
    assert_eq!(Settings::parse(&text), (changed, Vec::new()));
    assert_eq!(Settings::parse("network = sideways").1.len(), 1);
}

fn set_field(app: &mut App, name: &str, value: &str) {
    let tab = app.current_tab();
    let i = tab.controls.iter().position(|c| c.name == name).unwrap_or_else(|| panic!("no field {name}"));
    tab.control_values[i] = rynx::forms::ControlValue::Text(value.to_string());
}

fn choose(app: &mut App, name: &str, option: &str) {
    let tab = app.current_tab();
    let i = tab.controls.iter().position(|c| c.name == name).unwrap();
    let rynx::forms::ControlKind::Select { options } = &tab.controls[i].kind else { panic!("{name} isn't a choice") };
    let selected = options.iter().position(|o| o.value == option).unwrap();
    tab.control_values[i] = rynx::forms::ControlValue::Selected(selected);
}

fn tick(app: &mut App, name: &str, on: bool) {
    let tab = app.current_tab();
    let i = tab.controls.iter().position(|c| c.name == name).unwrap();
    tab.control_values[i] = rynx::forms::ControlValue::Checked(on);
}

#[tokio::test]
async fn the_settings_page_saves_and_applies_settings() {
    let dir = temp_dir("page");
    let mut app = test_app(Settings::default());
    app.config_dir = Some(dir.clone());
    app.history.record("https://visited.example/", "Visited", false).unwrap();

    press(&mut app, KeyCode::Char('S'));
    assert_eq!(app.current_tab().url_input, rynx::app::SETTINGS_PAGE);
    let page = text(&mut app);
    assert!(
        page.contains("Both: start on the clearweb, p switches")
            && page.contains("History")
            && page.contains("Save settings"),
        "{page}"
    );

    // A bad value is reported, and nothing is saved or changed
    set_field(&mut app, "home_page", "javascript:alert(1)");
    app.submit_form(0, None);
    assert!(
        app.current_tab().status_message.starts_with("Not saved: invalid home_page"),
        "{}",
        app.current_tab().status_message
    );
    assert!(!dir.join(SETTINGS_FILE).exists());

    set_field(&mut app, "home_page", "https://example.org/");
    choose(&mut app, "network", "i2p");
    tick(&mut app, "history", true);
    app.submit_form(0, None);
    assert_eq!(app.current_tab().status_message, "Settings saved");
    let saved = std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap();
    assert!(saved.contains("\nnetwork = i2p\n") && saved.contains("\nhistory = on\n"), "{saved}");
    assert_eq!(app.settings.home_page.as_deref(), Some("https://example.org/"));

    // Locked to I2P: on now, and p can't leave it
    assert!(app.i2p_mode);
    press(&mut app, KeyCode::Char('p'));
    assert!(app.i2p_mode, "p must not leave a locked I2P mode");
    assert!(app.current_tab().status_message.contains("locked"));

    // History saving turned on keeps this session's visits
    let history = std::fs::read_to_string(dir.join(rynx::history::HISTORY_FILE)).unwrap();
    assert!(history.contains("https://visited.example/"), "{history}");

    // The page shows what was saved
    assert!(text(&mut app).contains("https://example.org/"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn web_pages_cannot_change_settings() {
    let mut app = test_app(Settings::default());
    let id = app.current_tab().id;
    let html =
        r#"<form action="about:settings-save"><input name="network" value="clearweb"><input type="submit"></form>"#;
    app.current_tab().url_input = "https://evil.example/".into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), "https://evil.example/".into(), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    app.submit_form(0, None);
    assert!(app.current_tab().status_message.starts_with("Not sent"), "{}", app.current_tab().status_message);
    assert_eq!(app.settings, Settings::default());
}

#[tokio::test]
async fn a_clearweb_lock_disables_i2p_and_i2p_lock_starts_there() {
    let (clearweb, _) = Settings::parse("network = clearweb");
    let mut app = test_app(clearweb);
    press(&mut app, KeyCode::Char('p'));
    assert!(!app.i2p_mode);
    let (i2p, _) = Settings::parse("network = i2p");
    assert!(test_app(i2p).i2p_mode, "starts in I2P mode");
    // Switchable is the default and switches as before
    let mut app = test_app(Settings::default());
    press(&mut app, KeyCode::Char('p'));
    assert!(app.i2p_mode);
}

#[tokio::test]
async fn a_opens_the_archived_copy() {
    let mut app = test_app(Settings::default());
    // On the start page there's nothing to look up
    press(&mut app, KeyCode::Char('A'));
    assert!(app.current_tab().status_message.contains("Only web pages"));

    let id = app.current_tab().id;
    let url = "https://user:pw@gone.example/story?id=3";
    app.current_tab().url_input = url.into();
    let resp = NetworkResponse::Error(id, "Couldn't connect".into());
    handle_network_event(&mut app, resp, 80).unwrap();
    press(&mut app, KeyCode::Char('A'));
    // Even for a page that failed to load, and never with the password
    assert_eq!(app.current_tab().url_input, "https://web.archive.org/web/https://gone.example/story?id=3");

    app.current_tab().url_input = "http://site.i2p/".into();
    app.current_tab().page_url = Some("http://site.i2p/".into());
    press(&mut app, KeyCode::Char('A'));
    assert!(app.current_tab().status_message.contains(".i2p"));
}

#[tokio::test]
async fn new_tabs_show_the_start_page_address_and_back_returns_to_it() {
    let mut app = test_app(Settings::default());
    assert_eq!(app.current_tab().url_input, "about:help");
    let id = app.current_tab().id;
    app.follow_link("https://example.com/".into());
    let resp =
        NetworkResponse::Success(id, "t".into(), "<p>hi</p>".into(), "https://example.com/".into(), PageKind::Html);
    handle_network_event(&mut app, resp, 80).unwrap();
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.current_tab().url_input, "about:help", "back goes to the start page, not some website");
    assert!(text(&mut app).contains("Moving around"));
    press(&mut app, KeyCode::Char('t'));
    assert_eq!(app.current_tab().url_input, "about:help");
}

#[tokio::test]
async fn search_engines_are_chosen_or_custom_and_defaults_can_be_restored() {
    let dir = temp_dir("engines");
    let mut app = test_app(Settings::default());
    app.config_dir = Some(dir.clone());
    app.terminal_width = 100;
    press(&mut app, KeyCode::Char('S'));
    // Fields are wide enough to show most of a long value (80 columns of Safari's identity)
    let shown: String = rynx::constants::USER_AGENT.chars().take(80).collect();
    assert!(text(&mut app).contains(&format!("[{}", shown)), "{}", text(&mut app));

    choose(&mut app, "search_engine", "duckduckgo");
    app.submit_form(0, None);
    assert_eq!(app.settings.search_url, "https://html.duckduckgo.com/html/?q={query}");
    assert!(text(&mut app).contains("DuckDuckGo"), "the page shows the choice");

    choose(&mut app, "search_engine", "custom");
    set_field(&mut app, "search_url", "https://search.example/?q={query}");
    app.submit_form(0, None);
    assert_eq!(app.settings.search_url, "https://search.example/?q={query}");
    assert!(text(&mut app).contains("https://search.example/?q={query}"));

    // Restore defaults asks first; another key in between cancels
    let reset = app.current_tab().controls.iter().position(|c| c.name == "reset").unwrap();
    app.submit_form(0, Some(reset));
    assert!(app.current_tab().status_message.contains("again"));
    press(&mut app, KeyCode::Char('j'));
    app.submit_form(0, Some(reset));
    assert_eq!(app.settings.search_url, "https://search.example/?q={query}", "not restored yet");
    app.submit_form(0, Some(reset));
    assert_eq!(app.settings, Settings::default());
    assert_eq!(app.current_tab().status_message, "Settings restored to defaults");
    assert_eq!(std::fs::read_to_string(dir.join(SETTINGS_FILE)).unwrap(), Settings::default().to_file_text());
    let _ = std::fs::remove_dir_all(&dir);
}
