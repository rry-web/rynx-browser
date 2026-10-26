//! RSS and Atom feeds, shown as a page listing their items.
//!
//! Feeds are read with an XML parser that never expands custom entities, so "billion laughs"
//! entity bombs can't blow up memory, and the walk over the document isn't recursive.

use crate::network::escape_html;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

/// Feed types that are recognised from the Content-Type alone
const FEED_TYPES: &[&str] = &["application/rss+xml", "application/atom+xml", "application/rdf+xml"];

/// Whether a response with this Content-Type and body is an RSS or Atom feed
pub fn is_feed(content_type: Option<&str>, body: &str) -> bool {
    let mime = content_type.and_then(|ct| ct.split(';').next()).unwrap_or("").trim().to_ascii_lowercase();
    if FEED_TYPES.contains(&mime.as_str()) {
        return true;
    }
    // Feeds are often served as plain XML; look at the root element
    if mime.ends_with("xml") {
        return root_element(body).is_some_and(|root| matches!(root.as_str(), "rss" | "feed" | "rdf:RDF"));
    }
    false
}

/// The name of the document's first element, skipping the XML declaration, comments and the like
fn root_element(body: &str) -> Option<String> {
    let mut reader = quick_xml::Reader::from_str(body);
    // Only the start of the document is looked at
    for _ in 0..64 {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) => return Some(e.name().as_ref().to_string()),
            Event::Eof => return None,
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct Entry {
    title: String,
    link: String,
    date: String,
    /// Summary or description: HTML for RSS, and for Atom unless its type says text
    summary: String,
    summary_is_html: bool,
    /// Full content (content:encoded, or Atom's content), preferred over the summary
    content: String,
    content_is_html: bool,
}

/// What a text field being read goes into
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Title,
    Link,
    Date,
    Summary,
    Content,
    Subtitle,
}

/// Convert a feed to HTML: the feed's title and description, then each item with its date and
/// content. Returns the title and the page.
pub fn feed_to_html(xml: &str) -> (String, String) {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().check_end_names = false;

    let mut feed = Entry::default();
    let mut subtitle = String::new();
    let mut entries: Vec<Entry> = Vec::new();
    // Names of the open elements
    let mut open: Vec<String> = Vec::new();
    // The field whose text is being collected, whether it belongs to the latest entry, and the
    // depth of its element
    let mut field: Option<(Field, bool, usize)> = None;

    loop {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match event {
            Event::Start(e) => {
                let name = e.name().as_ref().to_string();
                let parent = open.last().map(|p| local_name(p).to_string());
                open.push(name.clone());
                if field.is_some() {
                    continue;
                }
                if matches!(local_name(&name), "item" | "entry") {
                    entries.push(Entry::default());
                    continue;
                }
                // Only an item's or the channel's own fields count, not those of e.g. its <image>
                let in_entry = match parent.as_deref() {
                    Some("item" | "entry") => true,
                    Some("channel" | "feed") => false,
                    _ => continue,
                };
                let Some(target) = (if in_entry { entries.last_mut() } else { Some(&mut feed) }) else {
                    continue;
                };
                if local_name(&name) == "link" {
                    // Atom links carry an href; RSS links hold the address as text
                    match alternate_href(&e) {
                        Some(href) => set_if_empty(&mut target.link, href),
                        None if attribute(&e, "href").is_none() => field = Some((Field::Link, in_entry, open.len())),
                        None => {}
                    }
                    continue;
                }
                let html = !matches!(attribute(&e, "type").as_deref(), Some("text"));
                let kind = match name.as_str() {
                    "title" | "dc:title" => Some(Field::Title),
                    "pubDate" | "published" | "updated" | "dc:date" if target.date.is_empty() => Some(Field::Date),
                    "subtitle" => Some(Field::Subtitle),
                    "description" | "summary" => {
                        target.summary_is_html = html;
                        Some(Field::Summary)
                    }
                    "content:encoded" | "content" => {
                        target.content_is_html = html;
                        Some(Field::Content)
                    }
                    _ => None,
                };
                field = kind.map(|f| (f, in_entry, open.len()));
            }
            Event::Empty(e) => {
                let name = e.name().as_ref().to_string();
                let in_entry = match open.last().map(|p| local_name(p)) {
                    Some("item" | "entry") => true,
                    Some("channel" | "feed") => false,
                    _ => continue,
                };
                if field.is_none() && local_name(&name) == "link" {
                    let target = if in_entry { entries.last_mut() } else { Some(&mut feed) };
                    if let (Some(target), Some(href)) = (target, alternate_href(&e)) {
                        set_if_empty(&mut target.link, href);
                    }
                }
            }
            Event::End(_) => {
                if field.is_some_and(|(_, _, depth)| depth == open.len()) {
                    field = None;
                }
                open.pop();
            }
            Event::Text(t) => append(&mut feed, &mut entries, &mut subtitle, field, &t.into_inner()),
            Event::CData(t) => append(&mut feed, &mut entries, &mut subtitle, field, &t.into_inner()),
            Event::GeneralRef(r) => {
                let text = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => match r.into_inner().as_ref() {
                        "amp" => "&".to_string(),
                        "lt" => "<".to_string(),
                        "gt" => ">".to_string(),
                        "quot" => "\"".to_string(),
                        "apos" => "'".to_string(),
                        other => format!("&{};", other),
                    },
                };
                append(&mut feed, &mut entries, &mut subtitle, field, &text);
            }
            _ => {}
        }
    }

    let title = clean(&feed.title);
    let title = if title.is_empty() { String::from("Feed") } else { title };
    let mut html = format!("<title>{0}</title><h1>{0}</h1>", escape_html(&title));
    let description = if subtitle.trim().is_empty() { &feed.summary } else { &subtitle };
    if !description.trim().is_empty() {
        html.push_str(&format!("<p>{}</p>", escape_html(&strip_tags(description))));
    }
    let count = match entries.len() {
        1 => String::from("1 item"),
        n => format!("{} items", n),
    };
    if feed.link.is_empty() {
        html.push_str(&format!("<p>Feed with {}.</p>", count));
    } else {
        html.push_str(&format!(
            "<p>Feed with {}. <a href=\"{}\">Visit the site</a></p>",
            count,
            escape_html(feed.link.trim())
        ));
    }
    for entry in &entries {
        let entry_title = clean(&entry.title);
        let entry_title = if entry_title.is_empty() { String::from("(untitled)") } else { entry_title };
        html.push_str("<hr>");
        if entry.link.trim().is_empty() {
            html.push_str(&format!("<h2>{}</h2>", escape_html(&entry_title)));
        } else {
            html.push_str(&format!(
                "<h2><a href=\"{}\">{}</a></h2>",
                escape_html(entry.link.trim()),
                escape_html(&entry_title)
            ));
        }
        if !entry.date.trim().is_empty() {
            html.push_str(&format!("<p><i>{}</i></p>", escape_html(entry.date.trim())));
        }
        let (body, is_html) = if entry.content.trim().is_empty() {
            (&entry.summary, entry.summary_is_html)
        } else {
            (&entry.content, entry.content_is_html)
        };
        // Feed HTML goes through the same renderer as any page, so it can't do more than a page can
        if is_html {
            html.push_str(&format!("<div>{}</div>", body));
        } else if !body.trim().is_empty() {
            html.push_str(&format!("<p>{}</p>", escape_html(body.trim())));
        }
    }
    (title, html)
}

fn append(
    feed: &mut Entry,
    entries: &mut [Entry],
    subtitle: &mut String,
    field: Option<(Field, bool, usize)>,
    text: &str,
) {
    let Some((field, in_entry, _)) = field else { return };
    if field == Field::Subtitle {
        subtitle.push_str(text);
        return;
    }
    let Some(target) = (if in_entry { entries.last_mut() } else { Some(feed) }) else { return };
    match field {
        Field::Title => target.title.push_str(text),
        Field::Link => target.link.push_str(text),
        Field::Date => target.date.push_str(text),
        Field::Summary => target.summary.push_str(text),
        Field::Content => target.content.push_str(text),
        Field::Subtitle => {}
    }
}

/// Keep the first link found
fn set_if_empty(slot: &mut String, value: String) {
    if slot.is_empty() {
        *slot = value;
    }
}

/// The part of an element name after its namespace prefix
fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn attribute(e: &BytesStart, name: &str) -> Option<String> {
    e.try_get_attribute(name).ok()??.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned())
}

/// An Atom <link>'s address, if it points at the page itself (rel missing or "alternate")
fn alternate_href(e: &BytesStart) -> Option<String> {
    let href = attribute(e, "href")?;
    matches!(attribute(e, "rel").as_deref(), None | Some("alternate")).then_some(href)
}

/// Titles can hold HTML; show them as text on one line
fn clean(text: &str) -> String {
    strip_tags(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Text without HTML tags, for places that show plain text
fn strip_tags(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}
