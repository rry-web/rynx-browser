//! Images in the page: a placeholder with the image's description, which can be selected to view
//! it, and, once the image is loaded, space reserved to draw it in the page.

use super::*;

/// Images no bigger than this (in pixels, by their width or height attribute) are tracking pixels
/// or spacers, not pictures
const TINY_IMAGE: u32 = 2;

/// Columns between images side by side
const IMAGE_GAP: usize = 2;

/// Loaded images laid out side by side, as galleries and grids are: each image's block on the
/// same rows, its caption on the line below, under it. The row stays open while only images
/// follow (the elements around each, like list items and figures, don't count; text does).
#[derive(Clone, Copy, Debug)]
pub(super) struct ImageRow {
    /// The rows' first line, how many there are, and the caption line below them
    first_line: usize,
    rows: u16,
    caption_line: usize,
    /// Where the next image would start, and the block prefix's width all of them share
    next_x: usize,
    prefix_width: usize,
    /// Words drawn when the last caption was: any more, and it's no longer a row of images
    words: usize,
}

/// The address an image loads from. Sites that load images with scripts put a blank placeholder
/// in src and the real address in data-src (or similar) or srcset.
pub(super) fn image_source(elem: &scraper::node::Element) -> Option<String> {
    let usable = |value: &str| {
        let value = value.trim();
        (!value.is_empty() && !value.starts_with("data:") && !value.starts_with("javascript:"))
            .then(|| value.to_string())
    };
    // The first address in "a.jpg 1x, b.jpg 2x" or "a.jpg 480w, b.jpg 800w"
    let first_of_set = |set: &str| set.split(',').next().and_then(|c| c.split_whitespace().next()).and_then(usable);
    ["data-src", "data-lazy-src", "data-original"]
        .iter()
        .find_map(|name| elem.attr(name).and_then(usable))
        .or_else(|| elem.attr("src").and_then(usable))
        .or_else(|| elem.attr("srcset").and_then(first_of_set))
        .or_else(|| elem.attr("data-srcset").and_then(first_of_set))
}

impl DomRenderer {
    /// An <img>: a placeholder, and the image itself once it's loaded
    pub(super) fn render_image(&mut self, elem: &scraper::node::Element) {
        let size = |name: &str| elem.attr(name).and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok());
        if size("width").is_some_and(|w| w <= TINY_IMAGE) || size("height").is_some_and(|h| h <= TINY_IMAGE) {
            return;
        }
        let source = image_source(elem);
        // alt="" marks a decorative image; without alt, a picture is still shown as "image" so it
        // can be loaded (or, in a link, so the link stays visible)
        let label = match elem.attr("alt").map(str::trim) {
            Some("") => return,
            Some(alt) => alt.to_string(),
            None if source.is_some() || self.active_link_url.is_some() => "image".to_string(),
            None => return,
        };
        if self.last_image.as_ref().is_some_and(|(last, at)| *last == label && *at == self.words_drawn) {
            return;
        }
        let index = source.map(|src| {
            self.images.push(PageImage { src, alt: label.clone() });
            self.images.len() - 1
        });

        // A loaded image gets space of its own, except in table cells and one-line elements:
        // beside the one before it if only images came between and it fits, else below
        let size = index.and_then(|i| self.image_sizes.get(&self.images[i].src).copied());
        if let (Some(index), Some((columns, rows))) = (index, size)
            && self.inline_depth == 0
            && rows > 0
            && self.place_beside(index, &label, columns, rows)
        {
            self.last_image = Some((label, self.words_drawn));
            return;
        }
        self.image_row = None;
        if let (Some(index), Some((columns, rows))) = (index, size)
            && self.inline_depth == 0
            && rows > 0
        {
            self.flush_line();
            let prefix = self.prefix_spans();
            let prefix_width: usize = prefix.iter().map(|s| s.width()).sum();
            let columns = columns.min(self.max_width.saturating_sub(prefix_width) as u16).max(1);
            let line_index = self.lines.len();
            for _ in 0..rows {
                let mut line = prefix.clone();
                line.push(Span::raw(" ".repeat(columns as usize)));
                self.lines.push(Line::from(line));
            }
            self.last_line_blank = false;
            self.image_blocks.push(ImageBlock { image: index, line_index, x: prefix_width, columns, rows });
        }

        self.active_image = index;
        self.add_style(|s| s.fg(Color::DarkGray));
        self.push_word(&format!("[{}]", sanitize_for_terminal(&label)));
        self.style_stack.pop();
        self.active_image = None;
        self.last_image = Some((label, self.words_drawn));
        if let (Some(index), Some((columns, rows))) = (index, size)
            && self.inline_depth == 0
            && rows > 0
        {
            // The caption under a drawn image is a line of its own; if it fit on one, more
            // images can go beside this one
            let first_line = self.lines.len() - rows as usize;
            self.flush_line();
            if self.lines.len() == first_line + rows as usize + 1 {
                let block = self.image_blocks.iter().rev().find(|b| b.image == index).copied();
                if let Some(block) = block {
                    let caption_line = block.line_index + block.rows as usize;
                    let caption_end: usize =
                        self.lines[caption_line].spans.iter().map(|s| text_width(&s.content)).sum();
                    self.image_row = Some(ImageRow {
                        first_line: block.line_index,
                        rows: block.rows,
                        caption_line,
                        next_x: (block.x + columns as usize).max(caption_end) + IMAGE_GAP,
                        prefix_width: block.x,
                        words: self.words_drawn,
                    });
                }
            }
        }
    }

    /// Lay a loaded image out beside the row of images before it, if only images came between,
    /// the prefix is the same, and it fits. Returns whether it was.
    fn place_beside(&mut self, index: usize, label: &str, columns: u16, rows: u16) -> bool {
        let Some(row) = self.image_row else { return false };
        let prefix_width: usize = self.prefix_spans().iter().map(|s| s.width()).sum();
        let blank_after =
            self.lines[row.caption_line + 1..].iter().all(|l| l.spans.iter().all(|s| s.content.trim().is_empty()));
        // Nothing on the line yet but its prefix (and a list item's bullet, not drawn yet)
        let prefix: String = self.prefix_spans().iter().map(|s| s.content.as_ref()).collect();
        let current: String = self.current_line.iter().map(|s| s.content.as_ref()).collect();
        let current_empty =
            current.trim().is_empty() || (self.marker_pending && text_width(&current) == text_width(&prefix));
        let caption = format!("[{}]", sanitize_for_terminal(label));
        let caption_width = text_width(&caption);
        if self.words_drawn != row.words
            || prefix_width != row.prefix_width
            || row.next_x + (columns as usize).max(caption_width) > self.max_width
            || !blank_after
            || !current_empty
        {
            return false;
        }
        // A list item's bullet waiting to be drawn isn't: the image is the item
        if self.marker_pending {
            self.current_line.clear();
            self.current_line_width = 0;
            self.marker_pending = false;
        }
        // A taller image: more rows, before the captions
        if rows > row.rows {
            self.insert_lines(row.caption_line, (rows - row.rows) as usize);
        }
        let rows_now = row.rows.max(rows);
        let caption_line = row.first_line + rows_now as usize;
        let x = row.next_x;
        for line_index in row.first_line..row.first_line + rows as usize {
            pad_line(&mut self.lines[line_index], x);
            self.lines[line_index].spans.push(Span::raw(" ".repeat(columns as usize)));
        }
        self.image_blocks.push(ImageBlock { image: index, line_index: row.first_line, x, columns, rows });
        // Its caption under it, selectable like any image's
        pad_line(&mut self.lines[caption_line], x);
        self.lines[caption_line].spans.push(Span::styled(caption, Style::default().fg(Color::DarkGray)));
        self.links.push(LinkRegion {
            url: self.active_link_url.clone().unwrap_or_default(),
            control: None,
            image: Some(index),
            line_index: caption_line,
            x_start: x,
            x_end: x + caption_width,
        });
        // The next starts past this image or its caption, whichever is wider
        let next_x = x + (columns as usize).max(caption_width) + IMAGE_GAP;
        self.image_row = Some(ImageRow { rows: rows_now, caption_line, next_x, ..row });
        true
    }

    /// Insert blank lines before line `at`, moving everything recorded after it down
    fn insert_lines(&mut self, at: usize, count: usize) {
        for _ in 0..count {
            self.lines.insert(at, Line::from(self.prefix_spans()));
        }
        for link in &mut self.links {
            if link.line_index >= at {
                link.line_index += count;
            }
        }
        for line in self.anchors.values_mut() {
            if *line >= at {
                *line += count;
            }
        }
        for block in &mut self.image_blocks {
            if block.line_index >= at {
                block.line_index += count;
            }
        }
        self.preformatted_lines =
            self.preformatted_lines.iter().map(|&l| if l >= at { l + count } else { l }).collect();
    }
}

/// Pad a line with spaces to column `x`
fn pad_line(line: &mut Line<'static>, x: usize) {
    let width: usize = line.spans.iter().map(|s| text_width(&s.content)).sum();
    if width < x {
        line.spans.push(Span::raw(" ".repeat(x - width)));
    }
}
