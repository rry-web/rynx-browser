//! Reader mode: only a page's main text.

use rynx::network::PageKind;
use rynx::renderer::{DomRenderer, RenderJob, render_page};
use scraper::Html;
use std::sync::Arc;

fn reader(html: &str) -> (Vec<String>, Option<bool>) {
    let page = render_page(&RenderJob {
        source: Arc::from(html),
        kind: PageKind::Html,
        source_view: false,
        reader: true,
        width: 82,
    });
    (page.lines.iter().map(|l| l.to_string()).filter(|l| !l.trim().is_empty()).collect(), page.reader_found)
}

const PROSE: &str = "Reader views keep the article and drop the rest of the page, which on news sites is most of it, ";

fn paragraphs(n: usize) -> String {
    (0..n)
        .map(|i| format!("<p>Paragraph {i}. {PROSE}and this one goes on a little, so it reads like prose.</p>"))
        .collect()
}

#[test]
fn the_article_is_kept_and_the_rest_dropped() {
    let html = format!(
        r#"<html><head><title>Big news | The Paper</title></head><body>
        <header><nav><ul><li><a href="/">Home</a></li><li><a href="/world">World</a></li></ul></nav></header>
        <main><article>
          <h1>Big news</h1><p class="byline">By A. Writer</p>
          <div class="share-buttons"><a href="/share">Share this</a></div>
          {}
          <aside><p>Related: something else entirely, which is not part of this article at all.</p></aside>
          <div class="newsletter-signup"><p>Sign up for our newsletter, it arrives every morning with the news.</p></div>
        </article></main>
        <section class="comments"><p>First! This comment is long enough to look like a paragraph of prose.</p></section>
        <footer><p>Copyright The Paper. All rights reserved, everywhere, forever and ever.</p></footer>
        </body></html>"#,
        paragraphs(4)
    );
    let (lines, found) = reader(&html);
    assert_eq!(found, Some(true));
    let text = lines.join("\n");
    assert!(lines[0].contains("Big news"), "{text}");
    assert!(text.contains("By A. Writer") && text.contains("Paragraph 3."), "{text}");
    for clutter in ["Home", "World", "Share this", "Related:", "newsletter", "First!", "Copyright"] {
        assert!(!text.contains(clutter), "{clutter} should be gone: {text}");
    }
}

#[test]
fn paragraphs_wrapped_in_layers_of_divs_are_found() {
    // BBC wraps every paragraph in four <div>s of its own
    let wrapped: String = (0..6)
        .map(|i| format!(r#"<div><div data-component="text-block"><div class="Grid"><div class="GridItem"><p>Paragraph {i}. {PROSE}</p></div></div></div></div>"#))
        .collect();
    let html = format!(
        r#"<title>Story</title><nav><a href="/">Home</a> <a href="/news">News</a></nav>
        <main id="main-content"><article><h1>The story</h1>{wrapped}</article></main>
        <div class="related"><p>More stories you might like, picked out especially for you today.</p></div>"#
    );
    let (lines, found) = reader(&html);
    assert_eq!(found, Some(true));
    assert_eq!(lines[0], "The story");
    assert!(lines.iter().any(|l| l.contains("Paragraph 5.")));
    assert!(!lines.iter().any(|l| l.contains("Home") || l.contains("More stories")));
}

#[test]
fn page_wide_classes_do_not_count_as_clutter() {
    // Wikipedia's <html> carries feature flags such as "vector-feature-...-menu-..."
    let html = format!(
        r#"<html class="client-nojs vector-feature-main-menu-pinned-disabled"><body class="skin-vector">
        <div id="mw-content-text"><h1>Lynx</h1>{}</div></body></html>"#,
        paragraphs(3)
    );
    assert_eq!(reader(&html).1, Some(true));
}

#[test]
fn pages_without_an_article_are_shown_whole() {
    let home = r#"<title>Home</title><ul>
        <li><a href="/1">A headline about something that happened today</a></li>
        <li><a href="/2">Another headline, about something else that happened</a></li></ul>
        <p><a href="/3">A teaser paragraph that is nothing but a link to the full story</a></p>"#;
    let (lines, found) = reader(home);
    assert_eq!(found, Some(false));
    assert!(lines.iter().any(|l| l.contains("A headline")), "the whole page is shown: {lines:?}");
}

#[test]
fn the_title_heads_an_article_without_one() {
    let html = format!("<title>Untitled essay</title><div class='post-body'>{}</div>", paragraphs(4));
    assert_eq!(reader(&html).0[0], "Untitled essay");
}

#[test]
fn repeated_lines_are_dropped_and_links_follow() {
    // A credit line sent twice (for small and large screens), and a link after it
    let html = format!(
        r#"<article><h1>Title</h1><p class="sm">Credit: A. Photographer</p><p class="lg">Credit: A. Photographer</p>
        {}<p>See <a href="/more">the full report</a> for more, which has every detail in it.</p>
        <pre>same
same</pre></article>"#,
        paragraphs(4)
    );
    let document = Html::parse_document(&html);
    let mut renderer = DomRenderer::new(84);
    assert!(renderer.render_reader(&document));
    let lines: Vec<String> = renderer.lines.iter().map(|l| l.to_string()).collect();
    assert_eq!(lines.iter().filter(|l| l.contains("Credit")).count(), 1, "{lines:?}");
    assert_eq!(lines.iter().filter(|l| *l == "same").count(), 2, "preformatted lines may repeat");
    // The link still points at its own text after the renumbering
    let link = renderer.links.iter().find(|l| l.url == "/more").expect("the link survives");
    let line: String = lines[link.line_index].chars().skip(link.x_start).take(link.x_end - link.x_start).collect();
    assert_eq!(line, "the full report");
}

#[test]
fn an_image_repeated_right_after_itself_appears_once() {
    // In any mode: sites send one copy per screen size
    let html = r#"<figure><img class="hidden md:block" alt="A synth"><img alt="A synth"></figure><p>Text</p>
                  <img alt="A synth">"#;
    let document = Html::parse_document(html);
    let mut renderer = DomRenderer::new(84);
    renderer.render(&document);
    let text: String = renderer.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
    assert_eq!(text.matches("[A synth]").count(), 2, "the copy after other text stays: {text}");
}

// --- In the browser ---

#[tokio::test]
async fn r_toggles_reader_mode_and_it_stays_on() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use ratatui::backend::TestBackend;
    use rynx::app::App;
    use rynx::event_handler::{handle_key_event, handle_network_event};
    use rynx::network::NetworkResponse;

    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    let show = |app: &mut App, url: &str, html: String| {
        let id = app.current_tab().id;
        app.current_tab().url_input = url.into();
        let resp = NetworkResponse::Success(id, "t".into(), html, url.into(), PageKind::Html);
        handle_network_event(app, resp, 80).unwrap();
    };
    let press = |app: &mut App, c: char| {
        let key = KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };
        handle_key_event(app, key, 80, 24).unwrap();
    };
    let text =
        |app: &mut App| app.current_tab().rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");

    let article = format!("<nav><a href='/'>Site menu</a></nav><article><h1>Chapter 1</h1>{}</article>", paragraphs(4));
    show(&mut app, "https://site.example/1", article.clone());
    assert!(text(&mut app).contains("Site menu"));

    press(&mut app, 'R');
    assert!(app.current_tab().reader_mode);
    assert!(!text(&mut app).contains("Site menu") && text(&mut app).contains("Chapter 1"));
    let mut terminal = ratatui::Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| rynx::ui::ui(f, &app)).unwrap();
    let screen: String = terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
    assert!(screen.contains("READER"), "{screen}");

    // The next page opens in reader mode too
    show(&mut app, "https://site.example/2", article.replace("Chapter 1", "Chapter 2"));
    assert!(text(&mut app).contains("Chapter 2") && !text(&mut app).contains("Site menu"));

    // A page with no article is shown whole, saying so
    show(&mut app, "https://site.example/", "<ul><li><a href='/1'>Chapter 1</a></li></ul>".into());
    assert!(app.current_tab().status_message.contains("no article found"), "{}", app.current_tab().status_message);

    press(&mut app, 'R');
    assert!(!app.current_tab().reader_mode);
}
