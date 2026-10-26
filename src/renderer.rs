use crate::constants::UI_BORDER_WIDTH;
use crate::models::LinkRegion;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use scraper::{ElementRef, Html, Node};
use std::borrow::Cow;
use std::collections::HashMap;
use unicode_width::UnicodeWidthChar;

/// Make untrusted text safe to draw. Control characters are dropped because a terminal acts on
/// them (escape sequences can set the title, clear the screen or write the clipboard) instead of
/// displaying them. Tabs become spaces so they can't move the cursor.
pub fn sanitize_for_terminal(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut clean = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\t' {
            clean.push_str("    ");
        } else if !ch.is_control() {
            clean.push(ch);
        }
    }
    Cow::Owned(clean)
}

/// Width of a rendered line as drawn: one cell per character's width. This can differ from
/// Line::width(), which measures whole strings (an emoji joined with ZWJ is width 2 as a string
/// but is drawn, and laid out by the renderer, character by character).
pub fn display_width(line: &Line) -> usize {
    line.spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(char_width)
        .sum()
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

fn text_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Elements whose content is never shown
const SKIPPED_TAGS: &[&str] = &[
    "script", "style", "head", "meta", "link", "title", "template", "svg", "object", "embed",
    "param", "source", "track",
];

const BULLETS: &[&str] = &["•", "◦", "▪"];

/// Elements nested deeper than this are rendered as plain text. Rendering recurses once per
/// level, and ~10,000 nested <div>s (60KB of HTML) used to overflow the stack and crash.
const MAX_NESTING: usize = 256;

/// Drawn at the start of every line inside a block: list indentation, quote bars
#[derive(Clone, Copy)]
enum LinePrefix {
    Indent(usize),
    QuoteBar,
}

#[derive(Clone, Copy)]
enum Numbering {
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

enum ListKind {
    Unordered,
    Ordered {
        next: i64,
        step: i64,
        numbering: Numbering,
        /// Widest marker in the list, so numbers line up ("  9." above " 10.")
        marker_width: usize,
    },
}

fn format_number(n: i64, numbering: Numbering) -> String {
    fn alpha(mut n: i64, base: u8) -> String {
        // Bijective base 26: 1 = a, 26 = z, 27 = aa
        let mut letters = Vec::new();
        while n > 0 {
            n -= 1;
            letters.push((base + (n % 26) as u8) as char);
            n /= 26;
        }
        letters.iter().rev().collect()
    }
    fn roman(mut n: i64) -> String {
        const NUMERALS: &[(i64, &str)] = &[
            (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
            (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
        ];
        let mut out = String::new();
        for &(value, numeral) in NUMERALS {
            while n >= value {
                out.push_str(numeral);
                n -= value;
            }
        }
        out
    }
    match numbering {
        Numbering::LowerAlpha if n > 0 => alpha(n, b'a'),
        Numbering::UpperAlpha if n > 0 => alpha(n, b'A'),
        Numbering::LowerRoman if (1..4000).contains(&n) => roman(n),
        Numbering::UpperRoman if (1..4000).contains(&n) => roman(n).to_uppercase(),
        // Letters and numerals can't express everything; fall back to digits like browsers do
        _ => n.to_string(),
    }
}

/// Hidden with the hidden attribute, aria-hidden or an inline display:none / visibility:hidden
fn is_hidden(elem: &scraper::node::Element) -> bool {
    if elem.attr("hidden").is_some() || elem.attr("aria-hidden") == Some("true") {
        return true;
    }
    elem.attr("style").is_some_and(|style| {
        let style: String = style.chars().filter(|c| !c.is_whitespace()).collect();
        let style = style.to_ascii_lowercase();
        style.contains("display:none") || style.contains("visibility:hidden")
    })
}

/// What to do once an element's children have been rendered
#[derive(PartialEq)]
enum AfterElement {
    Nothing,
    EndLine,
    BlockSpace,
}

pub struct DomRenderer {
    pub lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    pub links: Vec<LinkRegion>,
    /// Line index for each in-page link target (`id`, or `name` on `<a>`)
    pub anchors: HashMap<String, usize>,
    max_width: usize,
    current_line_width: usize,
    /// Width of the prefix (indentation, quote bars) at the start of the current line
    line_prefix_width: usize,
    /// Whether the current line has content beyond its prefix
    line_has_text: bool,
    last_line_blank: bool,
    /// Whitespace seen since the last word, drawn only if another word follows on the same line,
    /// in the style and link it appeared in
    pending_space: Option<(Style, Option<String>)>,
    /// A list marker was just drawn; keep the item's first content on the same line
    marker_pending: bool,
    active_link_url: Option<String>,
    preserve_whitespace: bool,
    prefix_stack: Vec<LinePrefix>,
    list_stack: Vec<ListKind>,
    /// How many elements deep the walk currently is
    depth: usize,
}

impl DomRenderer {
    pub fn new(width: usize) -> Self {
        Self::with_content_width(width.saturating_sub(UI_BORDER_WIDTH))
    }

    /// A renderer whose lines are at most `max_width` columns (no allowance for UI borders)
    fn with_content_width(max_width: usize) -> Self {
        Self {
            lines: Vec::new(),
            current_line: Vec::new(),
            style_stack: vec![Style::default()],
            links: Vec::new(),
            anchors: HashMap::new(),
            max_width,
            current_line_width: 0,
            line_prefix_width: 0,
            line_has_text: false,
            last_line_blank: true,
            pending_space: None,
            marker_pending: false,
            active_link_url: None,
            preserve_whitespace: false,
            prefix_stack: Vec::new(),
            list_stack: Vec::new(),
            depth: 0,
        }
    }

    pub fn render(&mut self, document: &Html) {
        for node in document.tree.root().children() {
            self.walk(node);
        }
        self.marker_pending = false;
        self.flush_line();
    }

    fn current_style(&self) -> Style {
        *self.style_stack.last().unwrap_or(&Style::default())
    }

    fn add_style(&mut self, change: impl FnOnce(Style) -> Style) {
        let style = change(self.current_style());
        self.style_stack.push(style);
    }

    // --- Lines ---

    fn flush_line(&mut self) {
        if self.marker_pending {
            return;
        }
        if self.line_has_text {
            self.lines.push(Line::from(std::mem::take(&mut self.current_line)));
            self.last_line_blank = false;
        }
        // A line holding only its prefix is discarded
        self.current_line.clear();
        self.current_line_width = 0;
        self.line_prefix_width = 0;
        self.line_has_text = false;
        self.pending_space = None;
    }

    /// A blank line, keeping quote bars so a quotation reads as one block
    fn push_blank_line(&mut self) {
        let mut spans = self.prefix_spans();
        // Drop the prefix's trailing spaces: "│ " becomes "│"
        while let Some(last) = spans.last_mut() {
            let trimmed = last.content.trim_end().to_string();
            if trimmed.is_empty() {
                spans.pop();
            } else {
                last.content = Cow::Owned(trimmed);
                break;
            }
        }
        self.lines.push(Line::from(spans));
        self.last_line_blank = true;
    }

    /// Separate blocks (paragraphs, headings, lists) with a single blank line
    fn add_vertical_space(&mut self) {
        if self.marker_pending {
            return;
        }
        self.flush_line();
        if !self.last_line_blank && !self.lines.is_empty() {
            self.push_blank_line();
        }
    }

    /// An explicit line break (<br>, a newline in <pre>); breaking an empty line leaves it blank
    fn line_break(&mut self) {
        if self.marker_pending {
            return;
        }
        let had_text = self.line_has_text;
        self.flush_line();
        // Runs of <br> used as spacers collapse to one blank line; in <pre> every line counts
        if !had_text && (self.preserve_whitespace || !self.last_line_blank) {
            self.push_blank_line();
        }
    }

    fn prefix_spans(&self) -> Vec<Span<'static>> {
        // Capped at half the width so deep nesting still leaves room for text
        let cap = self.max_width / 2;
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut width = 0;
        for prefix in &self.prefix_stack {
            let (text, style) = match prefix {
                LinePrefix::Indent(n) => (" ".repeat(*n), Style::default()),
                LinePrefix::QuoteBar => ("│ ".to_string(), Style::default().fg(Color::DarkGray)),
            };
            for ch in text.chars() {
                if width >= cap {
                    return spans;
                }
                match spans.last_mut() {
                    Some(last) if last.style == style => last.content.to_mut().push(ch),
                    _ => spans.push(Span::styled(ch.to_string(), style)),
                }
                width += 1;
            }
        }
        spans
    }

    /// Start a fresh line with the current block prefix
    fn apply_prefix(&mut self) {
        if !self.current_line.is_empty() {
            return;
        }
        self.current_line = self.prefix_spans();
        self.current_line_width = self.current_line.iter().map(|s| text_width(&s.content)).sum();
        self.line_prefix_width = self.current_line_width;
    }

    // --- Text ---

    /// Append text to the current line, tracking link regions
    fn emit(&mut self, text: &str, style: Style, link: Option<&str>) {
        for ch in text.chars() {
            let width = char_width(ch);
            let start_x = self.current_line_width;
            let end_x = start_x + width;

            // Runs of same-styled text share one span; a span per character used ~70x the page
            // size in memory. The UI splits visible lines into characters for highlighting.
            match self.current_line.last_mut() {
                Some(last) if last.style == style => last.content.to_mut().push(ch),
                _ => self.current_line.push(Span::styled(ch.to_string(), style)),
            }
            self.current_line_width += width;

            if let Some(url) = link {
                let line_index = self.lines.len();
                // Merge with the previous region if it's the same link and contiguous
                if let Some(last) = self.links.last_mut() {
                    if last.line_index == line_index && last.url == url && last.x_end == start_x {
                        last.x_end = end_x;
                        continue;
                    }
                }
                // A zero-width character (combining mark, zero-width space) can't start a
                // clickable region on its own
                if width == 0 {
                    continue;
                }
                self.links.push(LinkRegion {
                    url: url.to_string(),
                    line_index,
                    x_start: start_x,
                    x_end: end_x,
                });
            }
        }
    }

    /// Append content in the current style and link
    fn emit_text(&mut self, text: &str) {
        let link = self.active_link_url.clone();
        self.emit(text, self.current_style(), link.as_deref());
        self.line_has_text = true;
        self.marker_pending = false;
    }

    fn note_space(&mut self) {
        if self.pending_space.is_none() {
            self.pending_space = Some((self.current_style(), self.active_link_url.clone()));
        }
    }

    /// Place a word of flowing text, wrapping to a new line when it doesn't fit
    fn push_word(&mut self, word: &str) {
        if word.is_empty() {
            return;
        }
        self.apply_prefix();
        let width = text_width(word);
        let space = self.pending_space.take().filter(|_| self.line_has_text);
        let space_width = usize::from(space.is_some());

        if self.current_line_width + space_width + width <= self.max_width {
            if let Some((style, link)) = space {
                self.emit(" ", style, link.as_deref());
            }
            self.emit_text(word);
        } else if self.line_has_text && self.line_prefix_width + width <= self.max_width {
            self.flush_line();
            self.apply_prefix();
            self.emit_text(word);
        } else {
            self.hard_wrap(word);
        }
    }

    /// Split text that is too wide for any line (e.g. a long URL) across lines
    fn hard_wrap(&mut self, text: &str) {
        if self.line_has_text && !self.marker_pending {
            self.flush_line();
        }
        let mut remaining = text;
        while !remaining.is_empty() {
            self.apply_prefix();
            // Place at least one character per line, so this always makes progress even when
            // the prefix fills the line or the content area is 0 columns wide
            let available = self.max_width.saturating_sub(self.current_line_width).max(1);
            let mut used = 0;
            let mut split = 0;
            for (i, ch) in remaining.char_indices() {
                if used + char_width(ch) > available && split > 0 {
                    break;
                }
                used += char_width(ch);
                split = i + ch.len_utf8();
            }
            let (chunk, rest) = remaining.split_at(split);
            self.emit_text(chunk);
            remaining = rest;
            if !remaining.is_empty() {
                self.flush_line();
            }
        }
    }

    /// Preformatted text: spaces are kept, and long lines wrap at the edge of the screen
    fn push_preformatted(&mut self, text: &str) {
        for ch in text.chars() {
            self.apply_prefix();
            if self.line_has_text && self.current_line_width + char_width(ch) > self.max_width {
                self.flush_line();
                self.apply_prefix();
            }
            let mut buf = [0u8; 4];
            self.emit_text(ch.encode_utf8(&mut buf));
        }
    }

    fn push_text(&mut self, text: &str) {
        if self.preserve_whitespace {
            for (i, segment) in text.split('\n').enumerate() {
                if i > 0 {
                    self.line_break();
                }
                let segment = sanitize_for_terminal(segment);
                if !segment.is_empty() {
                    self.push_preformatted(&segment);
                }
            }
            return;
        }
        // HTML collapses ASCII whitespace only, so &nbsp; stays part of the word
        if text.starts_with(|c: char| c.is_ascii_whitespace()) {
            self.note_space();
        }
        for (i, word) in text.split_ascii_whitespace().enumerate() {
            if i > 0 {
                self.note_space();
            }
            self.push_word(&sanitize_for_terminal(word));
        }
        if text.ends_with(|c: char| c.is_ascii_whitespace()) {
            self.note_space();
        }
    }

    /// Text drawn as a link to `href`, e.g. for embedded frames and media
    fn push_link_text(&mut self, text: &str, href: &str) {
        let saved_link = self.active_link_url.replace(href.to_string());
        self.add_style(|s| s.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED));
        self.push_text(text);
        self.style_stack.pop();
        self.active_link_url = saved_link;
    }

    // --- Elements ---

    fn walk(&mut self, node: ego_tree::NodeRef<Node>) {
        match node.value() {
            Node::Text(text) => self.push_text(&text.text),
            Node::Element(_) => {
                if let Some(element) = ElementRef::wrap(node) {
                    self.walk_element(element);
                }
            }
            _ => {}
        }
    }

    fn walk_element(&mut self, element: ElementRef) {
        let elem = element.value();
        let tag = elem.name();
        if SKIPPED_TAGS.contains(&tag) || is_hidden(elem) {
            return;
        }
        if self.depth >= MAX_NESTING {
            // ElementRef::text walks the subtree iteratively, so this can't overflow the stack
            let text: String = element.text().collect();
            return self.push_text(&text);
        }

        // Elements drawn without walking their children
        match tag {
            "br" => return self.line_break(),
            "hr" => {
                self.add_vertical_space();
                self.apply_prefix();
                let rule = "─".repeat(self.max_width.saturating_sub(self.current_line_width));
                self.emit_text(&rule);
                return self.add_vertical_space();
            }
            "img" => {
                // alt="" marks a decorative image; an image without alt is only worth showing
                // when it's a link, so the link stays visible
                let label = match elem.attr("alt").map(str::trim) {
                    Some("") => return,
                    Some(alt) => alt.to_string(),
                    None if self.active_link_url.is_some() => "image".to_string(),
                    None => return,
                };
                self.add_style(|s| s.fg(Color::DarkGray));
                self.push_word(&format!("[{}]", sanitize_for_terminal(&label)));
                self.style_stack.pop();
                return;
            }
            "iframe" => {
                if let Some(src) = elem.attr("src") {
                    let label = elem.attr("title").unwrap_or("embedded page");
                    self.flush_line();
                    self.push_link_text(&format!("[Embedded: {}]", label), src);
                    self.flush_line();
                }
                return;
            }
            "video" | "audio" => {
                let src = elem.attr("src").or_else(|| {
                    element
                        .children()
                        .filter_map(ElementRef::wrap)
                        .find(|child| child.value().name() == "source")
                        .and_then(|source| source.value().attr("src"))
                });
                if let Some(src) = src {
                    let label = if tag == "video" { "[Video]" } else { "[Audio]" };
                    self.flush_line();
                    self.push_link_text(label, src);
                    self.flush_line();
                }
                return;
            }
            "noscript" => return self.walk_noscript(element),
            _ => {}
        }

        let saved_link = self.active_link_url.clone();
        let saved_preserve = self.preserve_whitespace;
        let saved_styles = self.style_stack.len();
        let saved_prefixes = self.prefix_stack.len();
        let saved_lists = self.list_stack.len();

        self.open_element(element);

        // Record link targets after any block spacing, so jumps land on the content
        let anchor = elem
            .attr("id")
            .or_else(|| if tag == "a" { elem.attr("name") } else { None });
        if let Some(name) = anchor {
            // First occurrence wins, as in browsers
            self.anchors.entry(name.to_string()).or_insert(self.lines.len());
        }

        self.depth += 1;
        for child in element.children() {
            self.walk(child);
        }
        self.depth -= 1;

        let after = self.close_element(tag);
        if after != AfterElement::Nothing {
            self.flush_line();
        }

        // A blank line left by the element's last child belongs outside it: redraw it without
        // the element's prefix, so a quotation doesn't end with a stray "│"
        let prefix_shrinks = self.prefix_stack.len() > saved_prefixes;

        // Undo everything the element pushed, even if its children were unbalanced
        self.style_stack.truncate(saved_styles);
        self.prefix_stack.truncate(saved_prefixes);
        if prefix_shrinks && self.last_line_blank && !self.line_has_text && !self.lines.is_empty() {
            self.lines.pop();
            self.push_blank_line();
        }
        self.list_stack.truncate(saved_lists);
        self.active_link_url = saved_link;
        self.preserve_whitespace = saved_preserve;

        if after == AfterElement::BlockSpace {
            self.add_vertical_space();
        }
    }

    fn open_element(&mut self, element: ElementRef) {
        let elem = element.value();
        match elem.name() {
            "b" | "strong" => self.add_style(|s| s.add_modifier(Modifier::BOLD)),
            "i" | "em" | "cite" | "dfn" | "var" => self.add_style(|s| s.add_modifier(Modifier::ITALIC)),
            "u" | "ins" => self.add_style(|s| s.add_modifier(Modifier::UNDERLINED)),
            "s" | "strike" | "del" => self.add_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
            "small" => self.add_style(|s| s.add_modifier(Modifier::DIM)),
            "mark" => self.add_style(|s| s.bg(Color::DarkGray).fg(Color::White)),
            "code" | "kbd" | "samp" | "tt" => self.add_style(|s| s.fg(Color::Magenta)),
            "a" => {
                // Only real links are styled; <a id=...> is just a jump target
                if let Some(href) = elem.attr("href") {
                    self.add_style(|s| s.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED));
                    self.active_link_url = Some(href.to_string());
                }
            }
            "q" => self.push_word("“"),
            "sup" => self.push_word("^"),
            "sub" => self.push_word("_"),
            "h1" | "h2" | "h3" => {
                self.add_vertical_space();
                self.add_style(|s| s.fg(Color::White).add_modifier(Modifier::BOLD));
            }
            "h4" | "h5" | "h6" => {
                self.add_vertical_space();
                self.add_style(|s| s.add_modifier(Modifier::BOLD));
            }
            "p" | "main" | "article" | "section" | "aside" | "figure" | "fieldset" | "form"
            | "details" | "center" | "hgroup" | "dl" | "table" => self.add_vertical_space(),
            "address" => {
                self.add_vertical_space();
                self.add_style(|s| s.add_modifier(Modifier::ITALIC));
            }
            "div" | "header" | "footer" | "nav" | "legend" | "search" | "tr" => self.flush_line(),
            "figcaption" => {
                self.flush_line();
                self.add_style(|s| s.add_modifier(Modifier::ITALIC | Modifier::DIM));
            }
            "summary" => {
                self.flush_line();
                self.add_style(|s| s.add_modifier(Modifier::BOLD));
                self.push_word("▸");
                self.note_space();
            }
            "pre" => {
                self.add_vertical_space();
                self.preserve_whitespace = true;
                self.add_style(|s| s.fg(Color::Magenta));
            }
            "blockquote" => {
                self.add_vertical_space();
                self.prefix_stack.push(LinePrefix::QuoteBar);
            }
            "ul" | "ol" | "menu" => {
                if self.list_stack.is_empty() {
                    self.add_vertical_space();
                } else {
                    self.flush_line();
                }
                let kind = if elem.name() == "ol" {
                    Self::ordered_list(element)
                } else {
                    ListKind::Unordered
                };
                self.list_stack.push(kind);
                self.prefix_stack.push(LinePrefix::Indent(2));
            }
            "li" => self.start_list_item(elem),
            "dt" => {
                self.flush_line();
                self.add_style(|s| s.add_modifier(Modifier::BOLD));
            }
            "dd" => {
                self.flush_line();
                self.prefix_stack.push(LinePrefix::Indent(4));
            }
            "td" | "th" => {
                if self.line_has_text {
                    self.emit_text("  ");
                }
                if elem.name() == "th" {
                    self.add_style(|s| s.add_modifier(Modifier::BOLD));
                }
            }
            _ => {}
        }
    }

    fn close_element(&mut self, tag: &str) -> AfterElement {
        match tag {
            "q" => {
                self.push_word("”");
                AfterElement::Nothing
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "main" | "article" | "section"
            | "aside" | "figure" | "fieldset" | "form" | "details" | "center" | "hgroup"
            | "address" | "pre" | "blockquote" | "dl" | "table" => AfterElement::BlockSpace,
            // Only the outermost list is separated from the text around it
            "ul" | "ol" | "menu" if self.list_stack.len() <= 1 => AfterElement::BlockSpace,
            "li" => {
                self.marker_pending = false;
                AfterElement::EndLine
            }
            "ul" | "ol" | "menu" | "div" | "header" | "footer" | "nav" | "legend" | "search"
            | "figcaption" | "summary" | "dt" | "dd" | "tr" => AfterElement::EndLine,
            _ => AfterElement::Nothing,
        }
    }

    fn ordered_list(element: ElementRef) -> ListKind {
        let elem = element.value();
        let items: Vec<ElementRef> = element
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|child| child.value().name() == "li")
            .collect();
        let count = items.len().max(1) as i64;
        let reversed = elem.attr("reversed").is_some();
        let step: i64 = if reversed { -1 } else { 1 };
        let start = elem
            .attr("start")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(if reversed { count } else { 1 });
        let numbering = match elem.attr("type") {
            Some("a") => Numbering::LowerAlpha,
            Some("A") => Numbering::UpperAlpha,
            Some("i") => Numbering::LowerRoman,
            Some("I") => Numbering::UpperRoman,
            _ => Numbering::Decimal,
        };
        // Number the items as start_list_item will, including value= jumps, to find the widest
        let mut next = start;
        let mut marker_width = 2;
        for item in &items {
            if let Some(value) = item.value().attr("value").and_then(|v| v.trim().parse::<i64>().ok()) {
                next = value;
            }
            marker_width = marker_width.max(format_number(next, numbering).len() + 1);
            next = next.saturating_add(step);
        }
        ListKind::Ordered { next: start, step, numbering, marker_width }
    }

    fn start_list_item(&mut self, elem: &scraper::node::Element) {
        self.marker_pending = false;
        self.flush_line();
        let unordered_depth = self
            .list_stack
            .iter()
            .filter(|kind| matches!(kind, ListKind::Unordered))
            .count();
        let marker = match self.list_stack.last_mut() {
            Some(ListKind::Ordered { next, step, numbering, marker_width }) => {
                if let Some(value) = elem.attr("value").and_then(|v| v.trim().parse::<i64>().ok()) {
                    *next = value;
                }
                let label = format!("{}.", format_number(*next, *numbering));
                *next = next.saturating_add(*step);
                format!("{:>width$} ", label, width = *marker_width)
            }
            _ => format!("{} ", BULLETS[unordered_depth.saturating_sub(1) % BULLETS.len()]),
        };
        self.apply_prefix();
        let style = self.current_style();
        self.emit(&marker, style, None);
        self.line_has_text = true;
        self.marker_pending = true;
        // Wrapped lines of the item line up with its text, after the marker
        self.prefix_stack.push(LinePrefix::Indent(text_width(&marker)));
    }

    /// With no JavaScript, <noscript> content is what the page wants shown. The parser treats it
    /// as raw text (as a scripting browser would), so parse that text as HTML.
    fn walk_noscript(&mut self, element: ElementRef) {
        if element.children().any(|child| child.value().is_element()) {
            for child in element.children() {
                self.walk(child);
            }
            return;
        }
        let markup: String = element.text().collect();
        let fragment = Html::parse_fragment(&markup);
        for child in fragment.root_element().children() {
            self.walk(child);
        }
    }
}
