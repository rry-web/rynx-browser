//! Pages that send you on: meta refreshes (often the site's way on for browsers without
//! JavaScript) followed like redirects, and script redirects offered as links.

use rynx::app::App;
use rynx::event_handler::handle_network_event;
use rynx::settings::Settings;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::with_settings(tx, rx, Settings::default()).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")
}

async fn page(server: &MockServer, at: &str, html: &str) {
    Mock::given(path(at))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html.to_string(), "text/html"))
        .mount(server)
        .await;
}

/// Handle what arrives until the browser has been quiet for a moment (redirects start new loads)
async fn settle(app: &mut App) {
    while let Ok(Some(response)) = tokio::time::timeout(std::time::Duration::from_millis(500), app.rx.recv()).await {
        handle_network_event(app, response, 80).unwrap();
    }
}

async fn requests(server: &MockServer, of: &str) -> usize {
    server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == of).count()
}

#[tokio::test]
async fn a_noscript_refresh_is_followed_and_back_skips_it() {
    let server = MockServer::start().await;
    page(&server, "/first", "<p>The first page</p>").await;
    page(
        &server,
        "/start",
        r#"<html><head><noscript><meta http-equiv="refresh" content="0; url=/nojs?x=1"></noscript></head>
           <body><p>Please enable JavaScript to continue.</p></body></html>"#,
    )
    .await;
    page(&server, "/nojs", "<p>The page without scripts</p>").await;
    let mut app = test_app();
    app.follow_link(format!("{}/first", server.uri()));
    settle(&mut app).await;
    app.follow_link(format!("{}/start", server.uri()));
    settle(&mut app).await;
    assert!(text(&mut app).contains("The page without scripts"), "{}", text(&mut app));
    assert!(app.current_tab().url_input.ends_with("/nojs?x=1"));

    // Back returns to the page before, not to the one that would send you on again
    app.go_back();
    settle(&mut app).await;
    assert!(text(&mut app).contains("The first page"), "{}", text(&mut app));
    assert_eq!(requests(&server, "/nojs").await, 1);
}

#[tokio::test]
async fn pages_sending_each_other_round_are_stopped() {
    let server = MockServer::start().await;
    page(&server, "/a", r#"<meta http-equiv="refresh" content="0;url=/b"><p>A</p>"#).await;
    page(&server, "/b", r#"<meta http-equiv="refresh" content="0;url=/a"><p>B</p>"#).await;
    let mut app = test_app();
    app.follow_link(format!("{}/a", server.uri()));
    settle(&mut app).await;
    let loads = requests(&server, "/a").await + requests(&server, "/b").await;
    assert!(loads <= 7, "{loads} loads");
    assert!(app.current_tab().status_message.contains("keeps sending you on"), "{}", app.current_tab().status_message);
    assert!(text(&mut app).contains("This page sends you on to: /"), "the way on is still a link");
}

#[tokio::test]
async fn countdowns_and_script_redirects_are_offered_as_links() {
    let server = MockServer::start().await;
    page(&server, "/countdown", r#"<meta http-equiv="refresh" content="30; url=/later"><p>Wait for it</p>"#).await;
    page(&server, "/script", r#"<script>window.location.href = "/next";</script><p>Enable JavaScript.</p>"#).await;
    page(&server, "/later", "<p>Later</p>").await;
    page(&server, "/next", "<p>Next</p>").await;
    let mut app = test_app();

    app.follow_link(format!("{}/countdown", server.uri()));
    settle(&mut app).await;
    assert!(text(&mut app).contains("This page sends you on to: /later"), "{}", text(&mut app));
    assert_eq!(requests(&server, "/later").await, 0, "a slow refresh waits for you");

    app.follow_link(format!("{}/script", server.uri()));
    settle(&mut app).await;
    let shown = text(&mut app);
    assert!(shown.starts_with("This page's script would take you to: /next"), "{shown}");
    assert_eq!(requests(&server, "/next").await, 0, "nothing is followed by itself");
    let link = &app.current_tab().link_regions[0];
    assert_eq!(link.url, "/next");
}

#[tokio::test]
async fn scripts_on_full_pages_are_left_alone() {
    let server = MockServer::start().await;
    let words = "word ".repeat(60);
    page(&server, "/full", &format!(r#"<script>location.href = "/elsewhere";</script><p>{words}</p>"#)).await;
    let mut app = test_app();
    app.follow_link(format!("{}/full", server.uri()));
    settle(&mut app).await;
    assert!(!text(&mut app).contains("would take you"), "{}", text(&mut app));
}
