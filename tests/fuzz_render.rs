//! Randomised robustness tests for the HTML renderer and UI, runnable on stable Rust.
//!
//! Each case builds messy HTML from a seeded generator, renders it at a random width and checks
//! invariants, then drives the app with random input. A failure prints its seed; rerun just that
//! case with `FUZZ_SEED=<seed> cargo test --test fuzz_render`. Run longer with
//! `FUZZ_ITERATIONS=100000 cargo test --release --test fuzz_render`, and split a long run across
//! processes with `FUZZ_START=<first seed>`.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::{Terminal, backend::TestBackend};
use rynx_browser::app::App;
use rynx_browser::event_handler::{handle_key_event, handle_mouse_event, handle_network_event};
use rynx_browser::network::NetworkResponse;
use rynx_browser::renderer::DomRenderer;
use rynx_browser::ui::ui;
use scraper::Html;

/// xorshift64*: tiny, deterministic, good enough to explore inputs
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const TAGS: &[&str] = &[
    "p", "div", "a", "b", "strong", "i", "em", "pre", "code", "ul", "ol", "li", "h1", "h2", "h3",
    "table", "tr", "td", "th", "img", "br", "hr", "span", "section", "article", "nav", "header",
    "footer", "aside", "main", "script", "style", "head", "title", "blockquote", "unknown",
];

const TEXT: &[&str] = &[
    "hello", "world", " ", "  ", "\n", "\t", "a", "longwordwithoutanyspacesatallthatmustbehardwrapped",
    "漢字テキスト", "中文", "🦀", "👨‍👩‍👧", "e\u{301}", "\u{200b}", "\u{202e}rtl", "\u{1b}[31m",
    "\u{1b}]52;c;cHduZWQ=\u{7}", "\u{9b}2J", "\r", "\u{0}", "&amp;", "&lt;b&gt;", "&#x1F980;", "&nbsp;",
    "x\u{fe0f}", "\u{fffd}",
];

const HREFS: &[&str] = &[
    "#top", "#ch1", "#", "", "page.html", "../up", "//other.example/x", "https://example.com/a?b=c#d",
    "http://[::1]/", "mailto:a@b.c", "javascript:void(0)", "data:,x", "\u{1b}[2J", "%zz", "https://",
    "https://xn--pple-43d.com/",
];

/// Keep pages small: nesting grows exponentially, and small inputs explore far more cases per
/// second. Render time is linear in size, so large pages add no new behaviour.
const MAX_HTML_BYTES: usize = 16 * 1024;

fn random_html(rng: &mut Rng, depth: usize) -> String {
    let mut html = String::new();
    random_html_into(rng, depth, &mut html);
    html
}

fn random_html_into(rng: &mut Rng, depth: usize, html: &mut String) {
    for _ in 0..rng.below(8) {
        if html.len() > MAX_HTML_BYTES {
            return;
        }
        match rng.below(5) {
            0 | 1 => html.push_str(rng.pick(TEXT)),
            _ if depth < 12 => {
                let tag = rng.pick(TAGS);
                html.push('<');
                html.push_str(tag);
                for _ in 0..rng.below(3) {
                    match rng.below(5) {
                        0 => html.push_str(&format!(" href=\"{}\"", rng.pick(HREFS))),
                        1 => html.push_str(&format!(" id=\"ch{}\"", rng.below(4))),
                        2 => html.push_str(&format!(" name=\"{}\"", rng.pick(TEXT))),
                        3 => html.push_str(&format!(" alt=\"{}\"", rng.pick(TEXT))),
                        _ => html.push_str(" hidden"),
                    }
                }
                html.push('>');
                random_html_into(rng, depth + 1, html);
                // Sometimes leave tags unclosed or close the wrong one
                match rng.below(4) {
                    0 => {}
                    1 => html.push_str(&format!("</{}>", rng.pick(TAGS))),
                    _ => html.push_str(&format!("</{}>", tag)),
                }
            }
            _ => html.push_str(rng.pick(TEXT)),
        }
    }
}

fn check_render_invariants(seed: u64, html: &str, width: usize) {
    let document = Html::parse_document(html);
    let mut renderer = DomRenderer::new(width);
    renderer.render(&document);
    let lines = &renderer.lines;

    for line in lines {
        let text = line.to_string();
        assert!(
            !text.contains(char::is_control),
            "seed {seed}: control character in rendered line {text:?}"
        );
    }
    for link in &renderer.links {
        assert!(link.line_index < lines.len(), "seed {seed}: link on missing line");
        assert!(link.x_start < link.x_end, "seed {seed}: empty link region");
        assert!(
            link.x_end <= lines[link.line_index].width(),
            "seed {seed}: link region past end of line"
        );
    }
    for (name, &line) in &renderer.anchors {
        assert!(line <= lines.len(), "seed {seed}: anchor {name:?} past end of page");
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::empty(),
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

/// Load the page into the app and hammer it with input that doesn't touch the network.
/// The app is reused across cases (building one is slow), so modes and state carry over.
fn drive_app(app: &mut App, rng: &mut Rng, html: &str) {
    let width = 1 + rng.below(120) as u16;
    let height = 1 + rng.below(50) as u16;
    let id = app.current_tab().id;
    handle_network_event::<TestBackend>(
        app,
        NetworkResponse::Success(id, "t".into(), html.into(), "http://127.0.0.1:1/p".into()),
        width,
    )
    .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    for _ in 0..30 {
        match rng.below(14) {
            0 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('j')), width, height),
            1 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('k')), width, height),
            2 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('h')), width, height),
            3 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('l')), width, height),
            4 => handle_key_event::<TestBackend>(app, key(KeyCode::Tab), width, height),
            5 => handle_key_event::<TestBackend>(app, key(KeyCode::BackTab), width, height),
            6 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('v')), width, height),
            7 => handle_key_event::<TestBackend>(app, key(KeyCode::Esc), width, height),
            8 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('/')), width, height),
            9 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('o')), width, height),
            10 => handle_key_event::<TestBackend>(app, key(KeyCode::Char('V')), width, height),
            11 => handle_key_event::<TestBackend>(app, key(KeyCode::Down), width, height),
            12 => {
                let kind = if rng.below(2) == 0 {
                    MouseEventKind::ScrollDown
                } else {
                    MouseEventKind::ScrollUp
                };
                let event = MouseEvent { kind, column: 5, row: 9, modifiers: KeyModifiers::empty() };
                handle_mouse_event::<TestBackend>(app, event, width, height).map(|_| false)
            }
            _ => {
                let new_width = 1 + rng.below(120) as u16;
                app.resize_all_tabs(new_width);
                Ok(false)
            }
        }
        .unwrap();
        terminal.draw(|f| ui(f, &app)).unwrap();
    }
}

fn new_app() -> App {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app
}

fn run_case(app: &mut App, seed: u64) {
    let mut rng = Rng::new(seed);
    let html = random_html(&mut rng, 0);
    // Include absurdly narrow widths: the content area can be 0 columns wide
    let width = match rng.below(4) {
        0 => rng.below(4),
        _ => rng.below(200),
    };
    let started = std::time::Instant::now();
    check_render_invariants(seed, &html, width);
    let rendered = started.elapsed();
    drive_app(app, &mut rng, &html);
    if std::env::var("FUZZ_VERBOSE").is_ok() {
        eprintln!(
            "seed {seed}: {} bytes of HTML at width {width}, render {:?}, app {:?}",
            html.len(),
            rendered,
            started.elapsed() - rendered
        );
    }
}

#[tokio::test]
async fn fuzz_render_and_ui() {
    if let Ok(seed) = std::env::var("FUZZ_SEED") {
        run_case(&mut new_app(), seed.parse().expect("FUZZ_SEED must be a number"));
        return;
    }
    let iterations: u64 = std::env::var("FUZZ_ITERATIONS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(100);
    let start: u64 = std::env::var("FUZZ_START")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let mut app = new_app();
    for seed in start..start + iterations {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_case(&mut app, seed)));
        if let Err(panic) = result {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            panic!("fuzz case failed; rerun with FUZZ_SEED={seed}: {message}");
        }
    }
}
