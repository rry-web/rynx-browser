use crate::constants::UI_BORDER_WIDTH;
use crate::forms::{ControlKind, Form, FormControl, FormMethod, SelectOption};
use crate::models::LinkRegion;
use crate::network::PageKind;
use ego_tree::iter::Edge;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use scraper::{ElementRef, Html, Node, Selector};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use unicode_width::UnicodeWidthChar;

mod columns;
mod forms;
mod head;
mod images;
mod menus;
mod plain;
mod reader;
mod tables;

use menus::{looks_like_menu, menu_items};

/// Make untrusted text safe to draw. Control characters are dropped because a terminal acts on
/// them (escape sequences can set the title, clear the screen or write the clipboard) instead of
/// displaying them. Tabs become spaces so they can't move the cursor. Invisible direction
/// controls are dropped too: they reorder what's shown ("evil\u{202e}fdp.exe" shows as
/// "evilexe.pdf"), which terminals rarely need and pages use to disguise names.
pub fn sanitize_for_terminal(text: &str) -> Cow<'_, str> {
    if !text.contains(|c: char| c.is_control() || is_direction_control(c)) {
        return Cow::Borrowed(text);
    }
    let mut clean = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\t' {
            clean.push_str("    ");
        } else if !ch.is_control() && !is_direction_control(ch) {
            clean.push(ch);
        }
    }
    Cow::Owned(clean)
}

/// Unicode's invisible controls of text direction (embeddings, overrides, isolates, marks)
fn is_direction_control(ch: char) -> bool {
    matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}')
}

/// Width of a rendered line as drawn: one cell per character's width. This can differ from
/// Line::width(), which measures whole strings (an emoji joined with ZWJ is width 2 as a string
/// but is drawn, and laid out by the renderer, character by character).
pub fn display_width(line: &Line) -> usize {
    line.spans.iter().flat_map(|span| span.content.chars()).map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

fn text_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Everything needed to lay out a page, so it can run away from the UI thread
pub struct RenderJob {
    pub source: Arc<str>,
    pub kind: PageKind,
    pub source_view: bool,
    /// Show only the page's main text (reader mode)
    pub reader: bool,
    /// Images loaded so far, by their address as written on the page: the columns and rows to
    /// reserve for each, to draw it in the page
    pub image_sizes: Arc<HashMap<String, (u16, u16)>>,
    /// Terminal width; the page's border is allowed for
    pub width: u16,
}

/// A laid-out page, ready to draw
pub struct RenderedPage {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<LinkRegion>,
    pub anchors: HashMap<String, usize>,
    /// The document's <title>, for HTML pages that have one
    pub title: Option<String>,
    pub forms: Vec<Form>,
    pub controls: Vec<FormControl>,
    /// In reader mode, whether a main text was found (otherwise the whole page is shown)
    pub reader_found: Option<bool>,
    /// Every image on the page, and where space was reserved to draw the loaded ones
    pub images: Vec<PageImage>,
    pub image_blocks: Vec<ImageBlock>,
}

/// An image on the page: its address as written, and its description
#[derive(Clone, Debug, PartialEq)]
pub struct PageImage {
    pub src: String,
    pub alt: String,
}

/// Space reserved in the page to draw a loaded image
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageBlock {
    /// Index into the page's images
    pub image: usize,
    /// The block's first line, and its column
    pub line_index: usize,
    pub x: usize,
    pub columns: u16,
    pub rows: u16,
}

/// Parse and lay out a page. Pure and thread-safe: the UI runs it on a background thread, so a
/// page that is slow to parse never freezes input or drawing.
pub fn render_page(job: &RenderJob) -> RenderedPage {
    if job.source_view {
        let lines = job.source.lines().map(|l| Line::from(sanitize_for_terminal(l).into_owned())).collect();
        return RenderedPage {
            lines,
            links: Vec::new(),
            anchors: HashMap::new(),
            title: None,
            forms: Vec::new(),
            controls: Vec::new(),
            reader_found: None,
            images: Vec::new(),
            image_blocks: Vec::new(),
        };
    }
    let mut renderer = DomRenderer::new(job.width as usize);
    renderer.image_sizes = Arc::clone(&job.image_sizes);
    let mut reader_found = None;
    let title = match job.kind {
        PageKind::Html => {
            let document = Html::parse_document(&job.source);
            if job.reader {
                reader_found = Some(renderer.render_reader(&document));
            } else {
                renderer.render(&document);
            }
            document_title(&document)
        }
        PageKind::PlainText => {
            renderer.render_plain_text(&job.source);
            None
        }
    };
    RenderedPage {
        lines: renderer.lines,
        links: renderer.links,
        anchors: renderer.anchors,
        title,
        forms: renderer.forms,
        controls: renderer.controls,
        reader_found,
        images: renderer.images,
        image_blocks: renderer.image_blocks,
    }
}

/// The text of the document's <title>, whitespace collapsed
fn document_title(document: &Html) -> Option<String> {
    static TITLE: OnceLock<Selector> = OnceLock::new();
    let selector = TITLE.get_or_init(|| Selector::parse("title").expect("valid selector"));
    let title = document.select(selector).next()?.text().collect::<String>();
    let title = title.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    (!title.is_empty()).then_some(title)
}

/// Elements whose content is never shown
const SKIPPED_TAGS: &[&str] = &[
    "script", "style", "head", "meta", "link", "title", "template", "svg", "object", "embed", "param", "source",
    "track",
];

const BULLETS: &[&str] = &["•", "◦", "▪"];

/// Elements nested deeper than this are rendered as plain text. Rendering recurses once per
/// level, and ~10,000 nested <div>s (60KB of HTML) used to overflow the stack and crash.
const MAX_NESTING: usize = 256;

/// The deepest data-rynx-indent drawn, so a page can't push its text off the screen
const MAX_RYNX_INDENT: usize = 40;

/// Drawn at the start of every line inside a block: list indentation, quote bars
#[derive(Clone, Copy)]
enum LinePrefix {
    Indent(usize),
    QuoteBar,
}

/// Where a line sits when it's narrower than the page
#[derive(Clone, Copy, PartialEq)]
enum Align {
    Center,
    Right,
}

/// A colour from rynx's own markup (Micron's): three hex digits ("79d", 12-bit) or six ("7799dd").
/// Drawn from the 256-colour palette, as NomadNet does in 256-colour terminals, so it shows the
/// same in terminals without true colour.
fn palette_color(hex: &str) -> Option<Color> {
    let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    let [r, g, b] = match digits.as_slice() {
        [r, g, b] => [r * 17, g * 17, b * 17],
        [r1, r2, g1, g2, b1, b2] => [r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2],
        _ => return None,
    };
    // Greys use the palette's finer grey ramp; colours its 6×6×6 cube
    if r == g && g == b {
        return Some(match r {
            0..=4 => Color::Indexed(16),
            251..=255 => Color::Indexed(231),
            // The ramp's greys are 8, 18, ... 238: the nearest one
            level => Color::Indexed(232 + (((level as u16).saturating_sub(8) + 5) / 10).min(23) as u8),
        });
    }
    let level = |v: u8| ((v as u16 * 5 + 127) / 255) as u8;
    Some(Color::Indexed(16 + 36 * level(r) + 6 * level(g) + level(b)))
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
            (1000, "m"),
            (900, "cm"),
            (500, "d"),
            (400, "cd"),
            (100, "c"),
            (90, "xc"),
            (50, "l"),
            (40, "xl"),
            (10, "x"),
            (9, "ix"),
            (5, "v"),
            (4, "iv"),
            (1, "i"),
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
    pub forms: Vec<Form>,
    pub controls: Vec<FormControl>,
    /// The <form> being rendered, which new controls belong to
    current_form: Option<usize>,
    /// Forms by id, for controls that name their form with form="id"
    form_ids: HashMap<String, usize>,
    /// The control (e.g. a <button>) whose content is being drawn
    active_control: Option<usize>,
    /// Above zero while drawing something that stays on one line (a button's content, a menu
    /// item holding a form control): blocks inside it don't start new lines
    inline_depth: usize,
    /// Menus already drawn (their entries), so a site's mobile and desktop copies of the same menu
    /// appear once
    menus_drawn: std::collections::HashSet<Vec<(String, Option<String>)>>,
    /// Reader mode: clutter inside the main text is left out
    reader: bool,
    /// Words drawn so far, and the last image's label with the count when it was drawn: an image
    /// repeated right after itself (a site's copies for different screen sizes) is drawn once
    words_drawn: usize,
    last_image: Option<(String, usize)>,
    /// Lines of preformatted text, which may legitimately repeat
    preformatted_lines: std::collections::HashSet<usize>,
    /// Lines centred or right-aligned (data-rynx-align, for Micron pages), innermost last
    align_stack: Vec<Align>,
    /// The page's images, and the blocks reserved for loaded ones
    pub images: Vec<PageImage>,
    pub image_blocks: Vec<ImageBlock>,
    /// Sizes of the images loaded so far (see RenderJob)
    image_sizes: Arc<HashMap<String, (u16, u16)>>,
    /// The image whose placeholder is being drawn
    active_image: Option<usize>,
    /// The row of images being laid out side by side (a gallery), while nothing but images follow
    image_row: Option<images::ImageRow>,
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
            forms: Vec::new(),
            controls: Vec::new(),
            current_form: None,
            form_ids: HashMap::new(),
            active_control: None,
            inline_depth: 0,
            menus_drawn: std::collections::HashSet::new(),
            reader: false,
            words_drawn: 0,
            last_image: None,
            preformatted_lines: std::collections::HashSet::new(),
            align_stack: Vec::new(),
            images: Vec::new(),
            image_blocks: Vec::new(),
            image_sizes: Arc::default(),
            active_image: None,
            image_row: None,
        }
    }

    pub fn render(&mut self, document: &Html) {
        self.render_tools_link(document);
        self.render_feed_links(document);
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

    /// Lay out something that places its own lines (tables, menus), even inside a one-line
    /// element: its line numbers for links and jump targets rely on its lines really ending
    fn outside_one_line(&mut self, layout: impl FnOnce(&mut Self)) {
        let saved = std::mem::take(&mut self.inline_depth);
        layout(self);
        self.inline_depth = saved;
    }

    /// End the current line where a block ends; on a one-line element, just a space
    fn flush_line(&mut self) {
        if self.inline_depth > 0 {
            self.note_space();
            return;
        }
        self.end_line();
    }

    /// End the current line, always: for wrapping
    fn end_line(&mut self) {
        if self.marker_pending {
            return;
        }
        if self.line_has_text {
            if self.preserve_whitespace {
                self.preformatted_lines.insert(self.lines.len());
            }
            let align = self.align_stack.last().copied();
            if let Some(align) = align.filter(|_| self.current_line_width < self.max_width) {
                // Pad after the line's prefix (its indent), and move the line's links with it
                let room = self.max_width - self.current_line_width;
                let pad = if align == Align::Center { room / 2 } else { room };
                let at = self.current_line.len().min(self.prefix_spans().len());
                self.current_line.insert(at, Span::raw(" ".repeat(pad)));
                let line_index = self.lines.len();
                for link in self.links.iter_mut().filter(|l| l.line_index == line_index) {
                    link.x_start += pad;
                    link.x_end += pad;
                }
            }
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
        if self.inline_depth > 0 {
            return self.note_space();
        }
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
        if self.inline_depth > 0 {
            return self.note_space();
        }
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
        self.emit_to(text, style, link, None);
    }

    /// Append text that is a link or form control (or neither), tracking its clickable region
    fn emit_to(&mut self, text: &str, style: Style, link: Option<&str>, control: Option<usize>) {
        for ch in text.chars() {
            // The space before an image's placeholder isn't part of it
            let image = if ch.is_whitespace() { None } else { self.active_image };
            let target = link.or(control.or(image).map(|_| ""));
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

            if let Some(url) = target {
                let line_index = self.lines.len();
                // Merge with the previous region if it's the same link and contiguous
                if let Some(last) = self.links.last_mut()
                    && last.line_index == line_index
                    && last.url == url
                    && last.control == control
                    && last.image == image
                    && last.x_end == start_x
                {
                    last.x_end = end_x;
                    continue;
                }
                // A zero-width character (combining mark, zero-width space) can't start a
                // clickable region on its own
                if width == 0 {
                    continue;
                }
                self.links.push(LinkRegion {
                    url: url.to_string(),
                    control,
                    image,
                    line_index,
                    x_start: start_x,
                    x_end: end_x,
                });
            }
        }
    }

    /// Append content in the current style and link
    fn emit_text(&mut self, text: &str) {
        // Inside a form control, the control is what gets activated, not a surrounding link
        let control = self.active_control;
        let link = if control.is_some() { None } else { self.active_link_url.clone() };
        self.emit_to(text, self.current_style(), link.as_deref(), control);
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
        self.words_drawn += 1;
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
            self.end_line();
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
            self.end_line();
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
                self.end_line();
            }
        }
    }

    /// Preformatted text: spaces are kept, and long lines wrap at the edge of the screen
    fn push_preformatted(&mut self, text: &str) {
        for ch in text.chars() {
            self.apply_prefix();
            if self.line_has_text && self.current_line_width + char_width(ch) > self.max_width {
                self.end_line();
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
        if SKIPPED_TAGS.contains(&tag) || is_hidden(elem) || (self.reader && reader::is_clutter(elem)) {
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
                // A Micron divider: its own character, and no space around it
                if let Some(glyph) = elem.attr("data-rynx-rule").and_then(|g| g.chars().next()) {
                    self.flush_line();
                    self.apply_prefix();
                    let width = self.max_width.saturating_sub(self.current_line_width);
                    let glyph = if glyph.is_control() || char_width(glyph) != 1 { '─' } else { glyph };
                    self.emit_text(&glyph.to_string().repeat(width));
                    self.line_has_text = true;
                    return self.flush_line();
                }
                self.add_vertical_space();
                self.apply_prefix();
                let rule = "─".repeat(self.max_width.saturating_sub(self.current_line_width));
                self.emit_text(&rule);
                return self.add_vertical_space();
            }
            "img" => return self.render_image(elem),
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
            "ul" | "ol" | "menu" if looks_like_menu(element) => {
                if let Some(items) = menu_items(element, 2) {
                    if let Some(id) = elem.attr("id") {
                        self.anchors.entry(id.to_string()).or_insert(self.lines.len());
                    }
                    // Only flat menus of links count as repeats: submenus and controls can differ
                    // behind the same labels
                    let flat = items.iter().all(|i| i.submenu.is_none() && i.with_control.is_none());
                    let entries = items.iter().map(|i| (i.label.clone(), i.href.clone())).collect();
                    if flat && !self.menus_drawn.insert(entries) {
                        return;
                    }
                    return self.outside_one_line(|r| {
                        r.add_vertical_space();
                        r.render_menu(&items, None, 0);
                        r.add_vertical_space();
                    });
                }
            }
            "input" => return self.render_input(elem),
            "textarea" => return self.render_textarea(element),
            "select" => return self.render_select(element),
            "table" => return self.outside_one_line(|r| r.render_table(element)),
            _ if elem.attr(columns::COLUMNS_ATTRIBUTE).is_some() => {
                let count = self.column_count(element);
                if count > 1 {
                    return self.outside_one_line(|r| r.render_columns(element, count));
                }
            }
            _ => {}
        }

        let saved_link = self.active_link_url.clone();
        let saved_preserve = self.preserve_whitespace;
        let saved_styles = self.style_stack.len();
        let saved_prefixes = self.prefix_stack.len();
        let saved_lists = self.list_stack.len();
        let saved_form = self.current_form;
        let saved_control = self.active_control;
        let saved_align = self.align_stack.len();

        self.open_element(element);
        self.open_rynx_markup(elem);

        // Record link targets after any block spacing, so jumps land on the content
        let anchor = elem.attr("id").or_else(|| if tag == "a" { elem.attr("name") } else { None });
        if let Some(name) = anchor {
            // First occurrence wins, as in browsers
            self.anchors.entry(name.to_string()).or_insert(self.lines.len());
        }

        // A button's content stays on one line, as in browsers, whatever blocks it holds
        let one_line = tag == "button";
        self.inline_depth += usize::from(one_line);
        self.depth += 1;
        for child in element.children() {
            self.walk(child);
        }
        self.depth -= 1;
        self.inline_depth -= usize::from(one_line);

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
        self.current_form = saved_form;
        self.active_control = saved_control;
        self.align_stack.truncate(saved_align);

        if after == AfterElement::BlockSpace {
            self.add_vertical_space();
        }
    }

    /// rynx's own markup, for pages it makes from other formats (Micron): data-rynx-indent (columns),
    /// data-rynx-align (center or right), and data-rynx-fg / data-rynx-bg (Micron colours). Named
    /// for rynx, so websites' own attributes change nothing.
    fn open_rynx_markup(&mut self, elem: &scraper::node::Element) {
        if let Some(columns) = elem.attr("data-rynx-indent").and_then(|n| n.parse::<usize>().ok()) {
            self.flush_line();
            self.prefix_stack.push(LinePrefix::Indent(columns.min(MAX_RYNX_INDENT)));
        }
        if let Some(align) = elem.attr("data-rynx-align") {
            self.flush_line();
            match align {
                "center" => self.align_stack.push(Align::Center),
                "right" => self.align_stack.push(Align::Right),
                _ => {}
            }
        }
        let fg = elem.attr("data-rynx-fg").and_then(palette_color);
        let bg = elem.attr("data-rynx-bg").and_then(palette_color);
        if fg.is_some() || bg.is_some() {
            self.add_style(|mut s| {
                if let Some(fg) = fg {
                    s = s.fg(fg);
                }
                if let Some(bg) = bg {
                    s = s.bg(bg);
                }
                s
            });
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
            "form" => {
                self.add_vertical_space();
                let index = self.forms.len();
                let method = match elem.attr("method") {
                    Some(m) if m.trim().eq_ignore_ascii_case("post") => FormMethod::Post,
                    _ => FormMethod::Get,
                };
                self.forms.push(Form { action: elem.attr("action").map(str::to_string), method });
                if let Some(id) = elem.attr("id") {
                    self.form_ids.entry(id.to_string()).or_insert(index);
                }
                self.current_form = Some(index);
            }
            "button" => {
                // Only submit buttons do anything without JavaScript
                let kind = elem.attr("type").unwrap_or("submit").trim().to_ascii_lowercase();
                if kind == "submit" {
                    let label: String =
                        element.text().collect::<String>().split_ascii_whitespace().collect::<Vec<_>>().join(" ");
                    let control = self.add_control(
                        elem,
                        FormControl {
                            form: None,
                            kind: ControlKind::Submit,
                            name: elem.attr("name").unwrap_or("").to_string(),
                            value: elem.attr("value").unwrap_or("").to_string(),
                            checked: false,
                            selected: 0,
                            width: 0,
                            label: sanitize_for_terminal(&label).into_owned(),
                            max_length: None,
                        },
                    );
                    self.active_control = control;
                    if control.is_some() {
                        self.add_style(|s| s.fg(Color::Yellow));
                        self.push_word("[");
                    } else {
                        self.add_style(|s| s.add_modifier(Modifier::DIM));
                    }
                } else {
                    self.add_style(|s| s.add_modifier(Modifier::DIM));
                }
            }
            "p" | "main" | "article" | "section" | "aside" | "figure" | "fieldset" | "details" | "center"
            | "hgroup" | "dl" => self.add_vertical_space(),
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
                let kind = if elem.name() == "ol" { Self::ordered_list(element) } else { ListKind::Unordered };
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
            "button" => {
                if self.active_control.is_some() {
                    self.push_word("]");
                }
                AfterElement::Nothing
            }
            "form" => AfterElement::BlockSpace,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "main" | "article" | "section" | "aside" | "figure"
            | "fieldset" | "details" | "center" | "hgroup" | "address" | "pre" | "blockquote" | "dl" => {
                AfterElement::BlockSpace
            }
            // Only the outermost list is separated from the text around it
            "ul" | "ol" | "menu" if self.list_stack.len() <= 1 => AfterElement::BlockSpace,
            "li" => {
                self.marker_pending = false;
                AfterElement::EndLine
            }
            "ul" | "ol" | "menu" | "div" | "header" | "footer" | "nav" | "legend" | "search" | "figcaption"
            | "summary" | "dt" | "dd" | "tr" => AfterElement::EndLine,
            _ => AfterElement::Nothing,
        }
    }

    fn ordered_list(element: ElementRef) -> ListKind {
        let elem = element.value();
        let items: Vec<ElementRef> =
            element.children().filter_map(ElementRef::wrap).filter(|child| child.value().name() == "li").collect();
        let count = items.len().max(1) as i64;
        let reversed = elem.attr("reversed").is_some();
        let step: i64 = if reversed { -1 } else { 1 };
        let start =
            elem.attr("start").and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(if reversed { count } else { 1 });
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
        let unordered_depth = self.list_stack.iter().filter(|kind| matches!(kind, ListKind::Unordered)).count();
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
