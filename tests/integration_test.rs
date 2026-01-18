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
