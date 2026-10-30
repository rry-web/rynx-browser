//! Micron, the markup of NomadNet pages on the Reticulum network, as HTML for rynx's renderer.
//!
//! It follows NomadNet's own parser (nomadnet/ui/textui/MicronParser.py): each source line is a
//! line on screen (wrapped, never joined with the next), formatting and colours carry from line to
//! line until changed or reset, and sections (`>`) indent what follows them. The HTML uses rynx's
//! own attributes for what HTML can't say simply: data-rynx-indent, data-rynx-align,
//! data-rynx-fg / data-rynx-bg and data-rynx-rule.
//!
//! Drawn as text for now: images and partials (parts of a page loaded separately) are links, and
//! fields show but can't be sent until rynx can reach Reticulum nodes. Collapsible sections are
//! shown open.

use crate::network::escape_html;

/// Columns each section level indents by, as in NomadNet
const SECTION_INDENT: usize = 2;

/// A field's width when the page doesn't give one
const DEFAULT_FIELD_WIDTH: usize = 24;

/// The widest field drawn, as in NomadNet
const MAX_FIELD_WIDTH: usize = 256;

/// A space that's kept where it is
const NBSP: char = '\u{a0}';

#[derive(Clone, Copy, PartialEq, Default)]
enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Formatting and colours, which carry over from line to line
#[derive(Clone, Default)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    fg: Option<String>,
    bg: Option<String>,
}

#[derive(Default)]
struct State {
    /// The section level: 0 outside any section
    depth: usize,
    align: Align,
    style: Style,
    /// Inside a `= block, shown as written
    literal: bool,
    /// Inside a `t block: its rows so far
    table: Option<Vec<String>>,
    title: Option<String>,
    html: String,
}

impl State {
    fn indent(&self) -> usize {
        self.depth.saturating_sub(1) * SECTION_INDENT
    }

    /// The attributes that place a line: its section's indent and its alignment
    fn line_attrs(&self) -> String {
        let mut attrs = String::new();
        if self.indent() > 0 {
            attrs.push_str(&format!(" data-rynx-indent=\"{}\"", self.indent()));
        }
        match self.align {
            Align::Left => {}
            Align::Center => attrs.push_str(" data-rynx-align=\"center\""),
            Align::Right => attrs.push_str(" data-rynx-align=\"right\""),
        }
        attrs
    }
}

/// Whether a page is Micron: NomadNet's pages end in .mu
pub fn is_micron(url: &str, content_type: Option<&str>) -> bool {
    if content_type.is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("text/micron")) {
        return true;
    }
    url::Url::parse(url).is_ok_and(|u| u.path().to_ascii_lowercase().ends_with(".mu"))
}

/// A Micron page as HTML, with its title (the first heading) if it has one
pub fn micron_to_html(source: &str) -> (Option<String>, String) {
    let mut state = State::default();
    for line in source.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line: String = line.chars().filter(|c| !c.is_control() || *c == '\t').collect();
        convert_line(&line, &mut state);
    }
    if state.literal {
        state.html.push_str("</pre></div>");
    }
    if let Some(rows) = state.table.take() {
        let table = table_html(&rows, &mut state);
        state.html.push_str(&table);
    }
    (state.title, state.html)
}

fn convert_line(line: &str, state: &mut State) {
    if line == "`=" {
        if state.literal {
            state.html.push_str("</pre></div>");
        } else {
            // A literal block keeps its section's indent
            state.html.push_str(&format!("<div{}><pre>", state.line_attrs()));
        }
        state.literal = !state.literal;
        return;
    }
    if state.literal {
        let text = if line == "\\`=" { "`=" } else { line };
        state.html.push_str(&escape_html(text));
        state.html.push('\n');
        return;
    }
    if line.is_empty() {
        if state.table.is_none() {
            state.html.push_str("<br>");
        }
        return;
    }

    // `+> and `-> make a heading foldable (open, or closed); rynx shows sections open
    let mut line = line;
    if (line.starts_with("`+") || line.starts_with("`-")) && line[2..].starts_with('>') {
        line = &line[2..];
    }
    // A heading can't hold fields
    if line.starts_with('>') && line.contains("`<") {
        line = line.trim_start_matches('>');
    }
    let mut escaped = false;
    if let Some(rest) = line.strip_prefix('\\') {
        line = rest;
        escaped = true;
    } else if line.starts_with('#') {
        // Comments, and #! headers (cache control, page colours, fold glyphs)
        return;
    }

    // `t opens and closes a table (its alignment and width options are left to the layout)
    if line.starts_with("`t") {
        match state.table.take() {
            Some(rows) => {
                let table = table_html(&rows, state);
                state.html.push_str(&table);
            }
            None => state.table = Some(Vec::new()),
        }
        return;
    }
    if let Some(rows) = state.table.as_mut() {
        rows.push(line.to_string());
        return;
    }
    if escaped {
        return text_line(line, state, true);
    }
    if let Some(partial) = line.strip_prefix("`{") {
        return partial_line(partial, state);
    }
    if let Some(image) = line.strip_prefix("`(") {
        return image_line(image, state);
    }
    if let Some(rest) = line.strip_prefix('<') {
        state.depth = 0;
        if !rest.is_empty() {
            convert_line(rest, state);
        }
        return;
    }
    if line.starts_with('>') {
        return heading_line(line, state);
    }
    if line.starts_with('-') {
        // A divider, of its second character when it has exactly two
        let mut chars = line.chars();
        chars.next();
        let glyph = match (chars.next(), chars.next()) {
            (Some(glyph), None) => glyph,
            _ => '─',
        };
        let glyph = escape_html(&glyph.to_string());
        state.html.push_str(&format!("<div{}><hr data-rynx-rule=\"{}\"></div>", state.line_attrs(), glyph));
        return;
    }
    text_line(line, state, false);
}

fn text_line(line: &str, state: &mut State, escaped: bool) {
    let content = inline_html(line, state, escaped);
    // A line of only formatting changes takes no space
    if !content.is_empty() {
        let attrs = state.line_attrs();
        state.html.push_str(&format!("<div{}>{}</div>", attrs, content));
    }
}

fn heading_line(line: &str, state: &mut State) {
    let level = line.chars().take_while(|&c| c == '>').count();
    state.depth = level;
    let text = &line[level..];
    if text.is_empty() {
        // A section without a heading: only its indent
        return;
    }
    // A heading's formatting is its own: the text after it continues as before
    let saved = state.style.clone();
    let content = inline_html(text, state, false);
    state.style = saved;
    let plain = plain_text(text);
    if state.title.is_none() && !plain.trim().is_empty() {
        state.title = Some(plain.trim().to_string());
    }
    let slug = slug(text);
    let id = if slug.is_empty() { String::new() } else { format!(" id=\"{}\"", escape_html(&slug)) };
    let underline = if level == 1 { ("<u>", "</u>") } else { ("", "") };
    let attrs = state.line_attrs();
    state.html.push_str(&format!("<div{}{}><b>{}{}{}</b></div>", id, attrs, underline.0, content, underline.1));
}

/// `{url`refresh`fields}: part of the page, loaded separately (and refreshed every `refresh`
/// seconds); shown as a link to it
fn partial_line(partial: &str, state: &mut State) {
    let Some(end) = partial.find('}') else { return };
    let url = partial[..end].split('`').next().unwrap_or("");
    if url.is_empty() {
        return;
    }
    let attrs = state.line_attrs();
    let href = escape_html(&link_href(url, ""));
    state.html.push_str(&format!(
        "<div{}><a href=\"{}\">[Part of the page: {}]</a></div>",
        attrs,
        href,
        escape_html(url)
    ));
}

/// `(alt`options`url): an image, shown as a link to it with its description
fn image_line(image: &str, state: &mut State) {
    let Some(end) = image.rfind(')') else { return };
    let parts: Vec<&str> = image[..end].split('`').collect();
    if parts.len() < 2 {
        return;
    }
    let (alt, url) = (parts[0].trim(), parts[parts.len() - 1].trim());
    let label = if alt.is_empty() { url } else { alt };
    let attrs = state.line_attrs();
    let href = escape_html(&link_href(url, ""));
    state.html.push_str(&format!("<div{}><a href=\"{}\">[Image: {}]</a></div>", attrs, href, escape_html(label)));
}

/// A `t block's rows ("| a | b |"), the first a header when a separator row ("| --- | :-: |")
/// follows it
fn table_html(rows: &[String], state: &mut State) -> String {
    let cells = |row: &str| -> Vec<String> {
        let row = row.trim();
        let row = row.strip_prefix('|').unwrap_or(row);
        let row = row.strip_suffix('|').unwrap_or(row);
        row.split('|').map(|cell| cell.trim().to_string()).collect()
    };
    let is_separator = |row: &str| {
        let cells = cells(row);
        !cells.is_empty()
            && cells.iter().all(|c| {
                let c = c.trim_matches(':');
                !c.is_empty() && c.chars().all(|ch| ch == '-')
            })
    };
    let headed = rows.get(1).is_some_and(|row| is_separator(row));
    let mut html = format!("<div{}><table>", state.line_attrs());
    for (i, row) in rows.iter().enumerate() {
        if row.trim().is_empty() || is_separator(row) {
            continue;
        }
        let tag = if headed && i == 0 { "th" } else { "td" };
        html.push_str("<tr>");
        for cell in cells(row) {
            // Formatting inside a cell stays in it
            let saved = state.style.clone();
            let content = inline_html(&cell, state, false);
            state.style = saved;
            html.push_str(&format!("<{tag}>{content}</{tag}>"));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table></div>");
    html
}

/// A run of text in the current formatting
fn styled(text: &str, style: &Style) -> String {
    let mut html = escape_html(text);
    if style.underline {
        html = format!("<u>{}</u>", html);
    }
    if style.italic {
        html = format!("<i>{}</i>", html);
    }
    if style.bold {
        html = format!("<b>{}</b>", html);
    }
    if style.fg.is_some() || style.bg.is_some() {
        let mut attrs = String::new();
        if let Some(fg) = &style.fg {
            attrs.push_str(&format!(" data-rynx-fg=\"{}\"", fg));
        }
        if let Some(bg) = &style.bg {
            attrs.push_str(&format!(" data-rynx-bg=\"{}\"", bg));
        }
        html = format!("<span{}>{}</span>", attrs, html);
    }
    html
}

/// A colour after `F or `B: three hex digits, or T and six; returns it and how many characters
/// it took
fn color_at(rest: &str) -> Option<(String, usize)> {
    let hex = |s: &str| s.chars().all(|c| c.is_ascii_hexdigit());
    if let Some(six) = rest.strip_prefix('T').and_then(|r| r.get(..6)).filter(|s| hex(s)) {
        return Some((six.to_ascii_lowercase(), 7));
    }
    rest.get(..3).filter(|s| hex(s)).map(|three| (three.to_ascii_lowercase(), 3))
}

/// The HTML for a line's text: formatting tags, colours, links, fields and anchors
fn inline_html(line: &str, state: &mut State, escaped: bool) -> String {
    let mut html = String::new();
    let mut text = String::new();
    let mut escape = escaped;
    let mut rest = line;
    let flush = |text: &mut String, html: &mut String, style: &Style| {
        if !text.is_empty() {
            html.push_str(&styled(text, style));
            text.clear();
        }
    };
    while let Some(c) = rest.chars().next() {
        rest = &rest[c.len_utf8()..];
        if escape {
            text.push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' => escape = true,
            '`' => {
                flush(&mut text, &mut html, &state.style);
                let Some(tag) = rest.chars().next() else { break };
                rest = &rest[tag.len_utf8()..];
                match tag {
                    '!' => state.style.bold ^= true,
                    '*' => state.style.italic ^= true,
                    '_' => state.style.underline ^= true,
                    'F' | 'B' => {
                        if let Some((color, used)) = color_at(rest) {
                            rest = &rest[used..];
                            if tag == 'F' {
                                state.style.fg = Some(color);
                            } else {
                                state.style.bg = Some(color);
                            }
                        }
                    }
                    'f' => state.style.fg = None,
                    'b' => state.style.bg = None,
                    '`' => {
                        state.style = Style::default();
                        state.align = Align::Left;
                    }
                    'c' => state.align = Align::Center,
                    'r' => state.align = Align::Right,
                    'l' | 'a' => state.align = Align::Left,
                    ':' => {
                        let end = rest
                            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                            .unwrap_or(rest.len());
                        if end > 0 {
                            html.push_str(&format!("<a id=\"{}\"></a>", escape_html(&rest[..end])));
                        }
                        rest = &rest[end..];
                    }
                    '[' => {
                        if let Some(end) = rest.find(']') {
                            html.push_str(&link_html(&rest[..end], &state.style));
                            rest = &rest[end + 1..];
                        }
                    }
                    '<' => {
                        if let Some((field, used)) = field_html(rest) {
                            html.push_str(&field);
                            rest = &rest[used..];
                        }
                    }
                    _ => {}
                }
            }
            // Spaces are kept, as Micron lays out with them (HTML would collapse them): those
            // starting a line, and all but the last of a run, where the line can still wrap
            ' ' if html.is_empty() && text.chars().all(|c| c == NBSP) => text.push(NBSP),
            ' ' => {
                if text.ends_with(' ') {
                    text.pop();
                    text.push(NBSP);
                }
                text.push(' ');
            }
            _ => text.push(c),
        }
    }
    flush(&mut text, &mut html, &state.style);
    html
}

/// `[label`url`fields]: a link. Its fields name page fields to send, which rynx can't yet;
/// `name=value` ones are request variables, sent in the address.
fn link_html(link: &str, style: &Style) -> String {
    let parts: Vec<&str> = link.split('`').collect();
    let (label, url, fields) = match parts.as_slice() {
        [url] => ("", *url, ""),
        [label, url] => (*label, *url, ""),
        [label, url, fields] => (*label, *url, *fields),
        _ => return String::new(),
    };
    if url.is_empty() {
        return String::new();
    }
    let label = if label.is_empty() { url } else { label };
    format!("<a href=\"{}\">{}</a>", escape_html(&link_href(url, fields)), styled(label, style))
}

/// Where a Micron link goes, as an address rynx understands. ":/page/x.mu" is on the same node
/// (a path here), "#name" a place on this page, and "<node>:/page/x.mu" another node, written
/// nomadnet://<node>/page/x.mu. Anything else (http://, lxmf@...) is kept.
fn link_href(url: &str, fields: &str) -> String {
    let mut href = if let Some(path) = url.strip_prefix(':') {
        path.to_string()
    } else if let Some((node, path)) = url.split_once(':')
        && node.len() == 32
        && node.chars().all(|c| c.is_ascii_hexdigit())
    {
        format!("nomadnet://{}{}", node.to_ascii_lowercase(), path)
    } else {
        url.to_string()
    };
    let variables: Vec<(&str, &str)> = fields.split('|').filter_map(|f| f.split_once('=')).collect();
    if !variables.is_empty() && !href.starts_with('#') {
        let query: String = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(variables).finish();
        href.push(if href.contains('?') { '&' } else { '?' });
        href.push_str(&query);
    }
    href
}

/// `<flags|name`data>: a field, checkbox or radio button. Returns its HTML and the characters it
/// took, up to and including the closing >.
fn field_html(rest: &str) -> Option<(String, usize)> {
    let tick = rest.find('`')?;
    let end = tick + rest[tick..].find('>')?;
    let spec = &rest[..tick];
    let data = &rest[tick + 1..end];
    let parts: Vec<&str> = spec.split('|').collect();
    let (flags, name) = match parts.as_slice() {
        [name] => ("", *name),
        [flags, name, ..] => (*flags, *name),
        [] => return None,
    };
    let name = escape_html(name);
    let html = if flags.contains('^') || flags.contains('?') {
        // For these, the data is the label; the value is given, or else the label
        let kind = if flags.contains('^') { "radio" } else { "checkbox" };
        let value = parts.get(2).copied().filter(|v| !v.is_empty()).unwrap_or(data);
        let checked = if parts.get(3) == Some(&"*") { " checked" } else { "" };
        format!(
            "<input type=\"{kind}\" name=\"{name}\" value=\"{}\"{checked}> {}",
            escape_html(value),
            escape_html(data)
        )
    } else {
        let masked = flags.contains('!');
        let size = flags.replace('!', "");
        let (width, rows) = size.split_once('x').unwrap_or((&size, ""));
        let width = width.parse::<usize>().unwrap_or(DEFAULT_FIELD_WIDTH).min(MAX_FIELD_WIDTH);
        let rows = rows.parse::<usize>().unwrap_or(1).clamp(1, 256);
        if rows > 1 {
            format!("<textarea name=\"{name}\" cols=\"{width}\" rows=\"{rows}\">{}</textarea>", escape_html(data))
        } else {
            let kind = if masked { "password" } else { "text" };
            format!("<input type=\"{kind}\" name=\"{name}\" size=\"{width}\" value=\"{}\">", escape_html(data))
        }
    };
    Some((html, end + 1))
}

/// A line's text without its Micron tags, for titles
fn plain_text(line: &str) -> String {
    let mut state = State::default();
    let html = inline_html(line, &mut state, false);
    let document = scraper::Html::parse_fragment(&html);
    document.root_element().text().collect()
}

/// A heading's anchor name, as NomadNet makes it: the text without tags, lowercased, with each
/// run of other characters than letters and digits made a hyphen
pub fn slug(heading: &str) -> String {
    let mut slug = String::new();
    for c in plain_text(heading).chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}
