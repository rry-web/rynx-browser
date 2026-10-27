//! Tables: aligned columns when they fit, `Header: value` rows when they don't.

use super::*;

const COLUMN_SEPARATOR: &str = " │ ";

const COLUMN_SEPARATOR_WIDTH: usize = 3;

const MAX_COLSPAN: usize = 64;

/// A long word (e.g. a URL) can't force its column wider than this; it is wrapped instead
const MAX_MIN_COLUMN_WIDTH: usize = 20;

struct TableCell<'a> {
    pub(super) element: ElementRef<'a>,
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
    pub(super) lines: Vec<Line<'static>>,
    pub(super) links: Vec<LinkRegion>,
    pub(super) anchors: Vec<(String, usize)>,
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
                        && node
                            .prev_siblings()
                            .any(|s| s.value().as_element().is_some_and(|e| matches!(e.name(), "td" | "th")));
                    if matches!(elem.name(), "td" | "th") && follows_cell {
                        // A nested table's column separator
                        words.push(("│".to_string(), indent));
                    } else if let Some(width) = control_width(elem) {
                        words.push(("_".repeat(width), indent));
                        if matches!(elem.name(), "select" | "textarea") {
                            // Their text is drawn inside the box, not as words
                            skip_depth += 1;
                        }
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

/// Columns a form control's box takes, for measuring table cells
fn control_width(elem: &scraper::node::Element) -> Option<usize> {
    let size = |attr: &str, default: usize| {
        elem.attr(attr).and_then(|n| n.trim().parse::<usize>().ok()).unwrap_or(default).clamp(4, 40) + 2
    };
    match elem.name() {
        "input" => match elem.attr("type").unwrap_or("text").trim().to_ascii_lowercase().as_str() {
            "hidden" => None,
            "checkbox" | "radio" => Some(3),
            "submit" | "image" => Some(elem.attr("value").map_or(6, |v| v.chars().count()) + 4),
            _ => Some(size("size", 20)),
        },
        "textarea" => Some(size("cols", 30)),
        "select" => Some(12),
        _ => None,
    }
}

/// (natural, minimum) width of a cell: all its words on one line, and its widest word (capped)
fn measure_cell(cell: ElementRef) -> (usize, usize) {
    let words = cell_words(cell);
    let natural = words.iter().map(|(w, _)| text_width(w)).sum::<usize>()
        + words.len().saturating_sub(1)
        + words.iter().map(|(_, indent)| *indent).max().unwrap_or(0);
    let minimum = words.iter().map(|(w, indent)| text_width(w).min(MAX_MIN_COLUMN_WIDTH) + indent).max().unwrap_or(0);
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
impl DomRenderer {
    pub(super) fn render_table(&mut self, table: ElementRef) {
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

        let columns =
            rows.iter().map(|row| row.cells.iter().map(|cell| cell.colspan).sum::<usize>()).max().unwrap_or(1);
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

        match available.checked_sub(separators).and_then(|space| column_widths(&natural, &minimum, space)) {
            Some(widths) => self.render_table_grid(&rows, &widths),
            None => self.render_table_stacked(&rows),
        }
        self.add_vertical_space();
    }

    fn render_table_grid(&mut self, rows: &[TableRow], widths: &[usize]) {
        let column_x =
            |column: usize| -> usize { widths[..column].iter().sum::<usize>() + COLUMN_SEPARATOR_WIDTH * column };
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
                let width = widths[column..column + span].iter().sum::<usize>() + COLUMN_SEPARATOR_WIDTH * (span - 1);
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
                        // No trailing space when the last cell has nothing on this line
                        let ends_line = i == cells.len() - 1 && cell.lines.get(k).is_none();
                        let separator = if ends_line { COLUMN_SEPARATOR.trim_end() } else { COLUMN_SEPARATOR };
                        self.emit(separator, separator_style, None);
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
                                control: link.control,
                                image: link.image,
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
    fn render_cell(&mut self, cell: &TableCell, column: usize, width: usize) -> RenderedCell {
        let mut renderer = DomRenderer::with_content_width(width);
        renderer.forms = std::mem::take(&mut self.forms);
        renderer.controls = std::mem::take(&mut self.controls);
        renderer.form_ids = std::mem::take(&mut self.form_ids);
        // The page's image list, so image numbers stay the page's own
        renderer.images = std::mem::take(&mut self.images);
        renderer.current_form = self.current_form;
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
        self.forms = std::mem::take(&mut renderer.forms);
        self.controls = std::mem::take(&mut renderer.controls);
        self.form_ids = std::mem::take(&mut renderer.form_ids);
        self.images = std::mem::take(&mut renderer.images);

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
}
