use rynx_browser::app::App;
use rynx_browser::constants::{DEFAULT_TAB_INDEX, INITIAL_TAB_ID};
use rynx_browser::event_handler::handle_key_event;
use rynx_browser::event_handler::handle_network_event;
use rynx_browser::network::NetworkResponse;
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

    if let Some(NetworkResponse::Success(id, title, body)) = final_response {
        assert_eq!(title, "Test Page");

        // Use the actual terminal width constant or a test value
        let test_width = 80;
        handle_network_event::<TestBackend>(
            &mut app,
            NetworkResponse::Success(id, title, body),
            test_width,
        )
        .unwrap();

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

fn test_app() -> App {
    let (tx, rx) = mpsc::channel(10);
    let mut app = App::new(tx, rx).expect("Failed to create App");
    // Never let a test write into the real Downloads folder
    app.download_dir = unique_temp_dir();
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
        NetworkResponse::Success(id, "Page".to_string(), html.to_string()),
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
    // Source view of the help tab has no html_source, so there are no lines at all
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

    assert_eq!(app.current_tab().history, vec!["http://127.0.0.1:1/first"]);
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
    let client = reqwest::Client::new();
    let resp = attempt_jump_with(&client, &services, "example.i2p", tx, 0)
        .await
        .expect("should fall through to the working service");
    assert_eq!(resp.text().await.unwrap(), "<p>found</p>");
}
