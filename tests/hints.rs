//! Link hints (f and ;): the links on screen numbered, and typing a number opens one. What
//! voice control will say ("follow 12"), so they're numbers.

use crossterm::event::{KeyCode, KeyEvent};
use rynx::app::{App, HintResult, Hints};
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::models::{InputMode, LinkRegion};
use rynx::settings::Settings;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn region(url: &str, line_index: usize, x_start: usize) -> LinkRegion {
    LinkRegion { url: url.to_string(), control: None, image: None, line_index, x_start, x_end: x_start + 4 }
}

/// Pressed on a screen tall enough for the whole test page
fn key(app: &mut App, code: KeyCode) {
    handle_key_event(app, KeyEvent::from(code), 80, 50).unwrap();
}

fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::with_settings(tx, rx, Settings::default()).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

async fn settle(app: &mut App) {
    while let Ok(Some(response)) = tokio::time::timeout(std::time::Duration::from_millis(500), app.rx.recv()).await {
        handle_network_event(app, response, 80).unwrap();
    }
}

/// A page of twelve links, and a field, served with the pages they lead to
async fn site() -> MockServer {
    let server = MockServer::start().await;
    let mut html = String::from("<form action=/search><input name=q></form>");
    for n in 1..=12 {
        html.push_str(&format!("<p><a href=\"/page{n}\">Page {n}</a></p>"));
        Mock::given(path(format!("/page{n}")))
            .respond_with(ResponseTemplate::new(200).set_body_raw(format!("<p>This is page {n}</p>"), "text/html"))
            .mount(&server)
            .await;
    }
    Mock::given(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
        .mount(&server)
        .await;
    server
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_number_is_chosen_once_no_longer_one_could_be_meant() {
    let regions: Vec<LinkRegion> = (0..12).map(|n| region(&format!("/{n}"), n, 0)).collect();
    let mut hints = Hints::for_screen(&regions, 0, 20, false);
    assert_eq!(hints.targets.len(), 12);
    // With twelve, 7 can't be the start of another
    assert_eq!(hints.digit('7'), HintResult::Chosen(6));

    let mut hints = Hints::for_screen(&regions, 0, 20, false);
    assert_eq!(hints.digit('1'), HintResult::Waiting, "1 could be 10, 11 or 12");
    assert_eq!(hints.labels().count(), 4, "1, 10, 11 and 12 still show");
    assert_eq!(hints.chosen(), HintResult::Chosen(0), "Enter takes 1 itself");
    assert_eq!(hints.digit('2'), HintResult::Chosen(11));

    let mut hints = Hints::for_screen(&regions, 0, 20, false);
    assert_eq!(hints.digit('0'), HintResult::Cancelled, "no label starts with 0");
    let mut hints = Hints::for_screen(&regions[..3], 0, 20, false);
    assert_eq!(hints.digit('5'), HintResult::Cancelled, "only three");
}

#[test]
fn only_whats_on_screen_is_numbered_and_wrapped_links_once() {
    let regions = vec![
        region("/above", 0, 3),
        region("/wrapped", 5, 70),
        region("/wrapped", 6, 0),
        region("/same-place-twice", 7, 2),
        region("/same-place-twice", 8, 2),
        region("/below", 30, 0),
    ];
    let hints = Hints::for_screen(&regions, 2, 20, false);
    let numbered: Vec<usize> = hints.targets.iter().map(|(_, region)| *region).collect();
    // The second part of the wrapped link starts its line; two links to one place apart don't
    assert_eq!(numbered, vec![1, 3, 4]);

    // A field can't open in a new tab, so ; leaves it out
    let mut field = region("", 3, 0);
    field.control = Some(0);
    let regions = vec![field, region("/a", 4, 0)];
    assert_eq!(Hints::for_screen(&regions, 0, 20, false).targets.len(), 2);
    assert_eq!(Hints::for_screen(&regions, 0, 20, true).targets, vec![(1, 1)]);
}

#[tokio::test]
async fn f_and_a_number_follow_the_link() {
    let server = site().await;
    let mut app = test_app();
    app.follow_link(format!("{}/", server.uri()));
    settle(&mut app).await;

    key(&mut app, KeyCode::Char('f'));
    assert!(app.current_tab().status_message.contains("Follow which link"));
    // The field is 1, so Page 9 is 10
    typed(&mut app, "10");
    settle(&mut app).await;
    assert!(text(&mut app).contains("This is page 9"), "{}", text(&mut app));
    assert!(app.current_tab().hints.is_none());
}

#[tokio::test]
async fn a_semicolon_opens_it_in_a_new_tab() {
    let server = site().await;
    let mut app = test_app();
    app.follow_link(format!("{}/", server.uri()));
    settle(&mut app).await;
    let tabs = app.tabs.len();

    key(&mut app, KeyCode::Char(';'));
    // No field among them: Page 3 is 3, and 3 is chosen at once (no 30 to wait for)
    typed(&mut app, "3");
    settle(&mut app).await;
    assert_eq!(app.tabs.len(), tabs + 1);
    assert!(text(&mut app).contains("This is page 3"), "{}", text(&mut app));
}

#[tokio::test]
async fn a_field_is_typed_into_and_esc_or_another_key_cancels() {
    let server = site().await;
    let mut app = test_app();
    app.follow_link(format!("{}/", server.uri()));
    settle(&mut app).await;

    key(&mut app, KeyCode::Char('f'));
    key(&mut app, KeyCode::Esc);
    assert!(app.current_tab().hints.is_none());
    assert_eq!(app.current_tab().input_mode, InputMode::Normal);

    // A key that isn't a number cancels, and does nothing else (q doesn't quit)
    key(&mut app, KeyCode::Char('f'));
    assert!(!handle_key_event(&mut app, KeyEvent::from(KeyCode::Char('q')), 80, 50).unwrap());
    assert!(app.current_tab().hints.is_none());
    assert_eq!(app.current_tab().status_message, "No such link");

    // 1 and Enter: the search field, ready for typing
    key(&mut app, KeyCode::Char('f'));
    key(&mut app, KeyCode::Char('1'));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.current_tab().input_mode, InputMode::FormEdit);
}
