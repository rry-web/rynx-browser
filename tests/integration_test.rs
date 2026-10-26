use rynx_browser::app::App;
use rynx_browser::constants::{DEFAULT_TAB_INDEX, INITIAL_TAB_ID};
use rynx_browser::event_handler::handle_key_event;
use rynx_browser::event_handler::handle_network_event;
use rynx_browser::network::{NetworkResponse, PageKind};
use rynx_browser::ui::ui;
use tokio::sync::mpsc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_full_request_to_render_flow() {
    let mock_server = MockServer::start().await;
    let mock_html = "<html><title>Test Page</title><body><h1>Hello World</h1></body></html>";

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(mock_html, "text/html"))
        .mount(&mock_server)
        .await;

    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).expect("Failed to create App");
    app.current_tab().url_input = mock_server.uri();

    app.submit_request();

    // Loop until we get a terminal response (Success or Error)
    let mut final_response = None;
    while let Some(resp) = app.rx.recv().await {
        match resp {
            NetworkResponse::Success(..) | NetworkResponse::Error(..) => {
                final_response = Some(resp);
                break;
            }
            _ => continue, // Skip Loading or Info messages
        }
    }

    if let Some(NetworkResponse::Success(id, title, body, final_url, kind)) = final_response {
        // The page isn't parsed by the network task; its <title> arrives with the layout
        assert_eq!(title, "127.0.0.1");

        // Use the actual terminal width constant or a test value
        let test_width = 80;
        handle_network_event::<TestBackend>(
            &mut app,
            NetworkResponse::Success(id, title, body, final_url, kind),
            test_width,
        )
        .unwrap();
        // Pages are laid out on a background thread; wait for the result
        while app.is_rendering(app.active_tab_index) {
            let resp = app.rx.recv().await.unwrap();
            handle_network_event::<TestBackend>(&mut app, resp, test_width).unwrap();
        }
        assert_eq!(app.current_tab().page_title, "Test Page");

        // Verify the content exists somewhere in the rendered lines
        let found = app
            .current_tab()
            .rendered_content
            .iter()
            .any(|line| line.to_string().contains("Hello World"));

        assert!(found, "Content 'Hello World' not found in rendered output");
    } else {
        panic!("Did not receive Success response");
    }
}

#[tokio::test]
async fn test_search_url_normalization() {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut app = App::new(tx, rx).unwrap();

    // Set a non-URL search term
    app.current_tab().url_input = "rust programming".to_string();

    // This triggers the normalization logic in submit_request
    app.submit_request();

    // Verify it was transformed into a Marginalia search URL
    assert!(
        app.current_tab()
            .url_input
            .starts_with(rynx_browser::constants::MARGINALIA_SEARCH_URL)
    );
    assert!(
        app.current_tab()
            .url_input
            .contains("query=rust+programming")
    );
}

#[test]
fn test_utf8_selection_extraction() {
    use ratatui::text::Line;
    let content = vec![Line::from("Hello 🦀 World")]; // "🦀" is multi-byte
    let selection = rynx_browser::models::Selection {
        start_line: 0,
        start_char: 6, // Index of the crab
        end_line: 0,
        end_char: 7, // Just after the crab
    };

    let extracted = selection.extract_text(&content);
    assert_eq!(extracted, "🦀");
}

#[tokio::test]
async fn test_app_initialization() {
    let (tx, rx) = mpsc::channel(10);
    let app = App::new(tx, rx).expect("Failed to create App");

    // Verify initial state
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active_tab_index, DEFAULT_TAB_INDEX);
    assert_eq!(app.tabs[0].id, INITIAL_TAB_ID);
}

#[tokio::test]
async fn test_tab_management() {
    let (tx, rx) = mpsc::channel(10);
    let mut app = App::new(tx, rx).expect("Failed to create App");

    // Add a tab
    app.add_tab(Some("https://example.com".to_string()));
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab_index, 1);

    // Close a tab
    app.close_tab();
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active_tab_index, 0);
}

#[tokio::test]
async fn test_ui_rendering() {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut app = App::new(tx, rx).unwrap();

    // Simulate a URL change
    app.current_tab().url_input = "https://rust-lang.org".to_string();

    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| ui(f, &app)).unwrap();

    let buffer = terminal.backend().buffer();

    // Check if the URL appears in the buffer
    let buffer_string: String = buffer.content().iter().map(|c| c.symbol()).collect();
    assert!(buffer_string.contains("rust-lang.org"));
}

#[tokio::test]
async fn test_input_handling_switch_to_edit_mode() {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let mut app = App::new(tx, rx).unwrap();

    // Create a 'e' key event to enter edit mode
    let key_event = KeyEvent {
        code: KeyCode::Char('e'),
        modifiers: KeyModifiers::empty(),
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    };

    // Simulate key event on a 80x24 terminal
    let result = handle_key_event::<TestBackend>(&mut app, key_event, 80, 24);
    assert!(result.is_ok());

    // Verify app state changed to Editing
    assert_eq!(
        app.current_tab().input_mode,
        rynx_browser::models::InputMode::Editing
    );
}

// --- Regression test helpers ---

const TEST_PAGE_URL: &str = "http://127.0.0.1:1/page";

fn test_app() -> App {
    let (tx, rx) = mpsc::channel(10);
    let mut app = App::new(tx, rx).expect("Failed to create App");
    // Never let a test write into the real Downloads folder
    app.download_dir = unique_temp_dir();
    // Lay pages out inline so tests see results immediately; background rendering has its own tests
    app.render_in_background = false;
    app
}

fn unique_temp_dir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "rynx-test-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Go through the same prompt-then-confirm flow as the 'd' key and 'y' answer
fn start_download(app: &mut App, url: &str) {
    let dir = app.download_dir.clone();
    app.current_tab().initiate_download_request(url.to_string(), &dir);
    let prompt = app.current_tab().download_prompt.take().expect("prompt shown");
    app.trigger_download(prompt);
}

fn history_urls(tab: &rynx_browser::app::BrowserTab) -> Vec<&str> {
    tab.history.iter().map(|entry| entry.url.as_str()).collect()
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::empty(),
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

/// Deliver a page to the active tab as if it had just finished loading
fn load_page(app: &mut App, html: &str) {
    let id = app.current_tab().id;
    handle_network_event::<TestBackend>(
        app,
        NetworkResponse::Success(id, "Page".to_string(), html.to_string(), TEST_PAGE_URL.to_string(), PageKind::Html),
        80,
    )
    .unwrap();
}

fn links_page(count: usize) -> String {
    (0..count)
        .map(|i| format!("<p><a href='/l{i}'>link {i}</a></p>"))
        .collect()
}

#[tokio::test]
async fn test_new_page_resets_stale_link_selection() {
    let mut app = test_app();
    load_page(&mut app, &links_page(30));
    app.current_tab().selected_link_index = 25;
    app.current_tab().cursor_line = 40;

    // A shorter page arrives without going through Enter-on-link (e.g. typed URL, history back)
    load_page(&mut app, &links_page(2));

    assert_eq!(app.current_tab().selected_link_index, 0);
    assert_eq!(app.current_tab().cursor_line, 0);
    assert!(app.current_tab().search_state.is_none());

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
}

#[tokio::test]
async fn test_rerender_clamps_link_selection() {
    let mut app = test_app();
    load_page(&mut app, &links_page(30));
    app.current_tab().selected_link_index = 25;

    // Source view has no links, so the selection must be pulled back in range
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('V')), 80, 24).unwrap();
    assert_eq!(app.current_tab().selected_link_index, 0);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
}

#[tokio::test]
async fn test_visual_mode_on_empty_page_does_not_panic() {
    let mut app = test_app();
    // Source view of an empty page has no lines at all
    load_page(&mut app, "");
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('V')), 80, 24).unwrap();
    assert!(app.current_tab().rendered_content.is_empty());

    for code in [KeyCode::Char('v'), KeyCode::Char('j'), KeyCode::Char('k'), KeyCode::Char('l')] {
        handle_key_event::<TestBackend>(&mut app, key(code), 80, 24).unwrap();
    }
}

#[tokio::test]
async fn test_download_popups_render_in_tiny_terminal() {
    use rynx_browser::models::{Download, DownloadPrompt, DownloadStatus};

    let mut app = test_app();
    app.current_tab().download_state = Some(Download {
        _id: 0,
        filename: "file.zip".to_string(),
        bytes_downloaded: 10,
        total_size: Some(100),
        status: DownloadStatus::Active,
    });
    app.current_tab().download_prompt = Some(DownloadPrompt {
        url: "https://example.com/file.zip".to_string(),
        filename: "file.zip".to_string(),
        target_path: std::path::PathBuf::from("file.zip"),
        file_exists: false,
    });

    // Heights below the tab bar + URL bar + popup sizes
    for height in [1, 4, 6, 8, 10] {
        let mut terminal = Terminal::new(TestBackend::new(20, height)).unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();
    }
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        handle_key_event::<TestBackend>(app, key(KeyCode::Char(c)), 80, 24).unwrap();
    }
}

#[tokio::test]
async fn test_typed_url_records_previous_page_in_history() {
    let mut app = test_app();
    app.current_tab().url_input = "http://127.0.0.1:1/first".to_string();

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('e')), 80, 24).unwrap();
    app.current_tab().url_input.clear();
    type_text(&mut app, "http://127.0.0.1:1/second");
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Enter), 80, 24).unwrap();

    assert_eq!(history_urls(&app.tabs[0]), ["http://127.0.0.1:1/first"]);
    assert_eq!(app.current_tab().url_input, "http://127.0.0.1:1/second");
}

#[tokio::test]
async fn test_escape_restores_address_bar() {
    let mut app = test_app();
    app.current_tab().url_input = "http://127.0.0.1:1/first".to_string();

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('e')), 80, 24).unwrap();
    type_text(&mut app, "garbage");
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Esc), 80, 24).unwrap();

    assert_eq!(app.current_tab().url_input, "http://127.0.0.1:1/first");
    assert!(app.current_tab().history.is_empty());
}

#[tokio::test]
async fn test_search_survives_query_with_no_matches() {
    let mut app = test_app();
    load_page(&mut app, "<p>hello world</p>");

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('/')), 80, 24).unwrap();
    // Typo: "wx" matches nothing, then correct it to "wo"
    type_text(&mut app, "wx");
    assert_eq!(app.current_tab().input_mode, rynx_browser::models::InputMode::Search);
    let state = app.current_tab().search_state.as_ref().expect("query should be kept");
    assert_eq!(state.query, "wx");
    assert!(state.matches.is_empty());

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("[0/0]"));

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Backspace), 80, 24).unwrap();
    type_text(&mut app, "o");
    let state = app.current_tab().search_state.as_ref().unwrap();
    assert_eq!(state.query, "wo");
    assert_eq!(state.matches.len(), 1);
}

/// Receive network messages until a download reports Finished or DownloadError
async fn finish_download(app: &mut App) -> (NetworkResponse, usize) {
    let mut progress_messages = 0;
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv())
            .await
            .expect("download timed out")
            .expect("channel closed");
        match resp {
            NetworkResponse::DownloadProgress(..) => {
                progress_messages += 1;
                handle_network_event::<TestBackend>(app, resp, 80).unwrap();
            }
            NetworkResponse::DownloadFinished(..) | NetworkResponse::DownloadError(..) => {
                return (resp, progress_messages);
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_failed_download_keeps_current_page() {
    use rynx_browser::models::DownloadStatus;

    let mut app = test_app();
    load_page(&mut app, "<h1>My Page</h1>");
    let source_before = app.current_tab().html_source.clone();

    // Nothing listens on port 1, so the connection is refused
    start_download(&mut app, "http://127.0.0.1:1/file.zip");
    let (resp, _) = finish_download(&mut app).await;
    assert!(matches!(resp, NetworkResponse::DownloadError(..)));
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();

    let tab = app.current_tab();
    assert_eq!(tab.html_source, source_before, "page was replaced by download error");
    assert_eq!(tab.page_title, "Page");
    assert!(matches!(
        tab.download_state.as_ref().map(|d| &d.status),
        Some(DownloadStatus::Failed(_))
    ));
}

#[tokio::test]
async fn test_page_error_does_not_fail_active_download() {
    use rynx_browser::models::{Download, DownloadStatus};

    let mut app = test_app();
    app.current_tab().download_state = Some(Download {
        _id: 0,
        filename: String::new(),
        bytes_downloaded: 10,
        total_size: None,
        status: DownloadStatus::Active,
    });
    let id = app.current_tab().id;
    handle_network_event::<TestBackend>(&mut app, NetworkResponse::Error(id, "boom".into()), 80)
        .unwrap();

    assert!(matches!(
        app.current_tab().download_state.as_ref().map(|d| &d.status),
        Some(DownloadStatus::Active)
    ));
}

#[tokio::test]
async fn test_process_network_events_drains_queue() {
    use rynx_browser::event_handler::process_network_events;

    let mut app = test_app();
    let id = app.current_tab().id;
    for i in 0..5 {
        app.tx.send(NetworkResponse::Info(id, format!("msg {i}"))).await.unwrap();
    }

    process_network_events::<TestBackend>(&mut app, 80).unwrap();

    assert!(app.rx.try_recv().is_err(), "messages left in the queue");
    assert_eq!(app.current_tab().status_message, "msg 4");
}

#[tokio::test]
async fn test_download_progress_is_throttled() {
    use rynx_browser::models::DownloadStatus;

    let mock_server = MockServer::start().await;
    let body = vec![b'x'; 4 * 1024 * 1024];
    Mock::given(method("GET"))
        .and(path("/big.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let download_dir = app.download_dir.clone();

    start_download(&mut app, &format!("{}/big.bin", mock_server.uri()));
    let (resp, progress_messages) = finish_download(&mut app).await;
    let saved_path = match &resp {
        NetworkResponse::DownloadFinished(_, path) => std::path::PathBuf::from(path),
        _ => panic!("download did not finish"),
    };
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();

    let saved_len = std::fs::metadata(&saved_path).unwrap().len();
    let _ = std::fs::remove_dir_all(&download_dir);

    assert_eq!(saved_len, body.len() as u64);
    // Unthrottled, this was one message per network chunk (hundreds for 4MB)
    assert!(progress_messages <= 5, "got {progress_messages} progress messages");
    assert!(matches!(
        app.current_tab().download_state.as_ref().map(|d| &d.status),
        Some(DownloadStatus::Completed)
    ));
}

#[test]
fn test_download_filename_from_url() {
    let cases = [
        ("https://x.com/a/file.zip?token=abc#frag", "file.zip"),
        ("https://x.com/releases/v1.2/", "v1.2"),
        ("https://x.com/", "download.dat"),
        ("https://x.com/my%20report.pdf", "my_report.pdf"),
        // Encoded path traversal must collapse to a bare filename
        ("https://x.com/..%2F..%2Fetc%2Fpasswd", "passwd"),
    ];
    for (url, expected) in cases {
        assert_eq!(App::download_filename(url), expected, "for {url}");
    }
}

#[tokio::test]
async fn test_download_ignores_query_string_and_saves_to_prompted_path() {
    use rynx_browser::models::DownloadStatus;

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PK\x03\x04data".to_vec()))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let dir = app.download_dir.clone();
    let url = format!("{}/file.zip?token=secret", mock_server.uri());
    app.current_tab().initiate_download_request(url, &dir);
    let prompt = app.current_tab().download_prompt.take().unwrap();
    assert_eq!(prompt.filename, "file.zip");
    let expected_path = prompt.target_path.clone();

    app.trigger_download(prompt);
    // The overlay shows the real name from the start, not "Downloading..."
    assert_eq!(app.current_tab().download_state.as_ref().unwrap().filename, "file.zip");

    let (resp, _) = finish_download(&mut app).await;
    let NetworkResponse::DownloadFinished(_, saved) = &resp else { panic!("download failed") };
    assert_eq!(std::path::PathBuf::from(saved), expected_path);
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();
    assert!(matches!(
        app.current_tab().download_state.as_ref().map(|d| &d.status),
        Some(DownloadStatus::Completed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_download_of_error_page_is_not_saved() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing.zip"))
        .respond_with(ResponseTemplate::new(404).set_body_raw("<h1>Not Found</h1>", "text/html"))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let dir = app.download_dir.clone();
    start_download(&mut app, &format!("{}/missing.zip", mock_server.uri()));
    let (resp, _) = finish_download(&mut app).await;

    let NetworkResponse::DownloadError(_, msg) = &resp else { panic!("404 was treated as success") };
    assert!(msg.contains("404"), "unexpected message: {msg}");
    assert!(!dir.join("missing.zip").exists());
}

#[tokio::test]
async fn test_sniffed_extension_does_not_overwrite_existing_file() {
    let png = b"\x89PNG\r\n\x1a\nnew image".to_vec();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/photo"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(png.clone()))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let dir = app.download_dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("photo.png"), b"original").unwrap();

    // The prompt only warned about "photo", so "photo.png" must not be clobbered
    start_download(&mut app, &format!("{}/photo", mock_server.uri()));
    let (resp, _) = finish_download(&mut app).await;
    let NetworkResponse::DownloadFinished(_, saved) = &resp else { panic!("download failed") };

    assert_eq!(std::path::PathBuf::from(saved), dir.join("photo_1.png"));
    assert_eq!(std::fs::read(dir.join("photo.png")).unwrap(), b"original");
    assert_eq!(std::fs::read(dir.join("photo_1.png")).unwrap(), png);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_second_download_in_same_tab_is_refused() {
    let mut app = test_app();
    start_download(&mut app, "http://127.0.0.1:1/first.zip");

    let dir = app.download_dir.clone();
    app.current_tab().initiate_download_request("http://127.0.0.1:1/second.zip".into(), &dir);
    assert!(app.current_tab().download_prompt.is_none());
    assert!(app.current_tab().status_message.contains("already in progress"));
}

#[test]
fn test_jump_only_for_i2p_sites_in_i2p_mode() {
    use reqwest::StatusCode;
    use rynx_browser::network::should_attempt_jump;

    assert!(should_attempt_jump(true, "example.i2p", StatusCode::INTERNAL_SERVER_ERROR));
    assert!(should_attempt_jump(true, "example.i2p", StatusCode::SERVICE_UNAVAILABLE));
    // A clearweb server error must not leak the domain to I2P jump services
    assert!(!should_attempt_jump(false, "example.com", StatusCode::INTERNAL_SERVER_ERROR));
    assert!(!should_attempt_jump(true, "example.com", StatusCode::INTERNAL_SERVER_ERROR));
    assert!(!should_attempt_jump(false, "example.i2p", StatusCode::INTERNAL_SERVER_ERROR));
    assert!(!should_attempt_jump(true, "example.i2p", StatusCode::NOT_FOUND));
}

#[tokio::test]
async fn test_jump_tries_next_service_after_failure() {
    use rynx_browser::network::attempt_jump_with;

    let no_entry = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&no_entry)
        .await;
    let working = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/jump/example.i2p"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>found</p>", "text/html"))
        .mount(&working)
        .await;

    let unreachable = "http://127.0.0.1:1/jump/".to_string();
    let no_entry_base = format!("{}/jump/", no_entry.uri());
    let working_base = format!("{}/jump/", working.uri());
    let services = [unreachable.as_str(), no_entry_base.as_str(), working_base.as_str()];

    let (tx, _rx) = mpsc::channel(20);
    // Use the browser's own client: a bare reqwest Client needs a process-wide crypto provider
    let manager = rynx_browser::network::NetworkManager::new().unwrap();
    let resp = attempt_jump_with(manager.get_client(false), &services, "example.i2p", tx, 0)
        .await
        .expect("should fall through to the working service");
    assert_eq!(resp.text().await.unwrap(), "<p>found</p>");
}

#[tokio::test]
async fn test_missing_clipboard_reports_instead_of_panicking() {
    let mut app = test_app();
    app.clipboard = None;
    app.current_tab().url_input = "https://example.com".to_string();

    let ctrl = |c| KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    };
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('e')), 80, 24).unwrap();

    handle_key_event::<TestBackend>(&mut app, ctrl('y'), 80, 24).unwrap();
    assert_eq!(app.current_tab().status_message, "Clipboard unavailable");

    // Ctrl+K must not wipe the address when there is nothing to paste
    handle_key_event::<TestBackend>(&mut app, ctrl('k'), 80, 24).unwrap();
    assert_eq!(app.current_tab().url_input, "https://example.com");

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Esc), 80, 24).unwrap();
    load_page(&mut app, "<p>some text</p>");
    for code in [KeyCode::Char('v'), KeyCode::Char('l'), KeyCode::Char('l'), KeyCode::Char('y')] {
        handle_key_event::<TestBackend>(&mut app, key(code), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().status_message, "Clipboard unavailable");
}

#[tokio::test]
async fn test_cursor_movement_scrolls_in_normal_and_visual_mode() {
    let mut app = test_app();
    let lines: String = (0..60).map(|i| format!("line {i}<br>")).collect();
    load_page(&mut app, &lines);
    // 24 rows minus tab bar, URL bar and borders leaves 16 visible content rows
    let visible = 16;

    for _ in 0..20 {
        handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('j')), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().cursor_line, 20);
    assert_eq!(app.current_tab().scroll, 20 + 1 - visible);

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('v')), 80, 24).unwrap();
    for _ in 0..20 {
        handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('j')), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().cursor_line, 40);
    assert_eq!(app.current_tab().scroll, 40 + 1 - visible, "visual mode did not scroll");

    for _ in 0..40 {
        handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('k')), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().scroll, 0);
}

fn wheel(kind: crossterm::event::MouseEventKind) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent { kind, column: 10, row: 10, modifiers: KeyModifiers::empty() }
}

fn sixty_line_page(app: &mut App) {
    let lines: String = (0..60).map(|i| format!("line {i}<br>")).collect();
    load_page(app, &lines);
}

#[tokio::test]
async fn test_scrolling_stops_at_end_of_page() {
    use crossterm::event::MouseEventKind;
    use rynx_browser::event_handler::handle_mouse_event;

    let mut app = test_app();
    sixty_line_page(&mut app);
    let total = app.current_tab().rendered_content.len();
    // 24 rows leaves 16 for content; the last line should rest at the bottom of the view
    let max_scroll = total - 16;

    for _ in 0..100 {
        handle_mouse_event::<TestBackend>(&mut app, wheel(MouseEventKind::ScrollDown), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().scroll, max_scroll);

    for _ in 0..100 {
        handle_key_event::<TestBackend>(&mut app, key(KeyCode::Down), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().scroll, max_scroll);

    // Scrolling back up takes effect immediately instead of unwinding overshoot first
    handle_mouse_event::<TestBackend>(&mut app, wheel(MouseEventKind::ScrollUp), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, max_scroll - 3);
}

#[tokio::test]
async fn test_cursor_follows_wheel_scroll_so_j_does_not_jump_to_top() {
    use crossterm::event::MouseEventKind;
    use rynx_browser::event_handler::handle_mouse_event;

    let mut app = test_app();
    sixty_line_page(&mut app);
    for _ in 0..10 {
        handle_mouse_event::<TestBackend>(&mut app, wheel(MouseEventKind::ScrollDown), 80, 24).unwrap();
    }
    assert_eq!(app.current_tab().scroll, 30);
    assert_eq!(app.current_tab().cursor_line, 30, "cursor should be pulled onto the screen");

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('j')), 80, 24).unwrap();
    assert_eq!(app.current_tab().cursor_line, 31);
    assert_eq!(app.current_tab().scroll, 30, "j jumped the view");

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('k')), 80, 24).unwrap();
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('k')), 80, 24).unwrap();
    assert_eq!(app.current_tab().cursor_line, 29);
    assert_eq!(app.current_tab().scroll, 29, "k should scroll by one line, not to the top");
}

#[tokio::test]
async fn test_wheel_scroll_in_visual_mode_extends_selection() {
    use crossterm::event::MouseEventKind;
    use rynx_browser::event_handler::handle_mouse_event;

    let mut app = test_app();
    sixty_line_page(&mut app);
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('v')), 80, 24).unwrap();
    for _ in 0..10 {
        handle_mouse_event::<TestBackend>(&mut app, wheel(MouseEventKind::ScrollDown), 80, 24).unwrap();
    }
    let sel = app.current_tab().selection.as_ref().unwrap();
    assert_eq!((sel.start_line, sel.end_line), (0, 30));
}

/// A table of contents linking to a chapter heading far down the page
fn book_page() -> String {
    let filler: String = (0..50).map(|i| format!("<p>filler {i}</p>")).collect();
    format!(
        "<p><a href='#ch2'>Go to chapter two</a> <a href='#nowhere'>Broken</a></p>{filler}\
         <p><a id='ch2'></a></p><h2>Chapter Two</h2><p><a href='next.html'>Next</a></p>{filler}"
    )
}

fn line_text(app: &mut App, line: usize) -> String {
    app.current_tab().rendered_content[line].to_string()
}

/// Wait a moment and report whether the tab started a network request
async fn made_request(app: &mut App) -> bool {
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    std::iter::from_fn(|| app.rx.try_recv().ok()).any(|r| matches!(r, NetworkResponse::Loading(_)))
}

#[tokio::test]
async fn test_fragment_link_jumps_within_page_without_reloading() {
    let mut app = test_app();
    load_page(&mut app, &book_page());
    let anchor_line = app.current_tab().anchors["ch2"];
    assert!(line_text(&mut app, anchor_line + 1).contains("Chapter Two") || line_text(&mut app, anchor_line).contains("Chapter Two"));

    // First link is selected by default
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Enter), 80, 24).unwrap();

    let tab = app.current_tab();
    assert_eq!(tab.scroll, anchor_line);
    assert_eq!(tab.cursor_line, anchor_line);
    assert_eq!(tab.url_input, format!("{TEST_PAGE_URL}#ch2"));
    assert_eq!(history_urls(tab), [TEST_PAGE_URL]);
    // Tab continues from the chapter, not from the top of the page
    assert_eq!(tab.link_regions[tab.selected_link_index].url, "next.html");
    assert!(!made_request(&mut app).await, "fragment link refetched the page");

    // Back returns to the top of the same page, also without a refetch
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Backspace), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, 0);
    assert_eq!(app.current_tab().url_input, TEST_PAGE_URL);
    assert!(!made_request(&mut app).await, "going back refetched the page");
}

#[tokio::test]
async fn test_missing_anchor_reports_and_stays_put() {
    let mut app = test_app();
    load_page(&mut app, &book_page());
    app.current_tab().scroll = 5;

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Tab), 80, 24).unwrap();
    app.current_tab().scroll = 5;
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Enter), 80, 24).unwrap();

    assert_eq!(app.current_tab().scroll, 5);
    assert!(app.current_tab().status_message.contains("#nowhere"));
    assert!(!made_request(&mut app).await);
}

#[tokio::test]
async fn test_loading_url_with_fragment_jumps_after_load() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/book.html"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(book_page(), "text/html"))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let url = format!("{}/book.html#ch2", mock_server.uri());
    app.current_tab().url_input = url.clone();
    app.submit_request();
    loop {
        match app.rx.recv().await.unwrap() {
            resp @ NetworkResponse::Success(..) => {
                handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();
                break;
            }
            NetworkResponse::Error(_, e) => panic!("load failed: {e}"),
            _ => {}
        }
    }

    let anchor_line = app.current_tab().anchors["ch2"];
    assert!(anchor_line > 0);
    assert_eq!(app.current_tab().scroll, anchor_line);
    assert_eq!(app.current_tab().url_input, url);
}

#[tokio::test]
async fn test_redirected_page_resolves_links_against_final_url() {
    let mut app = test_app();
    app.current_tab().url_input = "https://example.com/ebooks/1.html".to_string();
    let id = app.current_tab().id;
    handle_network_event::<TestBackend>(
        &mut app,
        NetworkResponse::Success(
            id,
            "Book".into(),
            "<a href='images/cover.jpg'>cover</a>".into(),
            "https://example.com/cache/epub/1/book.html".into(),
            PageKind::Html,
        ),
        80,
    )
    .unwrap();

    let tab = app.current_tab();
    assert_eq!(tab.url_input, "https://example.com/cache/epub/1/book.html");
    let resolved = rynx_browser::network::resolve_url(tab.link_base(), &tab.link_regions[0].url);
    assert_eq!(resolved, "https://example.com/cache/epub/1/images/cover.jpg");
}

#[test]
fn test_enforce_https_rules() {
    use rynx_browser::network::enforce_https;

    let cases = [
        // Only the scheme changes, not http:// inside the query
        ("http://example.com/?next=http://other.com", false, "https://example.com/?next=http://other.com"),
        // ".i2p" elsewhere in the URL must not exempt a clearweb host
        ("http://evil.com/?x=.i2p", false, "https://evil.com/?x=.i2p"),
        ("http://site.i2p/page", false, "http://site.i2p/page"),
        ("http://localhost:8080/", false, "http://localhost:8080/"),
        ("http://127.0.0.5/", false, "http://127.0.0.5/"),
        ("http://[::1]:3000/", false, "http://[::1]:3000/"),
        ("https://example.com/", false, "https://example.com/"),
        // I2P mode routes everything through the proxy unchanged
        ("http://example.com/", true, "http://example.com/"),
    ];
    for (url, i2p_mode, expected) in cases {
        assert_eq!(enforce_https(url, i2p_mode), expected, "for {url} (i2p_mode={i2p_mode})");
    }
}

#[test]
fn test_redirect_checks_block_local_network_from_public_sites() {
    use rynx_browser::network::check_redirect;
    use url::Url;

    let public = Url::parse("https://example.com/").unwrap();
    let local = Url::parse("http://127.0.0.1:8000/").unwrap();
    for target in [
        "http://127.0.0.1/",
        "http://127.0.0.2/",
        "http://localhost/",
        "http://[::1]/",
        "http://0.0.0.0/",
        "http://10.0.0.1/",
        "http://192.168.1.1/",
        "http://172.16.0.1/",
        "http://169.254.169.254/latest/meta-data/",
        "http://[fd00::1]/",
        "http://[::ffff:127.0.0.1]/",
    ] {
        let to = Url::parse(target).unwrap();
        assert!(check_redirect(&public, &to).is_err(), "public -> {target} was allowed");
    }
    assert!(check_redirect(&public, &Url::parse("https://other.org/").unwrap()).is_ok());
    // Browsing a local server and being redirected within it is fine
    assert!(check_redirect(&local, &Url::parse("http://localhost:8000/x").unwrap()).is_ok());
    assert!(check_redirect(&public, &Url::parse("file:///etc/passwd").unwrap()).is_err());
}

/// Load a URL in the active tab and wait for the page or error to arrive
async fn load_url(app: &mut App, url: &str) -> NetworkResponse {
    app.current_tab().url_input = url.to_string();
    app.submit_request();
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv())
            .await
            .expect("load timed out")
            .unwrap();
        if matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..)) {
            return resp;
        }
    }
}

#[tokio::test]
async fn test_redirects_are_followed_to_final_page() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/old"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/new"))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/new"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>moved here</p>", "text/html"))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/old", mock_server.uri())).await;
    let NetworkResponse::Success(_, _, body, final_url, _) = resp else { panic!("redirect not followed") };
    assert!(body.contains("moved here"));
    assert_eq!(final_url, format!("{}/new", mock_server.uri()));
}

#[tokio::test]
async fn test_redirect_to_http_is_upgraded_to_https() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/out"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://example.invalid/book.html"),
        )
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    // .invalid never resolves, so the error shows which URL the browser actually tried
    let resp = load_url(&mut app, &format!("{}/out", mock_server.uri())).await;
    let NetworkResponse::Error(_, msg) = resp else { panic!("expected a connection error") };
    assert!(msg.contains("https://example.invalid/book.html"), "redirect was not upgraded: {msg}");
}

#[tokio::test]
async fn test_redirect_loop_is_stopped() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/loop"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/loop"))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/loop", mock_server.uri())).await;
    let NetworkResponse::Error(_, msg) = resp else { panic!("loop not stopped") };
    assert!(msg.contains("Too many redirects"));
}

#[tokio::test]
async fn test_download_follows_redirects() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/get/report.pdf"))
        .respond_with(ResponseTemplate::new(301).insert_header("Location", "/files/real.pdf"))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/real.pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF-1.7 data".to_vec()))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let dir = app.download_dir.clone();
    start_download(&mut app, &format!("{}/get/report.pdf", mock_server.uri()));
    let (resp, _) = finish_download(&mut app).await;
    let NetworkResponse::DownloadFinished(_, saved) = resp else { panic!("download failed") };
    assert_eq!(std::fs::read(&saved).unwrap(), b"%PDF-1.7 data");
    let _ = std::fs::remove_dir_all(&dir);
}

fn screen_cells(terminal: &Terminal<TestBackend>) -> Vec<String> {
    terminal.backend().buffer().content().iter().map(|c| c.symbol().to_string()).collect()
}

#[tokio::test]
async fn test_control_characters_never_reach_the_terminal() {
    use rynx_browser::models::{Download, DownloadStatus};

    // OSC 0 sets the window title, OSC 52 writes the clipboard, CSI 2J clears the screen,
    // and U+009B is the single-byte C1 form of CSI
    let evil = "\x1b]52;c;ZWNobyBoaQ==\x07\x1b[2J\u{9b}31m";
    let html = format!(
        "<p>safe {evil}text</p><img alt='pic{evil}'><pre>col1\tcol2{evil}</pre>"
    );
    let mut app = test_app();
    let id = app.current_tab().id;
    handle_network_event::<TestBackend>(
        &mut app,
        NetworkResponse::Success(id, "Title\x1b]0;pwned\x07".to_string(), html, TEST_PAGE_URL.into(), PageKind::Html),
        80,
    )
    .unwrap();
    app.current_tab().status_message = format!("status{evil}");
    app.current_tab().download_state = Some(Download {
        _id: id,
        filename: format!("file{evil}.zip"),
        bytes_downloaded: 0,
        total_size: None,
        status: DownloadStatus::Failed(format!("boom{evil}")),
    });

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    for source_view in [false, true] {
        app.current_tab().is_source_view = source_view;
        app.render_tab(0, 100);
        terminal.draw(|f| ui(f, &app)).unwrap();
        let bad: Vec<_> = screen_cells(&terminal)
            .into_iter()
            .filter(|s| s.contains(char::is_control))
            .collect();
        assert!(bad.is_empty(), "control characters on screen (source_view={source_view}): {bad:?}");
    }

    // The visible text survives, and tabs in <pre> become spaces
    app.current_tab().is_source_view = false;
    app.render_tab(0, 100);
    let text: Vec<String> = app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect();
    assert!(text.iter().any(|l| l.contains("safe ]52;c;ZWNobyBoaQ==[2J31mtext")), "{text:?}");
    assert!(text.iter().any(|l| l.contains("col1    col2")), "{text:?}");
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

#[tokio::test]
async fn test_compressed_page_cannot_exceed_size_limit() {
    // ~20MB of HTML compresses to a few KB and arrives without a usable Content-Length
    let huge = format!("<p>{}</p>", "a ".repeat(10 * 1024 * 1024));
    let compressed = gzip(huge.as_bytes());
    assert!(compressed.len() < 100_000);

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/bomb"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Encoding", "gzip")
                .set_body_raw(compressed, "text/html"),
        )
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/bomb", mock_server.uri())).await;
    let NetworkResponse::Error(_, msg) = resp else { panic!("oversized page was accepted") };
    assert!(msg.contains("too large"), "{msg}");
}

#[tokio::test]
async fn test_compressed_page_under_limit_still_loads() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/small"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("Content-Encoding", "gzip")
                .set_body_raw(gzip(b"<title>Small</title><p>compressed ok</p>"), "text/html"),
        )
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/small", mock_server.uri())).await;
    let NetworkResponse::Success(_, _, ref body, _, _) = resp else { panic!("small page failed") };
    assert!(body.contains("compressed ok"));
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();
    assert_eq!(app.current_tab().page_title, "Small");
}

#[tokio::test]
async fn test_page_charset_is_honoured() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/latin1"))
        .respond_with(
            // "café" in windows-1252, where é is the single byte 0xE9
            ResponseTemplate::new(200)
                .set_body_raw(b"<p>caf\xe9</p>".to_vec(), "text/html; charset=windows-1252"),
        )
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/latin1", mock_server.uri())).await;
    let NetworkResponse::Success(_, _, body, _, _) = resp else { panic!("page failed") };
    assert!(body.contains("café"), "{body}");
}

#[tokio::test]
async fn test_non_web_links_are_refused() {
    let links = [
        ("mailto:someone@example.com", "mailto"),
        ("javascript:alert(document.cookie)", "javascript"),
        ("file:///etc/passwd", "file"),
        ("data:text/html,<p>hi</p>", "data"),
    ];
    let html: String = links.iter().map(|(href, _)| format!("<p><a href=\"{href}\">x</a></p>")).collect();
    let mut app = test_app();
    load_page(&mut app, &html);

    for (i, (_, scheme)) in links.iter().enumerate() {
        app.current_tab().selected_link_index = i;
        for code in [KeyCode::Enter, KeyCode::Char('t'), KeyCode::Char('d')] {
            handle_key_event::<TestBackend>(&mut app, key(code), 80, 24).unwrap();
            let tab = app.current_tab();
            assert_eq!(tab.status_message, format!("Unsupported link type: {scheme}"), "{code:?}");
            tab.status_message.clear();
        }
    }

    let tab = app.current_tab();
    assert_eq!(tab.url_input, TEST_PAGE_URL);
    assert!(tab.history.is_empty());
    assert!(tab.download_prompt.is_none());
    assert_eq!(app.tabs.len(), 1, "a new tab was opened");
    assert!(!made_request(&mut app).await);
}

#[tokio::test]
async fn test_i2p_sites_are_not_requested_outside_i2p_mode() {
    let mut app = test_app();
    assert!(!app.i2p_mode);
    let resp = load_url(&mut app, "http://secret-site.i2p/").await;
    let NetworkResponse::Error(_, msg) = resp else { panic!(".i2p site was requested") };
    assert!(msg.contains("Enable I2P mode"), "{msg}");

    // A clearweb page can't send the browser there by redirect either
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/to-i2p"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "http://secret-site.i2p/"))
        .mount(&mock_server)
        .await;
    let resp = load_url(&mut app, &format!("{}/to-i2p", mock_server.uri())).await;
    let NetworkResponse::Error(_, msg) = resp else { panic!("redirect to .i2p was followed") };
    assert!(msg.contains("Enable I2P mode"), "{msg}");
}

fn screen_text(terminal: &Terminal<TestBackend>) -> String {
    screen_cells(terminal).concat()
}

#[tokio::test]
async fn test_selected_link_destination_is_shown() {
    let mut app = test_app();
    // Link text claims one site, href goes to another; the second link is relative
    load_page(
        &mut app,
        "<p><a href='https://evil.example/login'>https://paypal.com</a> \
         <a href='docs/page.html'>docs</a> \
         <a href='https://\u{0430}pple.com/'>apple</a></p>",
    );
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    terminal.draw(|f| ui(f, &app)).unwrap();
    assert!(screen_text(&terminal).contains("Link: https://evil.example/login"));

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Tab), 100, 24).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    assert!(screen_text(&terminal).contains("Link: http://127.0.0.1:1/docs/page.html"));

    // A Cyrillic "а" look-alike is shown as punycode, not as "apple.com"
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Tab), 100, 24).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    assert!(screen_text(&terminal).contains("Link: https://xn--pple-43d.com/"), "{}", screen_text(&terminal));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn test_downloads_are_quarantined_for_gatekeeper() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tool.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PK\x03\x04zip".to_vec()))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let dir = app.download_dir.clone();
    start_download(&mut app, &format!("{}/tool.zip", mock_server.uri()));
    let (resp, _) = finish_download(&mut app).await;
    let NetworkResponse::DownloadFinished(_, saved) = resp else { panic!("download failed") };

    let output = std::process::Command::new("/usr/bin/xattr")
        .args(["-p", "com.apple.quarantine", &saved])
        .output()
        .unwrap();
    let value = String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "no quarantine attribute on {saved}");
    assert!(value.starts_with("0081;") && value.contains(";rynx_browser;"), "{value}");
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn test_downloads_record_where_they_came_from() {
    let mut app = test_app();
    let dir = app.download_dir.clone();
    let (saved, origin) = download_sample(&mut app, "tool.zip").await;

    // Decode the attribute with macOS's own plist parser
    let raw = std::process::Command::new("/usr/bin/xattr")
        .args(["-px", "com.apple.metadata:kMDItemWhereFroms", &saved])
        .output()
        .unwrap();
    assert!(raw.status.success(), "no kMDItemWhereFroms attribute");
    let hex: String = String::from_utf8_lossy(&raw.stdout).split_whitespace().collect();
    let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
    let plist_file = dir.join("where-from.plist");
    std::fs::write(&plist_file, &bytes).unwrap();
    let parsed = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&plist_file)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(parsed.status.success(), "plutil rejected the plist: {}", String::from_utf8_lossy(&parsed.stderr));
    let json = String::from_utf8_lossy(&parsed.stdout);
    assert_eq!(json.trim(), format!("[\"{}\"]", origin.replace('/', "\\/")));
}

/// Render on another thread so a regression into an infinite loop fails instead of hanging
fn render_within_time_limit(html: String, width: usize) -> Vec<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let document = scraper::Html::parse_document(&html);
        let mut renderer = rynx_browser::renderer::DomRenderer::new(width);
        renderer.render(&document);
        let _ = tx.send(renderer.lines.iter().map(|l| l.to_string()).collect());
    });
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_else(|_| panic!("rendering at width {width} did not finish (infinite loop?)"))
}

#[test]
fn test_deeply_nested_lists_do_not_hang_renderer() {
    // ~200 bytes of HTML used to freeze the browser at a normal terminal width
    let html = format!("{}<li>deep item{}", "<ul>".repeat(60), "</ul>".repeat(60));
    let lines = render_within_time_limit(html, 80);
    assert!(lines.iter().any(|l| l.contains("deep item")), "{lines:?}");
    // Indentation is capped so the text stays on screen
    assert!(lines.iter().all(|l| unicode_width::UnicodeWidthStr::width(l.as_str()) <= 80 - 2 + 1));
}

#[test]
fn test_zero_width_content_area_does_not_hang_renderer() {
    // A terminal 2 columns wide leaves 0 columns for content
    for width in 0..=3 {
        let lines = render_within_time_limit("<p>hello world</p><ul><li>x</li></ul>".into(), width);
        assert!(lines.concat().contains("hello"), "width {width}: {lines:?}");
    }
}

#[test]
fn test_zero_width_characters_do_not_create_empty_link_regions() {
    let document = scraper::Html::parse_document("<a href='/x'>\u{200b}</a><a href='/y'>e\u{301}</a>");
    let mut renderer = rynx_browser::renderer::DomRenderer::new(80);
    renderer.render(&document);
    assert!(renderer.links.iter().all(|l| l.x_start < l.x_end));
    assert!(renderer.links.iter().any(|l| l.url == "/y"));
}

#[tokio::test]
async fn test_error_messages_cannot_inject_markup() {
    let mut app = test_app();
    let id = app.current_tab().id;
    let msg = "failed <a href='https://evil.example/'>click here</a> & <b>more</b>";
    handle_network_event::<TestBackend>(&mut app, NetworkResponse::Error(id, msg.into()), 80).unwrap();

    let tab = app.current_tab();
    assert!(tab.link_regions.is_empty(), "error text became a link");
    let text: String = tab.rendered_content.iter().map(|l| l.to_string()).collect();
    assert!(text.contains("failed <a href='https://evil.example/'>click here</a> & <b>more</b>"), "{text}");
}

#[test]
fn test_rendered_text_is_stored_compactly() {
    // A span per character made a 10MB page use ~700MB; same-styled runs must share spans
    let html = "<p>plain text paragraph with <b>some bold</b> and a <a href='/x'>link</a></p>".repeat(1000);
    let document = scraper::Html::parse_document(&html);
    let mut renderer = rynx_browser::renderer::DomRenderer::new(80);
    renderer.render(&document);
    let spans: usize = renderer.lines.iter().map(|l| l.spans.len()).sum();
    let chars: usize = renderer.lines.iter().map(|l| l.to_string().chars().count()).sum();
    assert!(spans * 10 < chars, "{spans} spans for {chars} characters");
}

#[test]
fn test_link_regions_match_drawn_width_for_joined_emoji() {
    // A ZWJ family emoji is width 2 as one string but drawn as 6 cells character by character
    let document = scraper::Html::parse_document("<a href='/f'>family \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}</a>");
    let mut renderer = rynx_browser::renderer::DomRenderer::new(80);
    renderer.render(&document);
    let link = &renderer.links[0];
    assert!(link.x_end <= rynx_browser::renderer::display_width(&renderer.lines[link.line_index]));
}

#[tokio::test]
async fn test_requests_send_no_referer_header() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/page"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>ok</p>", "text/html"))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/page", mock_server.uri())).await;
    assert!(matches!(resp, NetworkResponse::Success(..)));
    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        requests.iter().all(|r| !r.headers.contains_key("referer")),
        "a Referer header was sent"
    );
}


#[test]
fn test_content_types_are_classified() {
    use rynx_browser::network::{ContentClass, classify_content_type};

    for html in [None, Some("text/html"), Some("text/html; charset=utf-8"), Some("application/xhtml+xml")] {
        assert_eq!(classify_content_type(html), ContentClass::Html, "{html:?}");
    }
    for text in [
        "text/plain", "text/markdown", "text/css; charset=utf-8", "application/json",
        "application/ld+json", "application/rss+xml", "application/javascript", "TEXT/PLAIN",
    ] {
        assert_eq!(classify_content_type(Some(text)), ContentClass::Text, "{text}");
    }
    for binary in ["image/png", "application/pdf", "application/octet-stream", "video/mp4"] {
        assert_eq!(classify_content_type(Some(binary)), ContentClass::Binary(binary.to_string()));
    }
}

#[tokio::test]
async fn test_plain_text_pages_are_shown_as_text() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/notes.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<b>not markup</b>\n\n  indented\nSee https://example.com/docs.",
            "text/plain; charset=utf-8",
        ))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let resp = load_url(&mut app, &format!("{}/notes.txt", mock_server.uri())).await;
    let NetworkResponse::Success(_, ref title, _, _, kind) = resp else { panic!("load failed") };
    assert_eq!(kind, PageKind::PlainText);
    assert_eq!(title, "notes.txt");
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();

    let tab = app.current_tab();
    let lines: Vec<String> = tab.rendered_content.iter().map(|l| l.to_string()).collect();
    assert_eq!(lines, ["<b>not markup</b>", "", "  indented", "See https://example.com/docs."]);
    assert_eq!(tab.link_regions.len(), 1);
    assert_eq!(tab.link_regions[0].url, "https://example.com/docs");

    // Source view and back keep it as text
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('V')), 80, 24).unwrap();
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('V')), 80, 24).unwrap();
    assert_eq!(app.current_tab().rendered_content[0].to_string(), "<b>not markup</b>");
}

#[tokio::test]
async fn test_binary_files_offer_a_download_instead_of_garbage() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/photo.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG\r\n\x1a\nbinary".to_vec(), "image/png"))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let url = format!("{}/photo.png", mock_server.uri());
    let resp = load_url(&mut app, &url).await;
    handle_network_event::<TestBackend>(&mut app, resp, 80).unwrap();

    let tab = app.current_tab();
    assert_eq!(tab.page_title, "photo.png");
    let text: String = tab.rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("image/png"), "{text}");
    assert!(!text.contains("PNG"), "binary content was rendered");
    assert_eq!(tab.link_regions.len(), 1);
    assert_eq!(tab.link_regions[0].url, url);

    // The link downloads the file
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('d')), 80, 24).unwrap();
    assert_eq!(app.current_tab().download_prompt.as_ref().map(|p| p.filename.as_str()), Some("photo.png"));
}

#[tokio::test]
async fn test_pages_use_the_full_content_width() {
    // An 80-column terminal leaves 78 columns inside the page's border
    let mut app = test_app();
    load_page(&mut app, &format!("<p>{}</p><hr>", "word ".repeat(200)));
    let widths: Vec<usize> = app
        .current_tab()
        .rendered_content
        .iter()
        .map(rynx_browser::renderer::display_width)
        .collect();
    assert_eq!(widths.iter().max(), Some(&78), "{widths:?}");

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    // The horizontal rule reaches the right border without being cut off
    let screen = screen_text(&terminal);
    assert!(screen.contains(&format!("│{}│", "─".repeat(78))), "rule doesn't span the content area");
}

#[test]
fn test_download_filenames_are_safe_on_every_platform() {
    let long_stem = "a".repeat(300);
    let cases: Vec<(String, String)> = vec![
        // Windows device names, with or without an extension, in any case
        ("https://x.com/CON".into(), "_CON".into()),
        ("https://x.com/nul.txt".into(), "_nul.txt".into()),
        ("https://x.com/com1.tar.gz".into(), "_com1.tar.gz".into()),
        ("https://x.com/LPT9".into(), "_LPT9".into()),
        ("https://x.com/COM%C2%B9".into(), "_COM¹".into()),
        // Similar names that aren't devices
        ("https://x.com/CONSOLE.txt".into(), "CONSOLE.txt".into()),
        ("https://x.com/COM10".into(), "COM10".into()),
        // No hidden files, no trailing dots
        ("https://x.com/.bashrc".into(), "bashrc".into()),
        ("https://x.com/report.".into(), "report".into()),
        ("https://x.com/...".into(), "download.dat".into()),
    ];
    for (url, expected) in cases {
        assert_eq!(App::download_filename(&url), expected, "for {url}");
    }
    let long = App::download_filename(&format!("https://x.com/{long_stem}.pdf"));
    assert!(long.len() <= 200 && long.ends_with(".pdf"), "{long}");
}

#[cfg(unix)]
#[tokio::test]
async fn test_downloads_never_write_through_symlinks() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/victim.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"attacker content".to_vec()))
        .mount(&mock_server)
        .await;

    let mut app = test_app();
    let dir = app.download_dir.clone();
    std::fs::create_dir_all(&dir).unwrap();
    // A link planted in Downloads, pointing at a file elsewhere (e.g. ~/.bashrc)
    let target = dir.with_extension("target");
    std::fs::write(&target, b"precious").unwrap();
    std::os::unix::fs::symlink(&target, dir.join("victim.txt")).unwrap();

    let url = format!("{}/victim.txt", mock_server.uri());
    app.current_tab().initiate_download_request(url, &dir);
    let prompt = app.current_tab().download_prompt.take().unwrap();
    assert!(prompt.file_exists, "the link should count as an existing file");
    app.trigger_download(prompt);
    let (resp, _) = finish_download(&mut app).await;

    let NetworkResponse::DownloadError(_, msg) = resp else { panic!("wrote through the symlink") };
    assert!(msg.contains("symbolic link"), "{msg}");
    assert_eq!(std::fs::read(&target).unwrap(), b"precious");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&target);
}

/// Download a small file and return where it was saved
async fn download_sample(app: &mut App, name: &str) -> (String, String) {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/{name}")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"PK\x03\x04zip".to_vec()))
        .mount(&mock_server)
        .await;
    // Credentials and fragments must not be recorded on the file
    let address = mock_server.address();
    let url = format!("http://user:secret@{address}/{name}#frag");
    start_download(app, &url);
    let (resp, _) = finish_download(app).await;
    let NetworkResponse::DownloadFinished(_, saved) = resp else { panic!("download failed") };
    (saved, format!("http://{address}/{name}"))
}

#[cfg(windows)]
#[tokio::test]
async fn test_downloads_get_the_mark_of_the_web() {
    let mut app = test_app();
    let (saved, origin) = download_sample(&mut app, "tool.zip").await;
    let mark = std::fs::read_to_string(format!("{saved}:Zone.Identifier")).expect("no Zone.Identifier");
    assert!(mark.contains("ZoneId=3"), "{mark}");
    assert!(mark.contains(&format!("HostUrl={origin}")), "{mark}");
    assert!(!mark.contains("secret"), "{mark}");
}

#[cfg(all(unix, not(target_os = "macos")))]
#[tokio::test]
async fn test_downloads_record_their_origin() {
    let mut app = test_app();
    // Not every filesystem stores user extended attributes (e.g. some tmpfs); skip there
    let probe = std::env::temp_dir().join(format!("rynx-xattr-probe-{}", std::process::id()));
    std::fs::write(&probe, b"").unwrap();
    let supported = xattr::set(&probe, "user.rynx.probe", b"1").is_ok();
    let _ = std::fs::remove_file(&probe);
    if !supported {
        eprintln!("skipping: user extended attributes unsupported on the temp filesystem");
        return;
    }
    let (saved, origin) = download_sample(&mut app, "tool.zip").await;
    let recorded = xattr::get(&saved, "user.xdg.origin.url").unwrap().expect("no origin attribute");
    assert_eq!(String::from_utf8(recorded).unwrap(), origin);
}


/// Load a page at a URL the way a finished network request would
fn arrive(app: &mut App, url: &str, html: &str) {
    let id = app.current_tab().id;
    handle_network_event::<TestBackend>(
        app,
        NetworkResponse::Success(id, "Page".into(), html.into(), url.into(), PageKind::Html),
        80,
    )
    .unwrap();
}

fn long_page(label: &str) -> String {
    (0..100).map(|i| format!("<p>{label} line {i}</p>")).collect()
}

#[tokio::test]
async fn test_back_and_forward_restore_scroll_positions() {
    let mut app = test_app();
    let first = "http://127.0.0.1:1/first";
    let second = "http://127.0.0.1:1/second";
    app.current_tab().url_input = first.into();
    arrive(&mut app, first, &format!("{}<a href='/second'>next</a>", long_page("first")));
    app.current_tab().scroll = 40;
    app.current_tab().cursor_line = 45;

    // Follow a link, scroll the second page, then go back
    app.follow_link(second.into());
    arrive(&mut app, second, &long_page("second"));
    assert_eq!(app.current_tab().scroll, 0);
    app.current_tab().scroll = 12;

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Backspace), 80, 24).unwrap();
    assert_eq!(app.current_tab().url_input, first);
    arrive(&mut app, first, &long_page("first"));
    assert_eq!((app.current_tab().scroll, app.current_tab().cursor_line), (40, 45));
    assert_eq!(app.current_tab().forward_history.len(), 1);

    // Forward returns to the second page where it was left
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Right), 80, 24).unwrap();
    assert_eq!(app.current_tab().url_input, second);
    arrive(&mut app, second, &long_page("second"));
    assert_eq!(app.current_tab().scroll, 12);

    // Following a new link clears the forward history
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('H')), 80, 24).unwrap();
    arrive(&mut app, first, &long_page("first"));
    app.follow_link("http://127.0.0.1:1/third".into());
    assert!(app.current_tab().forward_history.is_empty());
}

#[tokio::test]
async fn test_back_within_a_page_restores_position_without_reloading() {
    let mut app = test_app();
    app.current_tab().url_input = TEST_PAGE_URL.into();
    arrive(&mut app, TEST_PAGE_URL, &book_page());
    app.current_tab().scroll = 7;
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Enter), 80, 24).unwrap(); // #ch2
    assert!(app.current_tab().scroll > 7);

    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Backspace), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, 7);
    assert!(!made_request(&mut app).await);
}

#[tokio::test]
async fn test_reload_keeps_the_scroll_position() {
    let mut app = test_app();
    app.current_tab().url_input = TEST_PAGE_URL.into();
    arrive(&mut app, TEST_PAGE_URL, &long_page("page"));
    app.current_tab().scroll = 30;
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('r')), 80, 24).unwrap();
    assert!(made_request(&mut app).await, "reload didn't fetch the page");
    arrive(&mut app, TEST_PAGE_URL, &long_page("page"));
    assert_eq!(app.current_tab().scroll, 30);
}

#[tokio::test]
async fn test_paging_keys() {
    let mut app = test_app();
    arrive(&mut app, TEST_PAGE_URL, &long_page("page"));
    let total = app.current_tab().rendered_content.len();
    let ctrl = |c| KeyEvent { code: KeyCode::Char(c), modifiers: KeyModifiers::CONTROL, kind: KeyEventKind::Press, state: KeyEventState::empty() };
    // 24 rows leaves 16 visible
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::PageDown), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, 16);
    handle_key_event::<TestBackend>(&mut app, ctrl('d'), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, 24);
    assert!(app.current_tab().download_prompt.is_none(), "Ctrl+d must not start a download");
    handle_key_event::<TestBackend>(&mut app, ctrl('u'), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, 16);
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('G')), 80, 24).unwrap();
    assert_eq!(app.current_tab().scroll, total - 16);
    assert_eq!(app.current_tab().cursor_line, total - 1);
    handle_key_event::<TestBackend>(&mut app, key(KeyCode::Char('g')), 80, 24).unwrap();
    assert_eq!((app.current_tab().scroll, app.current_tab().cursor_line), (0, 0));
}

#[tokio::test]
async fn test_only_binary_file_links_skip_straight_to_downloading() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use rynx_browser::event_handler::handle_mouse_event;

    let click = |row| MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 2, row, modifiers: KeyModifiers::empty() };
    for (href, prompts) in [
        ("/files/report.zip", true),
        ("/files/index.html", false), // /files/ is an ordinary page path on many sites
        ("/download/", false),
        ("/notes.txt", false),         // shown as text now
        ("/page?file=a.zip", false),   // the query doesn't make it a download
    ] {
        let mut app = test_app();
        arrive(&mut app, TEST_PAGE_URL, &format!("<a href='{href}'>link</a>"));
        // The page's first line is drawn at row 7 (below the tab bar and address bar)
        handle_mouse_event::<TestBackend>(&mut app, click(7), 80, 24).unwrap();
        assert_eq!(app.current_tab().download_prompt.is_some(), prompts, "{href}");
    }
}

#[tokio::test]
async fn test_find_on_page_is_smart_case() {
    let mut app = test_app();
    load_page(&mut app, "<p>Rust and rust and RUST</p>");
    app.current_tab().perform_search("rust");
    assert_eq!(app.current_tab().search_state.as_ref().unwrap().matches.len(), 3);
    // A capital letter makes the search exact
    app.current_tab().perform_search("Rust");
    assert_eq!(app.current_tab().search_state.as_ref().unwrap().matches.len(), 1);
}

#[tokio::test]
async fn test_find_highlights_line_up_after_wide_characters() {
    let mut app = test_app();
    // Each of the two CJK characters is two columns wide
    load_page(&mut app, "<p>漢字 target</p>");
    app.current_tab().perform_search("target");
    let m = &app.current_tab().search_state.as_ref().unwrap().matches[0];
    assert_eq!((m.start_char, m.end_char), (5, 11));

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    // The page starts at column 1 (after the border), row 7
    let buffer = terminal.backend().buffer();
    let highlighted: String = (0..80)
        .map(|x| &buffer[(x, 7)])
        .filter(|cell| cell.bg == ratatui::style::Color::Green)
        .map(|cell| cell.symbol())
        .collect();
    assert_eq!(highlighted, "target");
}
