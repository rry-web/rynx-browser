//! Images: placeholders, loading on request or by the settings, drawing in the page and the viewer.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rynx::app::App;
use rynx::event_handler::{handle_key_event, handle_network_event};
use rynx::network::{NetworkResponse, PageKind};
use rynx::renderer::{DomRenderer, RenderJob, render_page};
use rynx::settings::Settings;
use scraper::Html;
use std::collections::HashMap;
use std::sync::Arc;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn render(html: &str, sizes: &[(&str, (u16, u16))]) -> rynx::renderer::RenderedPage {
    let image_sizes = Arc::new(sizes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<HashMap<_, _>>());
    render_page(&RenderJob {
        source: Arc::from(html),
        kind: PageKind::Html,
        source_view: false,
        reader: false,
        image_sizes,
        width: 82,
    })
}

fn texts(page: &rynx::renderer::RenderedPage) -> Vec<String> {
    page.lines.iter().map(|l| l.to_string()).collect()
}

#[test]
fn images_are_listed_with_their_real_addresses() {
    let page = render(
        r#"<p><img src="data:image/gif;base64,R0lGOD" data-src="/real.jpg" alt="Lazy">
           <img srcset="/small.png 480w, /big.png 800w" alt="Set">
           <a href="/story"><img src="/thumb.png" alt="Thumb"></a></p>"#,
        &[],
    );
    let sources: Vec<&str> = page.images.iter().map(|i| i.src.as_str()).collect();
    assert_eq!(sources, ["/real.jpg", "/small.png", "/thumb.png"]);
    // Each placeholder is selectable, as an image (inside a link, the link still leads on)
    let regions: Vec<(String, Option<usize>)> = page.links.iter().map(|l| (l.url.clone(), l.image)).collect();
    assert_eq!(regions, [(String::new(), Some(0)), (String::new(), Some(1)), ("/story".to_string(), Some(2))]);
}

#[test]
fn loaded_images_get_space_in_the_page() {
    let page = render("<p>Before</p><img src='/a.png' alt='A chart'><p>After</p>", &[("/a.png", (20, 5))]);
    let lines = texts(&page);
    assert_eq!(page.image_blocks.len(), 1);
    let block = page.image_blocks[0];
    assert_eq!((block.columns, block.rows, block.x), (20, 5, 0));
    assert_eq!(lines[block.line_index], " ".repeat(20));
    assert_eq!(lines[block.line_index + 5], "[A chart]", "the caption follows the image: {lines:?}");
    assert!(lines.iter().position(|l| l == "After").unwrap() > block.line_index + 5);

    // In a table cell, an image stays a placeholder
    let page =
        render("<table><tr><td><img src='/a.png' alt='In a cell'></td><td>x</td></tr></table>", &[("/a.png", (20, 5))]);
    assert!(page.image_blocks.is_empty());
    assert!(texts(&page).iter().any(|l| l.contains("[In a cell]")));
}

#[test]
fn image_numbers_survive_tables() {
    let document = Html::parse_document(
        "<img src='/1.png' alt='one'><table><tr><td><img src='/2.png' alt='two'></td></tr></table><img src='/3.png' alt='three'>",
    );
    let mut renderer = DomRenderer::new(82);
    renderer.render(&document);
    let sources: Vec<&str> = renderer.images.iter().map(|i| i.src.as_str()).collect();
    assert_eq!(sources, ["/1.png", "/2.png", "/3.png"]);
    let numbered: Vec<usize> = renderer.links.iter().filter_map(|l| l.image).collect();
    assert_eq!(numbered, [0, 1, 2]);
}

// --- In the browser ---

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| image::Rgb([(x * 6) as u8, (y * 12) as u8, 200]));
    let mut data = std::io::Cursor::new(Vec::new());
    image.write_to(&mut data, image::ImageFormat::Png).unwrap();
    data.into_inner()
}

/// A PNG claiming to be 60,000 pixels square: decoding it fully would need ~10 GB
fn png_bomb() -> Vec<u8> {
    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        [&(data.len() as u32).to_be_bytes()[..], kind, data, &crc.sum().to_be_bytes()].concat()
    }
    let mut header = Vec::new();
    header.extend_from_slice(&60_000u32.to_be_bytes());
    header.extend_from_slice(&60_000u32.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    [
        &b"\x89PNG\r\n\x1a\n"[..],
        &chunk(b"IHDR", &header),
        &chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]),
        &chunk(b"IEND", &[]),
    ]
    .concat()
}

async fn picture_site() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(path("/pic.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(png(200, 100), "image/png"))
        .mount(&server)
        .await;
    Mock::given(path("/bomb.png"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(png_bomb(), "image/png"))
        .mount(&server)
        .await;
    Mock::given(path("/logo.svg"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("<svg xmlns='http://www.w3.org/2000/svg'/>", "image/svg+xml"),
        )
        .mount(&server)
        .await;
    server
}

fn test_app(settings: Settings) -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::with_settings(tx, rx, settings).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app.terminal_width = 80;
    app
}

fn show(app: &mut App, url: &str, html: &str) {
    let id = app.current_tab().id;
    app.current_tab().url_input = url.into();
    let resp = NetworkResponse::Success(id, "t".into(), html.into(), url.into(), PageKind::Html);
    handle_network_event(app, resp, 80).unwrap();
}

fn press(app: &mut App, code: KeyCode) {
    let key =
        KeyEvent { code, modifiers: KeyModifiers::empty(), kind: KeyEventKind::Press, state: KeyEventState::empty() };
    handle_key_event(app, key, 80, 30).unwrap();
}

/// Handle messages until `count` images have arrived
async fn images_arrive(app: &mut App, count: usize) {
    let mut seen = 0;
    while seen < count {
        let resp = tokio::time::timeout(std::time::Duration::from_secs(20), app.rx.recv()).await.unwrap().unwrap();
        seen += usize::from(matches!(resp, NetworkResponse::ImageLoaded(..)));
        handle_network_event(app, resp, 80).unwrap();
    }
}

fn screen(app: &App) -> (String, ratatui::buffer::Buffer) {
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|f| rynx::ui::ui(f, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (buffer.content().iter().map(|c| c.symbol()).collect(), buffer)
}

async fn requests(server: &MockServer, of: &str) -> usize {
    server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == of).count()
}

#[tokio::test]
async fn images_load_when_asked_and_are_drawn_in_the_page() {
    let server = picture_site().await;
    let mut app = test_app(Settings::default());
    let page = format!("{}/", server.uri());
    show(&mut app, &page, "<p>Before</p><img src='/pic.png' alt='A picture'><p>After</p>");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(requests(&server, "/pic.png").await, 0, "by default nothing loads by itself");
    assert!(screen(&app).0.contains("[A picture]"));

    press(&mut app, KeyCode::Char('I'));
    assert!(app.current_tab().status_message.contains("Loading 1 image"));
    images_arrive(&mut app, 1).await;
    assert_eq!(app.current_tab().image_blocks.len(), 1, "space is made for it");
    // Drawn with coloured half blocks here (the test terminal has no graphics)
    let (text, buffer) = screen(&app);
    let block = app.current_tab().image_blocks[0];
    let top = 7 + block.line_index as u16; // tab bar, address bar, the page's border
    let drawn = (0..block.columns).filter(|x| matches!(buffer[(1 + x, top)].symbol(), "▀" | "▄")).count();
    assert!(drawn > 0, "{text}");
    assert!(app.current_tab().status_message.contains("Images loaded"));

    // i shows it full screen; Esc closes
    press(&mut app, KeyCode::Char('i'));
    assert!(app.images.viewer.is_some());
    assert!(screen(&app).0.contains("A picture") && screen(&app).0.contains("Esc closes"));
    press(&mut app, KeyCode::Char('j'));
    assert!(app.images.viewer.is_some(), "other keys don't reach the page behind");
    press(&mut app, KeyCode::Esc);
    assert!(app.images.viewer.is_none());
}

#[tokio::test]
async fn the_site_setting_loads_only_the_pages_own_images() {
    let server = picture_site().await;
    let (settings, _) = Settings::parse("images = site");
    let mut app = test_app(settings);
    let page = format!("{}/", server.uri());
    // The same server under another name stands in for a third-party (tracking) host
    let other = server.uri().replace("127.0.0.1", "localhost");
    show(&mut app, &page, &format!("<img src='/pic.png' alt='Own'><img src='{other}/pic.png?tracker' alt='Other'>"));
    images_arrive(&mut app, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let asked: Vec<String> = server.received_requests().await.unwrap().iter().map(|r| r.url.to_string()).collect();
    assert_eq!(asked.len(), 1, "{asked:?}");
    // (The mock server reports every request as localhost; the third-party one has the query)
    assert!(!asked[0].contains("tracker"), "{asked:?}");
}

#[tokio::test]
async fn public_pages_cannot_load_local_network_images() {
    let server = picture_site().await;
    let mut app = test_app(Settings::default());
    show(&mut app, "https://evil.example/", &format!("<img src='{}/pic.png' alt='Router admin'>", server.uri()));
    press(&mut app, KeyCode::Char('I'));
    assert!(app.current_tab().status_message.contains("local network"), "{}", app.current_tab().status_message);
    press(&mut app, KeyCode::Char('i'));
    assert!(app.current_tab().status_message.contains("Blocked"));
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(requests(&server, "/pic.png").await, 0);
}

#[tokio::test]
async fn image_bombs_and_svg_are_refused() {
    let server = picture_site().await;
    let mut app = test_app(Settings::default());
    show(&mut app, &format!("{}/", server.uri()), "<img src='/bomb.png' alt='Bomb'><img src='/logo.svg' alt='Logo'>");
    press(&mut app, KeyCode::Char('I'));
    images_arrive(&mut app, 2).await;
    assert!(app.current_tab().image_blocks.is_empty());
    assert!(app.current_tab().status_message.contains("couldn't be"), "{}", app.current_tab().status_message);
}

#[test]
fn image_settings_are_parsed() {
    let (settings, problems) = Settings::parse("images = all\nimage_display = viewer\nimage_protocol = blocks");
    assert!(problems.is_empty());
    assert_eq!(Settings::parse(&settings.to_file_text()).0, settings);
    assert_eq!(Settings::parse("images = maybe").1.len(), 1);
}

#[test]
fn only_terminals_known_to_draw_images_are_asked() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
    };
    use rynx::app::draws_images;
    assert!(!draws_images(&env(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm-256color")])), "macOS Terminal");
    assert!(!draws_images(&env(&[("TERM", "xterm-256color")])), "an unknown terminal isn't asked");
    assert!(!draws_images(&env(&[("TERM_PROGRAM", "tmux")])));
    assert!(draws_images(&env(&[("TERM_PROGRAM", "iTerm.app")])));
    assert!(draws_images(&env(&[("TERM", "xterm-kitty")])));
    assert!(draws_images(&env(&[("TERM_PROGRAM", "ghostty")])));
    assert!(draws_images(&env(&[("TERM_PROGRAM", "WezTerm")])));
}
