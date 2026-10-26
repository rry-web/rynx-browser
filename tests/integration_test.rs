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
    App::new(tx, rx).expect("Failed to create App")
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
