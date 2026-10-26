//! RSS and Atom feeds.

use rynx_browser::feeds::{feed_to_html, is_feed};
use rynx_browser::network::PageKind;
use rynx_browser::renderer::{RenderJob, render_page};
use std::sync::Arc;

fn rendered(html: &str) -> (String, Vec<String>) {
    let job = RenderJob { source: Arc::from(html), kind: PageKind::Html, source_view: false, width: 80 };
    let page = render_page(&job);
    let text = page.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
    (text, page.links.into_iter().map(|l| l.url).collect())
}

const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/">
<channel>
  <title>Tom &amp; Jerry's Blog</title>
  <link>https://blog.example/</link>
  <description>Notes on cats &amp; mice</description>
  <image><title>Logo title</title><url>https://blog.example/logo.png</url><link>https://blog.example/</link></image>
  <item>
    <title>First &lt;em&gt;post&lt;/em&gt;</title>
    <link>https://blog.example/first</link>
    <pubDate>Mon, 26 Oct 2026 09:00:00 GMT</pubDate>
    <description>&lt;p&gt;Short &lt;b&gt;summary&lt;/b&gt;&lt;/p&gt;</description>
    <content:encoded><![CDATA[<p>The <a href="/full">full</a> text &amp; more</p><script>alert(1)</script>]]></content:encoded>
  </item>
  <item>
    <title>Second</title>
    <link>https://blog.example/second</link>
    <description>Plain &#8220;quoted&#8221; summary</description>
  </item>
</channel>
</rss>"#;

#[test]
fn rss_feeds_are_listed() {
    let (title, html) = feed_to_html(RSS);
    assert_eq!(title, "Tom & Jerry's Blog", "the <image> title isn't part of it");
    let (text, links) = rendered(&html);
    assert!(text.contains("Notes on cats & mice"), "{text}");
    assert!(text.contains("Feed with 2 items."), "{text}");
    assert!(text.contains("First post"), "tags in titles are dropped: {text}");
    assert!(text.contains("Mon, 26 Oct 2026"), "{text}");
    assert!(text.contains("The full text & more"), "full content is preferred: {text}");
    assert!(!text.contains("Short summary"), "{text}");
    assert!(!text.contains("alert"), "scripts never show: {text}");
    assert!(text.contains("Plain “quoted” summary"), "{text}");
    assert_eq!(links, ["https://blog.example/", "https://blog.example/first", "/full", "https://blog.example/second"]);
}

const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title type="text">Atom Example</title>
  <subtitle>All about atoms</subtitle>
  <link rel="self" href="https://atom.example/feed.xml"/>
  <link href="https://atom.example/"/>
  <entry>
    <title>Entry one</title>
    <link rel="alternate" href="https://atom.example/1"/>
    <updated>2026-10-25T12:00:00Z</updated>
    <summary type="text">Less than &lt; greater than &gt; as text</summary>
  </entry>
  <entry>
    <title type="html">Entry &lt;i&gt;two&lt;/i&gt;</title>
    <link href="https://atom.example/2"/>
    <content type="html">&lt;ul&gt;&lt;li&gt;one&lt;/li&gt;&lt;li&gt;two&lt;/li&gt;&lt;/ul&gt;</content>
  </entry>
</feed>"#;

#[test]
fn atom_feeds_are_listed() {
    let (title, html) = feed_to_html(ATOM);
    assert_eq!(title, "Atom Example");
    let (text, links) = rendered(&html);
    assert!(text.contains("All about atoms"), "{text}");
    assert!(text.contains("Less than < greater than > as text"), "text summaries stay text: {text}");
    assert!(text.contains("Entry two"), "{text}");
    assert!(text.contains("• one"), "html content is rendered: {text}");
    assert!(text.contains("2026-10-25T12:00:00Z"), "{text}");
    assert_eq!(links, ["https://atom.example/", "https://atom.example/1", "https://atom.example/2"]);
}

#[test]
fn feeds_are_recognised() {
    assert!(is_feed(Some("application/rss+xml; charset=utf-8"), ""));
    assert!(is_feed(Some("application/atom+xml"), ""));
    assert!(is_feed(Some("text/xml"), RSS));
    assert!(is_feed(Some("application/xml"), ATOM));
    assert!(!is_feed(Some("application/xml"), "<?xml version=\"1.0\"?><svg/>"));
    assert!(!is_feed(Some("text/html"), RSS));
    assert!(!is_feed(None, RSS));
}

#[test]
fn entity_bombs_are_not_expanded() {
    let bomb = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
<!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">]>
<rss><channel><title>&lol3;</title><item><title>&lol3;</title></item></channel></rss>"#;
    let (title, html) = feed_to_html(bomb);
    assert_eq!(title, "&lol3;");
    assert!(html.len() < 1000, "{}", html.len());
}

#[test]
fn broken_and_deep_feeds_do_not_crash() {
    let deep = format!("<rss><channel><title>x</title>{}</channel></rss>", "<a>".repeat(100_000));
    feed_to_html(&deep);
    feed_to_html("<rss><channel><item><title>unclosed");
    let (title, _) = feed_to_html("not xml at all");
    assert_eq!(title, "Feed");
}

#[test]
fn pages_show_the_feeds_they_announce() {
    let html = r#"<html><head><title>Blog</title>
        <link rel="alternate" type="application/rss+xml" title="Posts" href="/feed.rss">
        <link rel="alternate" type="application/atom+xml" href="/atom.xml">
        <link rel="alternate" hreflang="fr" href="/fr/">
        <link rel="stylesheet" href="/style.css"></head><body><p>Hello</p></body></html>"#;
    let (text, links) = rendered(html);
    assert!(text.starts_with("Feeds: Posts · Atom\n"), "{text}");
    assert_eq!(links, ["/feed.rss", "/atom.xml"]);
}

#[tokio::test]
async fn feeds_served_as_xml_open_as_a_page() {
    use rynx_browser::app::App;
    use rynx_browser::event_handler::handle_network_event;
    use rynx_browser::network::NetworkResponse;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(path("/feed"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(ATOM, "text/xml; charset=utf-8"))
        .mount(&server)
        .await;
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app.current_tab().url_input = format!("{}/feed", server.uri());
    app.submit_request();
    loop {
        let resp = app.rx.recv().await.unwrap();
        let done = matches!(resp, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(&mut app, resp, 80).unwrap();
        if done {
            break;
        }
    }
    assert_eq!(app.current_tab().page_title, "Atom Example");
    assert_eq!(app.current_tab().link_regions[1].url, "https://atom.example/1");
    assert_eq!(app.history.entries[0].title, "Atom Example");
}

#[tokio::test]
#[ignore = "needs the internet"]
async fn live_feed() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let body = reqwest::get("https://blog.rust-lang.org/feed.xml").await.unwrap().text().await.unwrap();
    assert!(is_feed(Some("application/atom+xml"), &body));
    let (title, html) = feed_to_html(&body);
    let (text, links) = rendered(&html);
    println!("{title}: {} links\n{}", links.len(), text.lines().take(40).collect::<Vec<_>>().join("\n"));
    assert!(links.len() > 5);
}
