use crate::constants::UI_BORDER_WIDTH;
use crate::models::LinkRegion;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ego_tree::iter::Edge;
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

/// Byte ranges of http(s) URLs in a line of plain text, without trailing punctuation
fn find_urls(line: &str) -> Vec<(usize, usize)> {
    let mut urls = Vec::new();
    let mut from = 0;
    while let Some(found) = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| line[from..].find(scheme))
        .min()
    {
        let start = from + found;
        let mut end = line[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
            .map_or(line.len(), |i| start + i);
        // "see https://example.com/x." ends the sentence, not the URL; a closing bracket only
        // belongs to the URL if the URL opened it
        loop {
            let url = &line[start..end];
            let trim = match url.chars().last() {
                Some('.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*') => true,
                Some(')') => url.matches('(').count() < url.matches(')').count(),
                Some(']') => url.matches('[').count() < url.matches(']').count(),
                _ => false,
            };
            if !trim {
                break;
            }
            end -= 1;
        }
        if end - start > "https://".len() {
            urls.push((start, end));
        }
        from = end.max(start + 1);
    }
    urls
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

const COLUMN_SEPARATOR: &str = " │ ";
const COLUMN_SEPARATOR_WIDTH: usize = 3;
const MAX_COLSPAN: usize = 64;
/// A long word (e.g. a URL) can't force its column wider than this; it is wrapped instead
const MAX_MIN_COLUMN_WIDTH: usize = 20;

struct TableCell<'a> {
    element: ElementRef<'a>,
    header: bool,
    colspan: usize,
}

struct TableRow<'a> {
    cells: Vec<TableCell<'a>>,
    header: bool,
}

/// A cell rendered at its column width, ready to be placed into table lines
struct RenderedCell {
    column: usize,
    lines: Vec<Line<'static>>,
    links: Vec<LinkRegion>,
    anchors: Vec<(String, usize)>,
}

/// The words a cell would show, each with the indentation it would be drawn at, for measuring
/// columns without rendering. Includes image alt text and nested table separators, and skips
/// content that isn't drawn.
fn cell_words(cell: ElementRef) -> Vec<(String, usize)> {
    fn indent_of(name: &str) -> usize {
        match name {
            "li" | "dd" => 4, // list indent and marker, or definition indent
            "blockquote" => 2,
            _ => 0,
        }
    }
    let mut words = Vec::new();
    let mut skip_depth = 0;
    let mut indent = 0;
    let mut link_depth = 0;
    for edge in cell.traverse() {
        match edge {
            Edge::Open(node) => match node.value() {
                Node::Element(elem) => {
                    if skip_depth > 0 || SKIPPED_TAGS.contains(&elem.name()) || is_hidden(elem) {
                        skip_depth += 1;
                        continue;
                    }
                    indent += indent_of(elem.name());
                    if elem.name() == "a" && elem.attr("href").is_some() {
                        link_depth += 1;
                    }
                    // The cell itself isn't a separator, only cells of tables nested inside it
                    let follows_cell = node.id() != cell.id()
                        && node.prev_siblings().any(|s| {
                            s.value().as_element().is_some_and(|e| matches!(e.name(), "td" | "th"))
                        });
                    if matches!(elem.name(), "td" | "th") && follows_cell {
                        // A nested table's column separator
                        words.push(("│".to_string(), indent));
                    } else if elem.name() == "img" {
                        // Mirrors how walk_element labels images
                        match elem.attr("alt").map(str::trim) {
                            Some("") => {}
                            Some(alt) => words.push((format!("[{}]", alt), indent)),
                            None if link_depth > 0 => words.push(("[image]".to_string(), indent)),
                            None => {}
                        }
                    }
                }
                Node::Text(text) if skip_depth == 0 => {
                    words.extend(text.split_ascii_whitespace().map(|w| (w.to_string(), indent)));
                }
                _ => {}
            },
            Edge::Close(node) => {
                if let Some(elem) = node.value().as_element() {
                    if skip_depth > 0 {
                        skip_depth -= 1;
                    } else {
                        indent -= indent_of(elem.name());
                        if elem.name() == "a" && elem.attr("href").is_some() {
                            link_depth -= 1;
                        }
                    }
                }
            }
        }
    }
    words
}

/// (natural, minimum) width of a cell: all its words on one line, and its widest word (capped)
fn measure_cell(cell: ElementRef) -> (usize, usize) {
    let words = cell_words(cell);
    let natural = words.iter().map(|(w, _)| text_width(w)).sum::<usize>()
        + words.len().saturating_sub(1)
        + words.iter().map(|(_, indent)| *indent).max().unwrap_or(0);
    let minimum = words
        .iter()
        .map(|(w, indent)| text_width(w).min(MAX_MIN_COLUMN_WIDTH) + indent)
        .max()
        .unwrap_or(0);
    (natural, minimum)
}

/// The rows of a table, excluding rows of tables nested inside its cells
fn table_rows<'a>(table: ElementRef<'a>) -> (Option<ElementRef<'a>>, Vec<TableRow<'a>>) {
    let mut caption = None;
    let mut rows = Vec::new();
    let mut add_row = |row: ElementRef<'a>, in_head: bool| {
        if is_hidden(row.value()) {
            return;
        }
        let cells: Vec<TableCell<'a>> = row
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|cell| matches!(cell.value().name(), "td" | "th") && !is_hidden(cell.value()))
            .map(|cell| TableCell {
                element: cell,
                header: cell.value().name() == "th",
                colspan: cell
                    .value()
                    .attr("colspan")
                    .and_then(|n| n.trim().parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, MAX_COLSPAN),
            })
            .collect();
        if !cells.is_empty() {
            let header = in_head || cells.iter().all(|cell| cell.header);
            rows.push(TableRow { cells, header });
        }
    };
    for child in table.children().filter_map(ElementRef::wrap) {
        match child.value().name() {
            "caption" if caption.is_none() => caption = Some(child),
            "tr" => add_row(child, false),
            "thead" | "tbody" | "tfoot" => {
                let in_head = child.value().name() == "thead";
                for row in child.children().filter_map(ElementRef::wrap) {
                    if row.value().name() == "tr" {
                        add_row(row, in_head);
                    }
                }
            }
            _ => {}
        }
    }
    (caption, rows)
}

/// Share `available` columns between table columns. Each gets at least its longest word (capped);
/// the rest goes to the columns wanting the least first, so short columns ("12 KB") stay on one
/// line and long text takes what's left. None if even the minimums don't fit.
fn column_widths(natural: &[usize], minimum: &[usize], available: usize) -> Option<Vec<usize>> {
    if natural.iter().sum::<usize>() <= available {
        return Some(natural.to_vec());
    }
    let min_total: usize = minimum.iter().sum();
    if min_total > available {
        return None;
    }
    let mut extra = available - min_total;
    let wants: Vec<usize> = natural.iter().zip(minimum).map(|(n, m)| n.saturating_sub(*m)).collect();
    let mut widths = minimum.to_vec();
    let mut order: Vec<usize> = (0..wants.len()).filter(|&c| wants[c] > 0).collect();
    order.sort_by_key(|&c| wants[c]);
    let mut remaining = order.len();
    for column in order {
        let give = wants[column].min(extra / remaining);
        widths[column] += give;
        extra -= give;
        remaining -= 1;
    }
    Some(widths)
}

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
    /// The link of the last word drawn on the current line
    last_word_link: Option<String>,
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
            last_word_link: None,
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

    /// Render plain text (text/plain, JSON, source code...). Lines and indentation are kept, long
    /// lines wrap at spaces and continue under their own indentation, and URLs become links.
    pub fn render_plain_text(&mut self, text: &str) {
        for raw_line in text.lines() {
            let line = sanitize_for_terminal(raw_line);
            let line = line.trim_end();
            if line.is_empty() {
                self.flush_line();
                self.push_blank_line();
            } else {
                self.push_plain_line(line);
            }
        }
        self.flush_line();
    }

    fn push_plain_line(&mut self, line: &str) {
        let urls = find_urls(line);
        let indent = line.len() - line.trim_start_matches(' ').len();
        // Continuation lines start under the text, but never more than halfway across
        let hanging = indent.min(self.max_width / 2);
        let mut pos = 0;
        let mut first = true;
        while pos < line.len() {
            self.flush_line();
            let lead = if first { 0 } else { hanging };
            if lead > 0 {
                self.emit(&" ".repeat(lead), Style::default(), None);
            }
            let available = self.max_width.saturating_sub(lead).max(1);

            // The most that fits, then back up to the last space if the line continues
            let mut used = 0;
            let mut fit_end = pos;
            for (i, ch) in line[pos..].char_indices() {
                if used + char_width(ch) > available && fit_end > pos {
                    break;
                }
                used += char_width(ch);
                fit_end = pos + i + ch.len_utf8();
            }
            let mut end = fit_end;
            if fit_end < line.len() {
                let chunk = &line[pos..fit_end];
                let leading_spaces = chunk.len() - chunk.trim_start_matches(' ').len();
                if let Some(space) = chunk.rfind(' ').filter(|&i| i > leading_spaces) {
                    end = pos + space;
                }
            }

            self.emit_plain(line, pos, end, &urls);
            self.line_has_text = true;
            pos = end;
            // Continuation lines get the hanging indent instead of the spaces at the break
            while line[pos..].starts_with(' ') {
                pos += 1;
            }
            first = false;
        }
        self.flush_line();
    }

    /// Draw line[start..end], making the parts inside URL ranges links to those URLs
    fn emit_plain(&mut self, line: &str, start: usize, end: usize, urls: &[(usize, usize)]) {
        let link_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
        let mut pos = start;
        for &(url_start, url_end) in urls {
            if url_end <= pos || url_start >= end {
                continue;
            }
            let link_from = url_start.max(pos);
            let link_to = url_end.min(end);
            self.emit(&line[pos..link_from], Style::default(), None);
            self.emit(&line[link_from..link_to], link_style, Some(&line[url_start..url_end]));
            pos = link_to;
        }
        self.emit(&line[pos..end], Style::default(), None);
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
        self.last_word_link = None;
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
        // Two different links with nothing between them (sites space them with CSS) would read
        // as one; separate them
        if self.pending_space.is_none()
            && self.active_link_url.is_some()
            && self.last_word_link.is_some()
            && self.active_link_url != self.last_word_link
        {
            self.pending_space = Some((Style::default(), None));
        }
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
        self.last_word_link = self.active_link_url.clone();
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
            "table" => return self.render_table(element),
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
            | "details" | "center" | "hgroup" | "dl" => self.add_vertical_space(),
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
            | "address" | "pre" | "blockquote" | "dl" => AfterElement::BlockSpace,
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

    // --- Tables ---

    fn render_table(&mut self, table: ElementRef) {
        let (caption, rows) = table_rows(table);
        self.add_vertical_space();
        if let Some(id) = table.value().attr("id") {
            self.anchors.entry(id.to_string()).or_insert(self.lines.len());
        }
        if rows.is_empty() {
            // Not really a table (e.g. text directly inside <table>); show what's there
            for child in table.children() {
                self.walk(child);
            }
            return self.add_vertical_space();
        }
        if let Some(caption) = caption {
            self.add_style(|s| s.add_modifier(Modifier::ITALIC));
            for child in caption.children() {
                self.walk(child);
            }
            self.style_stack.pop();
            self.flush_line();
        }

        let columns = rows
            .iter()
            .map(|row| row.cells.iter().map(|cell| cell.colspan).sum::<usize>())
            .max()
            .unwrap_or(1);
        let prefix_width: usize = self.prefix_spans().iter().map(|s| text_width(&s.content)).sum();
        let separators = COLUMN_SEPARATOR_WIDTH * (columns - 1);
        let available = self.max_width.saturating_sub(prefix_width);

        // Measure from cells that occupy a single column; spanning cells wrap within their columns
        let mut natural = vec![0; columns];
        let mut minimum = vec![0; columns];
        for row in &rows {
            let mut column = 0;
            for cell in &row.cells {
                if cell.colspan == 1 {
                    let (wanted, least) = measure_cell(cell.element);
                    natural[column] = natural[column].max(wanted.min(available));
                    minimum[column] = minimum[column].max(least);
                }
                column += cell.colspan;
            }
        }

        match available
            .checked_sub(separators)
            .and_then(|space| column_widths(&natural, &minimum, space))
        {
            Some(widths) => self.render_table_grid(&rows, &widths),
            None => self.render_table_stacked(&rows),
        }
        self.add_vertical_space();
    }

    fn render_table_grid(&mut self, rows: &[TableRow], widths: &[usize]) {
        let column_x = |column: usize| -> usize {
            widths[..column].iter().sum::<usize>() + COLUMN_SEPARATOR_WIDTH * column
        };
        let separator_style = Style::default().fg(Color::DarkGray);
        let mut header_rule_drawn = false;

        for (row_index, row) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            let mut column = 0;
            for cell in &row.cells {
                if column >= widths.len() {
                    break;
                }
                let span = cell.colspan.min(widths.len() - column);
                let width = widths[column..column + span].iter().sum::<usize>()
                    + COLUMN_SEPARATOR_WIDTH * (span - 1);
                cells.push(self.render_cell(cell, column, width));
                column += span;
            }

            let row_start = self.lines.len();
            let height = cells.iter().map(|cell| cell.lines.len()).max().unwrap_or(0).max(1);
            for k in 0..height {
                self.flush_line();
                self.apply_prefix();
                let base = self.current_line_width;
                for (i, cell) in cells.iter().enumerate() {
                    let x = base + column_x(cell.column);
                    if i > 0 {
                        let pad = (x - COLUMN_SEPARATOR_WIDTH).saturating_sub(self.current_line_width);
                        self.emit(&" ".repeat(pad), Style::default(), None);
                        self.emit(COLUMN_SEPARATOR, separator_style, None);
                    }
                    if let Some(line) = cell.lines.get(k) {
                        let x = self.current_line_width;
                        for span in &line.spans {
                            self.emit(&span.content, span.style, None);
                        }
                        let line_index = self.lines.len();
                        for link in cell.links.iter().filter(|l| l.line_index == k) {
                            self.links.push(LinkRegion {
                                url: link.url.clone(),
                                line_index,
                                x_start: x + link.x_start,
                                x_end: x + link.x_end,
                            });
                        }
                    }
                }
                self.line_has_text = true;
                self.marker_pending = false;
                self.flush_line();
            }
            for cell in &cells {
                for (name, line) in &cell.anchors {
                    self.anchors.entry(name.clone()).or_insert(row_start + line);
                }
            }
            for cell in &row.cells {
                if let Some(id) = cell.element.value().attr("id") {
                    self.anchors.entry(id.to_string()).or_insert(row_start);
                }
            }

            // A rule under the header rows, when data rows follow
            let next_is_data = rows.get(row_index + 1).is_some_and(|next| !next.header);
            if row.header && next_is_data && !header_rule_drawn {
                header_rule_drawn = true;
                self.apply_prefix();
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.emit(&rule.join("─┼─"), separator_style, None);
                self.line_has_text = true;
                self.flush_line();
            }
        }
    }

    /// Render one cell with its own renderer at the column's width
    fn render_cell(&self, cell: &TableCell, column: usize, width: usize) -> RenderedCell {
        let mut renderer = DomRenderer::with_content_width(width);
        renderer.depth = self.depth + 1;
        let mut style = self.current_style();
        if cell.header {
            style = style.add_modifier(Modifier::BOLD);
        }
        renderer.style_stack = vec![style];
        renderer.active_link_url = self.active_link_url.clone();
        for child in cell.element.children() {
            renderer.walk(child);
        }
        renderer.marker_pending = false;
        renderer.flush_line();

        // Blank lines at the edges of a cell (from <p> margins) would only pad the row
        let is_blank = |line: &Line| line.spans.iter().all(|s| s.content.trim().is_empty());
        let leading = renderer.lines.iter().take_while(|l| is_blank(l)).count();
        let trailing = renderer.lines.iter().rev().take_while(|l| is_blank(l)).count();
        let keep = renderer.lines.len().saturating_sub(leading + trailing);
        let lines: Vec<Line<'static>> = renderer.lines.into_iter().skip(leading).take(keep).collect();
        let links = renderer
            .links
            .into_iter()
            .filter(|l| l.line_index >= leading && l.line_index < leading + keep)
            .map(|l| LinkRegion { line_index: l.line_index - leading, ..l })
            .collect();
        let anchors = renderer
            .anchors
            .into_iter()
            .map(|(name, line)| (name, line.saturating_sub(leading).min(keep.saturating_sub(1))))
            .collect();
        RenderedCell { column, lines, links, anchors }
    }

    /// When the columns can't fit, show each row as lines, labelled with the header row if any
    fn render_table_stacked(&mut self, rows: &[TableRow]) {
        let headers: Vec<String> = match rows.first() {
            Some(first) if first.header && rows.len() > 1 => first
                .cells
                .iter()
                .map(|cell| {
                    let words: Vec<String> = cell_words(cell.element).into_iter().map(|(w, _)| w).collect();
                    sanitize_for_terminal(&words.join(" ")).into_owned()
                })
                .collect(),
            _ => Vec::new(),
        };
        let skip = usize::from(!headers.is_empty());
        for row in rows.iter().skip(skip) {
            for (i, cell) in row.cells.iter().enumerate() {
                self.flush_line();
                let saved_styles = self.style_stack.len();
                if let Some(header) = headers.get(i).filter(|h| !h.is_empty()) {
                    self.add_style(|s| s.add_modifier(Modifier::BOLD));
                    self.push_word(&format!("{}:", header));
                    self.style_stack.pop();
                    self.note_space();
                }
                if cell.header {
                    self.add_style(|s| s.add_modifier(Modifier::BOLD));
                }
                if let Some(id) = cell.element.value().attr("id") {
                    self.anchors.entry(id.to_string()).or_insert(self.lines.len());
                }
                self.depth += 1;
                for child in cell.element.children() {
                    self.walk(child);
                }
                self.depth -= 1;
                self.style_stack.truncate(saved_styles);
                self.flush_line();
            }
            self.add_vertical_space();
        }
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
