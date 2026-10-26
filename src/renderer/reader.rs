//! Reader mode: only a page's main text, without its menus, sidebars, share buttons and the like.
//!
//! The main text is the deepest element holding most of the page's prose. Prose is paragraph
//! text that isn't link text, outside navigation, comments, sidebars and other clutter. Looking
//! for the deepest holder, rather than scoring each paragraph's parent, copes with sites that wrap
//! every paragraph in several layers of their own <div>s. The work is bounded: a fixed number of
//! paragraphs, each counted towards a fixed number of ancestors.

use super::menus::class_words;
use super::*;

/// Paragraphs shorter than this are captions, labels or bylines, not article text
const MIN_PARAGRAPH_CHARS: usize = 25;

/// A main text shorter than this isn't an article (a home page, a list of links)
const MIN_ARTICLE_CHARS: usize = 400;

/// At most this many paragraphs are counted
const MAX_PARAGRAPHS: usize = 5000;

/// Each paragraph counts towards this many ancestors
const MAX_LEVELS: usize = 40;

/// The main text holds at least this share of the page's prose (in tenths)
const SHARE_OF_PROSE: usize = 7;

/// Class and id words of things around an article, which never hold its text
const CLUTTER_WORDS: &[&str] = &[
    "comment",
    "comments",
    "sidebar",
    "footer",
    "nav",
    "menu",
    "related",
    "promo",
    "ad",
    "ads",
    "advert",
    "share",
    "sharing",
    "social",
    "sponsor",
    "sponsored",
    "newsletter",
    "popup",
    "banner",
    "widget",
    "masthead",
    "breadcrumb",
    "breadcrumbs",
    "subscribe",
    "signup",
    "recommended",
    "trending",
    "cookie",
];

fn words_of(elem: &scraper::node::Element) -> Vec<String> {
    let mut words = elem.attr("class").map(class_words).unwrap_or_default();
    words.extend(elem.attr("id").map(class_words).unwrap_or_default());
    words
}

/// An element's text length, and how much of it is inside links
fn text_and_links(element: ElementRef) -> (usize, usize) {
    let mut total = 0;
    let mut in_links = 0;
    for node in element.descendants() {
        if let Node::Text(text) = node.value() {
            let len = text.trim().chars().count();
            total += len;
            if node.ancestors().take(8).any(|a| matches!(a.value(), Node::Element(e) if e.name() == "a")) {
                in_links += len;
            }
        }
    }
    (total, in_links)
}

/// The element holding the page's main text, or None if it doesn't look like an article
pub(super) fn main_content(document: &Html) -> Option<ElementRef<'_>> {
    static PARAGRAPHS: OnceLock<Selector> = OnceLock::new();
    let paragraphs = PARAGRAPHS.get_or_init(|| Selector::parse("p, pre").expect("valid selector"));
    let mut prose: HashMap<ego_tree::NodeId, usize> = HashMap::new();
    let mut total = 0;
    for paragraph in document.select(paragraphs).take(MAX_PARAGRAPHS) {
        let (length, in_links) = text_and_links(paragraph);
        let own = length - in_links;
        // Short lines and paragraphs that are mostly links (teasers, link lists) aren't prose
        if length < MIN_PARAGRAPH_CHARS || own * 2 < length {
            continue;
        }
        let ancestors: Vec<ElementRef> = paragraph.ancestors().filter_map(ElementRef::wrap).take(MAX_LEVELS).collect();
        if ancestors.iter().any(|a| is_clutter(a.value())) {
            continue;
        }
        total += own;
        for ancestor in ancestors {
            *prose.entry(ancestor.id()).or_default() += own;
        }
    }
    if total < MIN_ARTICLE_CHARS {
        return None;
    }
    // The deepest element holding most of the prose
    let best = prose
        .into_iter()
        .filter(|&(_, amount)| amount * 10 >= total * SHARE_OF_PROSE)
        .filter_map(|(id, _)| ElementRef::wrap(document.tree.get(id)?))
        .max_by_key(|element| element.ancestors().take(10_000).count())?;
    // A semantic container just around the text also holds its headline and byline
    let best = best
        .ancestors()
        .filter_map(ElementRef::wrap)
        .take(3)
        .find(|a| matches!(a.value().name(), "article" | "main") || a.value().attr("role") == Some("main"))
        .unwrap_or(best);
    Some(best)
}

/// Whether an element inside the main text is clutter to leave out: navigation, asides, forms,
/// share and related-content boxes
pub(super) fn is_clutter(elem: &scraper::node::Element) -> bool {
    if matches!(elem.name(), "nav" | "aside" | "form" | "button" | "input" | "select" | "textarea" | "footer" | "menu")
    {
        return true;
    }
    if matches!(elem.attr("role"), Some("navigation" | "complementary" | "banner" | "contentinfo")) {
        return true;
    }
    // Classes on <html> and <body> describe the whole page (Wikipedia's are feature flags such as
    // "vector-feature-...-menu-..."), and the article itself can't be clutter
    if matches!(elem.name(), "html" | "body" | "main" | "article") {
        return false;
    }
    words_of(elem).iter().any(|w| CLUTTER_WORDS.contains(&w.as_str()))
}

impl DomRenderer {
    /// Render only the page's main text. Returns false, having rendered the whole page, if there
    /// is no article to pick out.
    pub fn render_reader(&mut self, document: &Html) -> bool {
        let Some(main) = main_content(document) else {
            self.render(document);
            return false;
        };
        self.reader = true;
        static HEADING: OnceLock<Selector> = OnceLock::new();
        let heading = HEADING.get_or_init(|| Selector::parse("h1").expect("valid selector"));
        // The page's title as a headline, if the article has none of its own
        if main.select(heading).next().is_none()
            && let Some(title) = document_title(document)
        {
            self.add_style(|s| s.add_modifier(Modifier::BOLD));
            self.push_text(&title);
            self.style_stack.pop();
            self.add_vertical_space();
        }
        // The container's children, so its own class can't count it as clutter
        for child in main.children() {
            self.walk(child);
        }
        self.marker_pending = false;
        self.flush_line();
        self.drop_repeated_lines();
        true
    }

    /// Drop a line that repeats the one before it: sites send some things twice, for small and
    /// large screens, and only CSS (which rynx doesn't apply) hides one. Preformatted text may
    /// repeat lines on purpose, so it's left alone. Link and jump-target positions follow.
    fn drop_repeated_lines(&mut self) {
        let texts: Vec<String> = self.lines.iter().map(|l| l.to_string()).collect();
        let mut keep = vec![true; texts.len()];
        // The last line with text, and whether only blank lines have come since
        let mut previous: Option<usize> = None;
        for i in 0..texts.len() {
            if texts[i].trim().is_empty() {
                continue;
            }
            let repeat = previous.is_some_and(|p| texts[p] == texts[i])
                && !self.preformatted_lines.contains(&i)
                && previous.is_some_and(|p| !self.preformatted_lines.contains(&p));
            if repeat {
                // Drop the copy and the blank lines before it, keeping the spacing after it
                keep[previous.unwrap_or(i) + 1..=i].fill(false);
            } else {
                previous = Some(i);
            }
        }
        if keep.iter().all(|&k| k) {
            return;
        }
        // Where each old line went (a dropped line maps to the copy just before it)
        let mut new_index = Vec::with_capacity(keep.len() + 1);
        let mut next = 0usize;
        for &kept in &keep {
            new_index.push(if kept { next } else { next.saturating_sub(1) });
            next += usize::from(kept);
        }
        new_index.push(next);
        let lines = std::mem::take(&mut self.lines);
        self.lines = lines.into_iter().zip(&keep).filter(|(_, k)| **k).map(|(l, _)| l).collect();
        self.links.retain(|link| keep.get(link.line_index).copied().unwrap_or(false));
        for link in &mut self.links {
            link.line_index = new_index[link.line_index];
        }
        for line in self.anchors.values_mut() {
            *line = new_index[(*line).min(new_index.len() - 1)];
        }
    }
}
