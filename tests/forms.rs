//! HTML forms: rendering controls, entering values, and submitting.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use rynx::app::App;
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::forms::{ControlKind, ControlValue, FormMethod, build_submission};
use rynx::models::InputMode;
use rynx::network::{NetworkResponse, PageKind};
use rynx::renderer::{RenderJob, render_page};
use rynx::ui::ui;
use std::sync::Arc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PAGE: &str = "https://example.com/dir/page.html";

fn render(html: &str) -> rynx::renderer::RenderedPage {
    render_page(&RenderJob {
        source: Arc::from(html),
        kind: PageKind::Html,
        source_view: false,
        reader: false,
        width: 82,
    })
}

fn text(page: &rynx::renderer::RenderedPage) -> Vec<String> {
    page.lines.iter().map(|l| l.to_string()).filter(|l| !l.trim().is_empty()).collect()
}

/// Submit form 0 of `html` with the initial values changed by `edit`
fn submit(
    html: &str,
    submitter: Option<usize>,
    edit: impl FnOnce(&mut Vec<ControlValue>),
) -> Result<rynx::forms::Submission, String> {
    let page = render(html);
    let mut values: Vec<ControlValue> = page.controls.iter().map(|c| c.initial_value()).collect();
    edit(&mut values);
    build_submission(&page.forms, &page.controls, &values, 0, submitter, PAGE)
}

#[test]
fn controls_are_drawn_as_boxes() {
    let page = render(
        "<form><input name=q size=10 placeholder='Search'> <input type=submit value=Go>\
         <br><input type=checkbox name=a checked> A <input type=radio name=r> R\
         <br><select name=s><option>One</option><option selected>Two</option></select>\
         <input type=hidden name=h value=secret></form>",
    );
    assert_eq!(text(&page), ["[Search____] [ Go ]", "[x] A ( ) R", "[Two ▾]"]);
    // Every visible control is selectable like a link; the hidden one isn't drawn
    assert_eq!(page.links.iter().filter(|l| l.control.is_some()).count(), 5);
    assert_eq!(page.controls.len(), 6);
    assert_eq!(page.controls[5].kind, ControlKind::Hidden);
}

#[test]
fn get_forms_put_their_data_in_the_address() {
    let html = "<form action='/search'><input name=q><input type=hidden name=lang value=en>\
                <input type=checkbox name=safe value=1 checked><input type=checkbox name=x></form>";
    let submission = submit(html, None, |v| v[0] = ControlValue::Text("rust & tui".into())).unwrap();
    assert_eq!(submission.method, FormMethod::Get);
    assert_eq!(submission.url, "https://example.com/search?q=rust+%26+tui&lang=en&safe=1");
    assert_eq!(submission.body, None);
}

#[test]
fn post_forms_send_a_body() {
    let html = "<form method=POST action=submit><textarea name=msg></textarea>\
                <select name=pick><option value=a>A</option><option value=b>B</option></select>\
                <button name=act value=send>Send</button><button name=act value=draft>Draft</button></form>";
    let submission = submit(html, Some(3), |v| {
        v[0] = ControlValue::Text("line 1\nline 2".into());
        v[1] = ControlValue::Selected(1);
    })
    .unwrap();
    assert_eq!(submission.method, FormMethod::Post);
    assert_eq!(submission.url, "https://example.com/dir/submit");
    // Only the pressed button is sent; textarea line breaks are CRLF
    assert_eq!(submission.body.as_deref(), Some("msg=line+1%0D%0Aline+2&pick=b&act=draft"));
}

#[test]
fn radio_buttons_send_the_chosen_option() {
    let html = "<form><input type=radio name=size value=s checked><input type=radio name=size value=l></form>";
    let submission = submit(html, None, |v| {
        v[0] = ControlValue::Checked(false);
        v[1] = ControlValue::Checked(true);
    })
    .unwrap();
    assert!(submission.url.ends_with("?size=l"), "{}", submission.url);
}

#[test]
fn a_form_without_action_submits_to_its_own_page() {
    let submission = submit("<form><input name=q value=x></form>", None, |_| {}).unwrap();
    assert_eq!(submission.url, "https://example.com/dir/page.html?q=x");
}

#[test]
fn passwords_are_never_put_in_the_address() {
    let html = "<form><input name=user><input type=password name=pass></form>";
    let err = submit(html, None, |v| v[1] = ControlValue::Text("hunter2".into())).unwrap_err();
    assert!(err.contains("password"), "{err}");
    // Sent in a POST body it's fine
    let html = "<form method=post><input name=user><input type=password name=pass></form>";
    assert!(submit(html, None, |v| v[1] = ControlValue::Text("hunter2".into())).is_ok());
}

#[test]
fn forms_cannot_target_non_web_addresses() {
    for action in ["javascript:alert(1)", "mailto:a@b.c", "file:///etc/passwd"] {
        let html = format!("<form action='{action}'><input name=q></form>");
        assert!(submit(&html, None, |_| {}).is_err(), "{action}");
    }
}

#[test]
fn disabled_controls_are_shown_but_not_sent() {
    let page = render("<form><input name=a value=1 disabled><input name=b value=2></form>");
    assert_eq!(page.controls.len(), 1);
    let submission =
        submit("<form><input name=a value=1 disabled><input name=b value=2></form>", None, |_| {}).unwrap();
    assert!(submission.url.ends_with("?b=2"), "{}", submission.url);
}

#[test]
fn controls_can_join_a_form_by_id() {
    let html = "<form id=f action=/go></form><input form=f name=q value=outside>";
    let submission = submit(html, None, |_| {}).unwrap();
    assert_eq!(submission.url, "https://example.com/go?q=outside");
}

#[test]
fn forms_inside_tables_work() {
    let page = render("<table><tr><td>Search:</td><td><form action=/s><input name=q size=8></form></td></tr></table>");
    assert_eq!(text(&page), ["Search: │ [________]"]);
    let region = page.links.iter().find(|l| l.control.is_some()).unwrap();
    assert_eq!(page.controls[region.control.unwrap()].form, Some(0));
}

// --- In the browser ---

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() }
}

fn press(app: &mut App, code: KeyCode) {
    handle_key_event(app, key(code), 80, 24).unwrap();
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

fn arrive(app: &mut App, url: &str, html: &str) {
    let id = app.current_tab().id;
    app.current_tab().url_input = url.to_string();
    let resp = NetworkResponse::Success(id, "Page".into(), html.into(), url.into(), PageKind::Html);
    handle_network_event(app, resp, 80).unwrap();
}

/// Handle network messages until a page has loaded
async fn wait_for_page(app: &mut App) {
    loop {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv()).await.unwrap().unwrap();
        let done = matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(app, resp, 80).unwrap();
        if done {
            return;
        }
    }
}

#[tokio::test]
async fn typing_a_search_and_pressing_enter_submits_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>results</p>", "text/html"))
        .mount(&server)
        .await;

    let mut app = test_app();
    let page = format!("{}/", server.uri());
    arrive(&mut app, &page, "<form action=/search><input name=q> <button>Search</button></form>");

    // The field is the first selectable item
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.current_tab().input_mode, InputMode::FormEdit);
    type_text(&mut app, "rust tui");
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Char('i'));
    press(&mut app, KeyCode::Enter);
    wait_for_page(&mut app).await;

    let requests = server.received_requests().await.unwrap();
    let search = requests.iter().find(|r| r.url.path() == "/search").expect("form not submitted");
    assert_eq!(search.url.query(), Some("q=rust+tui"));
    assert!(app.current_tab().rendered_content.iter().any(|l| l.to_string().contains("results")));
    // Back returns to the form
    assert_eq!(app.current_tab().history.last().map(|e| e.url.as_str()), Some(page.as_str()));
}

#[tokio::test]
async fn post_forms_are_sent_and_redirects_become_get() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/comment"))
        .respond_with(ResponseTemplate::new(303).insert_header("Location", "/thanks"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/thanks"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<p>thanks</p>", "text/html"))
        .mount(&server)
        .await;

    let mut app = test_app();
    arrive(
        &mut app,
        &format!("{}/", server.uri()),
        "<form method=post action=/comment><input name=text><input type=submit value=Post></form>",
    );
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "hi there");
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.current_tab().input_mode, InputMode::Normal);
    // Tab to the button and press it
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    wait_for_page(&mut app).await;

    let requests = server.received_requests().await.unwrap();
    let post = requests.iter().find(|r| r.method.as_str() == "POST").expect("no POST");
    assert_eq!(String::from_utf8_lossy(&post.body), "text=hi+there");
    assert_eq!(
        post.headers.get("content-type").and_then(|v| v.to_str().ok()),
        Some("application/x-www-form-urlencoded")
    );
    assert!(requests.iter().any(|r| r.method.as_str() == "GET" && r.url.path() == "/thanks"));
    assert!(app.current_tab().url_input.ends_with("/thanks"));
}

#[tokio::test]
async fn typed_values_are_shown_and_passwords_masked() {
    let mut app = test_app();
    arrive(&mut app, PAGE, "<form method=post><input name=user size=12><input type=password name=pw size=12></form>");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "ray");
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "secret");

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("[ray_________]"), "typed text not shown");
    assert!(screen.contains("[••••••▏_____]"), "password not masked while typing");
    assert!(!screen.contains("secret"));
    // The status bar says what the field is and where the form sends it
    assert!(screen.contains("Password field \"pw\" → sends to (POST) https://example.com/dir/page.html"), "{screen}");
}

#[tokio::test]
async fn checkboxes_radios_and_choices_respond_to_enter() {
    let mut app = test_app();
    arrive(
        &mut app,
        PAGE,
        "<form><input type=checkbox name=c><input type=radio name=r value=1 checked><input type=radio name=r value=2>\
                            <select name=s><option>a</option><option>b</option></select></form>",
    );
    press(&mut app, KeyCode::Enter); // checkbox on
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter); // second radio
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Enter); // next option
    assert_eq!(
        app.current_tab().control_values,
        [
            ControlValue::Checked(true),
            ControlValue::Checked(false),
            ControlValue::Checked(true),
            ControlValue::Selected(1)
        ]
    );
}

#[tokio::test]
async fn controls_are_never_downloaded_or_opened_in_tabs() {
    let mut app = test_app();
    arrive(&mut app, PAGE, "<form><input name=q></form>");
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Char('t'));
    assert!(app.current_tab().download_prompt.is_none());
    assert_eq!(app.tabs.len(), 1);
}

#[test]
fn controls_outside_forms_are_inert() {
    // Only scripts could use these, so they shouldn't be Tab stops
    let page = render("<button>Hide</button> <input type=checkbox> <form><input name=q></form>");
    assert_eq!(page.controls.len(), 1);
    assert_eq!(page.links.iter().filter(|l| l.control.is_some()).count(), 1);
    assert!(text(&page)[0].contains("Hide"));
}

// --- Protections ---

fn submit_from(page_url: &str, action: &str) -> Result<rynx::forms::Submission, String> {
    let page = render(&format!("<form action='{action}' method=post><input name=q value=x></form>"));
    let values: Vec<ControlValue> = page.controls.iter().map(|c| c.initial_value()).collect();
    build_submission(&page.forms, &page.controls, &values, 0, None, page_url)
}

#[test]
fn public_pages_cannot_submit_forms_to_the_local_network() {
    for action in [
        "http://192.168.1.1/apply.cgi",
        "http://10.0.0.5/admin",
        "http://localhost:8080/api",
        "http://127.0.0.1/",
        "http://[::1]/",
        "http://169.254.169.254/latest",
    ] {
        let err = submit_from("https://evil.example/page", action).unwrap_err();
        assert!(err.contains("local network"), "{action}: {err}");
    }
    // A page on the local network may submit to itself or its neighbours
    assert!(submit_from("http://localhost:8000/", "/save").is_ok());
    assert!(submit_from("http://192.168.1.1/", "http://192.168.1.1/apply").is_ok());
    // Public to public is fine
    assert!(submit_from("https://a.example/", "https://b.example/search").is_ok());
}

#[test]
fn forms_whose_address_hides_its_real_site_are_refused() {
    let err = submit_from("https://a.example/", "https://paypal.com@evil.example/login").unwrap_err();
    assert!(err.contains("evil.example"), "{err}");
}

#[tokio::test]
async fn links_that_hide_their_real_site_are_blocked_and_flagged() {
    let mut app = test_app();
    arrive(&mut app, PAGE, "<a href='https://paypal.com@evil.example/login'>PayPal login</a>");

    // The preview warns before anything is followed
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("really goes to evil.example"), "no warning in preview");

    for code in [KeyCode::Enter, KeyCode::Char('t')] {
        press(&mut app, code);
        assert!(app.current_tab().status_message.contains("evil.example"), "{code:?}");
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.current_tab().url_input, PAGE);
    }
}

#[tokio::test]
async fn typed_addresses_with_credentials_still_work() {
    // The user chose this address; only links from pages are blocked
    let mut app = test_app();
    app.current_tab().url_input = "https://user@intranet.example/".into();
    app.submit_request();
    assert_eq!(app.current_tab().url_input, "https://user@intranet.example/");
}

#[tokio::test]
async fn typed_passwords_are_hidden_and_never_bookmarked() {
    let mut app = test_app();
    let url = "https://ray:hunter2@intranet.example/wiki";
    arrive(&mut app, url, "<title>Wiki</title><p>page</p>");

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal.draw(|f| ui(f, &app)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(!screen.contains("hunter2"), "password shown in the address bar");
    assert!(screen.contains("https://ray:••••@intranet.example/wiki"));

    press(&mut app, KeyCode::Char('b'));
    assert_eq!(app.bookmarks.entries[0].url, "https://intranet.example/wiki");
}
