//! Pages are laid out on a background thread so slow pages never freeze the UI.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use rynx::app::{App, RENDERING_STATUS};
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::network::{NetworkResponse, PageKind};
use std::time::{Duration, Instant};

const WIDTH: u16 = 80;

fn background_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    assert!(app.render_in_background, "background rendering should be the default");
    app
}

/// Deliver a page as if it had just been downloaded
fn deliver(app: &mut App, html: String) {
    let id = app.current_tab().id;
    let resp = NetworkResponse::Success(id, "page".into(), html, "http://127.0.0.1:1/p".into(), PageKind::Html);
    handle_network_event(app, resp, WIDTH).unwrap();
}

/// Handle events until the tab's background render is done; returns how many renders arrived
async fn settle(app: &mut App) -> usize {
    let mut renders = 0;
    while app.is_rendering(app.active_tab_index) {
        let resp =
            tokio::time::timeout(Duration::from_secs(60), app.rx.recv()).await.expect("render never finished").unwrap();
        if matches!(resp, NetworkResponse::Rendered(..)) {
            renders += 1;
        }
        handle_network_event(app, resp, WIDTH).unwrap();
    }
    renders
}

fn text(app: &mut App) -> String {
    let lines: Vec<String> = app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect();
    lines.join("\n").trim().to_string()
}

/// Thousands of nested <div>s parse slowly: html5ever's time grows quadratically with depth
fn slow_page(marker: &str) -> String {
    format!("<title>{marker}</title>{}{marker}{}", "<div>".repeat(15_000), "</div>".repeat(15_000))
}

#[tokio::test]
async fn slow_pages_do_not_block_the_ui() {
    let mut app = background_app();

    let started = Instant::now();
    deliver(&mut app, slow_page("slow page"));
    let handled = started.elapsed();
    assert!(handled < Duration::from_millis(200), "delivering the page took {handled:?}");
    assert!(app.is_rendering(0));
    assert_eq!(app.current_tab().status_message, RENDERING_STATUS);

    // Input is handled while the page is being laid out
    let key = KeyEvent {
        code: KeyCode::Char('j'),
        modifiers: KeyModifiers::empty(),
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    };
    let started = Instant::now();
    handle_key_event(&mut app, key, WIDTH, 24).unwrap();
    assert!(started.elapsed() < Duration::from_millis(50));

    settle(&mut app).await;
    assert!(text(&mut app).contains("slow page"));
    assert_eq!(app.current_tab().page_title, "slow page");
    assert_eq!(app.current_tab().status_message, "Loaded");
}

#[tokio::test]
async fn an_older_render_never_replaces_a_newer_page() {
    let mut app = background_app();
    deliver(&mut app, slow_page("old page"));
    // The user moves on before the slow page is laid out
    deliver(&mut app, "<title>new page</title><p>new content</p>".into());
    settle(&mut app).await;
    assert_eq!(text(&mut app), "new content");

    // The slow page's render still arrives later; it must be ignored
    let resp = tokio::time::timeout(Duration::from_secs(60), app.rx.recv()).await.unwrap().unwrap();
    assert!(matches!(resp, NetworkResponse::Rendered(..)));
    handle_network_event(&mut app, resp, WIDTH).unwrap();
    assert_eq!(text(&mut app), "new content");
    assert_eq!(app.current_tab().page_title, "new page");
}

#[tokio::test]
async fn rapid_resizes_are_coalesced() {
    let mut app = background_app();
    // Long paragraphs, so every line fills the width it was laid out for
    let paragraphs = format!("<p>{}</p>", "wrapping words ".repeat(100)).repeat(1_000);
    deliver(&mut app, paragraphs);
    settle(&mut app).await;

    // A window being dragged sends a burst of resize events
    for width in 40..90 {
        app.resize_all_tabs(width);
    }
    let renders = settle(&mut app).await;
    assert!(renders <= 2, "{renders} renders for one burst of resizes");

    // The final layout is the one for the last width
    let expected = rynx::renderer::render_page(&rynx::renderer::RenderJob {
        source: app.current_tab().html_source.clone(),
        kind: PageKind::Html,
        source_view: false,
        width: 89,
    });
    assert!(app.current_tab().rendered_content == expected.lines, "layout isn't for the final width");
}

#[tokio::test]
async fn fragment_jumps_wait_for_the_layout() {
    let mut app = background_app();
    let filler: String = (0..200).map(|i| format!("<p>filler {i}</p>")).collect();
    app.current_tab().pending_fragment = Some("target".into());
    deliver(&mut app, format!("{filler}<h2 id='target'>Target</h2>{filler}"));
    settle(&mut app).await;
    let tab = app.current_tab();
    assert!(tab.scroll > 0);
    assert_eq!(tab.scroll, tab.anchors["target"]);
}

#[tokio::test]
async fn the_help_page_survives_a_resize() {
    let mut app = background_app();
    app.resize_all_tabs(60);
    settle(&mut app).await;
    assert!(text(&mut app).contains("Moving around"), "help page was blanked by a resize");
}

#[tokio::test]
async fn history_gets_the_title_from_a_background_render() {
    let mut app = background_app();
    deliver(&mut app, "<title>Laid out later</title><p>text</p>".into());
    assert!(app.history.entries.is_empty(), "recorded before the title was known");
    settle(&mut app).await;
    assert_eq!(app.history.entries.len(), 1);
    assert_eq!(app.history.entries[0].title, "Laid out later");

    // Re-laying out the same page (a resize) doesn't record it again
    app.render_tab(0, 60);
    settle(&mut app).await;
    assert_eq!(app.history.entries.len(), 1);
}
