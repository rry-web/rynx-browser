//! NomadNet pages through the Reticulum bridge, against a fake bridge (the real one has its own
//! test, bridges/reticulum/test_bridge.py, against real Reticulum). One test, in order: the
//! bridge's address is shared by the whole program.

use rynx::app::App;
use rynx::event_handler::handle_network_event;
use rynx::network::NetworkResponse;
use rynx::settings::Settings;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const NODE: &str = "72914442a3689add83a09a767963f57c";

fn app_with(settings: &str) -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let (settings, problems) = Settings::parse(settings);
    assert!(problems.is_empty(), "{problems:?}");
    let mut app = App::with_settings(tx, rx, settings).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app
}

/// Open `address` and handle messages until it has loaded
async fn open(app: &mut App, address: &str) {
    app.current_tab().url_input = address.to_string();
    app.submit_request();
    finish(app).await;
}

async fn finish(app: &mut App) {
    loop {
        let Ok(Some(response)) = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv()).await else {
            return;
        };
        let done = matches!(response, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(app, response, 80).unwrap();
        if done {
            return;
        }
    }
}

fn text(app: &mut App) -> String {
    app.current_tab().rendered_content.iter().map(|l| l.to_string() + "\n").collect()
}

fn micron(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "text/micron; charset=utf-8")
}

#[tokio::test]
async fn nomadnet_pages_come_through_the_bridge() {
    let bridge = MockServer::builder().start().await;
    // Only requests carrying rynx's header are answered, as by the real bridge
    let at = |p: &str| Mock::given(method("GET")).and(path(format!("/{NODE}{p}"))).and(header("x-rynx", "1"));
    at("/page/index.mu")
        .respond_with(micron(
            ">Test node\n`!Welcome`! to the node\n`[About`:/page/about.mu`topic=rynx]\n`[Notes`:/file/notes.txt]",
        ))
        .mount(&bridge)
        .await;
    at("/page/about.mu").respond_with(micron(">About\nAbout this node")).mount(&bridge).await;
    at("/file/notes.txt")
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("Notes over Reticulum\n", "application/octet-stream")
                .insert_header("content-disposition", "attachment; filename=\"notes.txt\""),
        )
        .mount(&bridge)
        .await;
    at("/page/offline.mu")
        .respond_with(
            ResponseTemplate::new(504).set_body_raw(">No path to the node\n\nIt may be offline.", "text/micron"),
        )
        .mount(&bridge)
        .await;

    let downloads = std::env::temp_dir().join(format!("rynx-nomadnet-{}", std::process::id()));
    let mut app = app_with(&format!("reticulum_bridge = {}\n", bridge.uri()));
    app.download_dir = downloads.clone();

    // Typed as NomadNet writes it, shown under its nomadnet:// address
    open(&mut app, &format!("{NODE}:/page/index.mu")).await;
    assert_eq!(app.current_tab().url_input, format!("nomadnet://{NODE}/page/index.mu"));
    assert_eq!(app.current_tab().page_title, "Test node");
    assert!(text(&mut app).contains("Welcome to the node"), "{}", text(&mut app));

    // A link on the same node, with its variable, leads on through the bridge
    let about = app.current_tab().link_regions[0].url.clone();
    let about = rynx::network::resolve_url(app.current_tab().link_base(), &about);
    app.follow_link(about);
    finish(&mut app).await;
    assert_eq!(app.current_tab().url_input, format!("nomadnet://{NODE}/page/about.mu?topic=rynx"));
    assert!(text(&mut app).contains("About this node"), "{}", text(&mut app));
    let requests = bridge.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .any(|r| r.url.path() == format!("/{NODE}/page/about.mu") && r.url.query() == Some("topic=rynx"))
    );

    // A file is offered as a download, and comes through the bridge too
    app.go_back();
    finish(&mut app).await;
    let notes = app.current_tab().link_regions.iter().find(|l| l.url.contains("notes")).unwrap().url.clone();
    let notes = rynx::network::resolve_url(app.current_tab().link_base(), &notes);
    let dir = app.download_dir.clone();
    app.current_tab().initiate_download_request(notes, &dir);
    let prompt = app.current_tab().download_prompt.take().expect("a download prompt");
    app.trigger_download(prompt);
    loop {
        let response = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv()).await.unwrap().unwrap();
        let done = matches!(response, NetworkResponse::DownloadFinished(..) | NetworkResponse::DownloadError(..));
        handle_network_event(&mut app, response, 80).unwrap();
        if done {
            break;
        }
    }
    assert_eq!(std::fs::read_to_string(downloads.join("notes.txt")).unwrap(), "Notes over Reticulum\n");

    // A node the bridge can't reach: the bridge's page says why
    open(&mut app, &format!("nomadnet://{NODE}/page/offline.mu")).await;
    assert!(text(&mut app).contains("No path to the node"), "{}", text(&mut app));

    // Not a node's address
    open(&mut app, "nomadnet://not-a-node/page/index.mu").await;
    assert!(text(&mut app).contains("Not a NomadNet address"), "{}", text(&mut app));

    // Locked to I2P, NomadNet addresses aren't opened: Reticulum isn't I2P
    let before = bridge.received_requests().await.unwrap().len();
    let mut locked = app_with(&format!("reticulum_bridge = {}\nnetwork = i2p\n", bridge.uri()));
    locked.current_tab().url_input = format!("nomadnet://{NODE}/page/index.mu");
    locked.submit_request();
    assert!(
        locked.current_tab().status_message.contains("lock rynx to I2P"),
        "{}",
        locked.current_tab().status_message
    );
    assert_eq!(bridge.received_requests().await.unwrap().len(), before);

    // No bridge running: says how to start one
    let mut nothing = app_with("reticulum_bridge = http://127.0.0.1:1\n");
    open(&mut nothing, &format!("nomadnet://{NODE}/")).await;
    let shown = text(&mut nothing) + &nothing.current_tab().status_message;
    assert!(shown.contains("rynx_reticulum_bridge.py"), "{shown}");

    // Only an address on this computer is accepted for the bridge
    assert_eq!(Settings::parse("reticulum_bridge = http://example.org:4243").1.len(), 1);
    let _ = std::fs::remove_dir_all(&downloads);
}
