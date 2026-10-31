//! Galleries: a container a site marks as one (class "gallery", "wp-block-gallery" and the like)
//! lays its items out side by side, each with its image on top and its caption under it, in as
//! many rows as the width needs. Until one of its images is loaded, it's drawn as usual.

use super::tables::{RenderedCell, image_columns, measure_cell};
use super::*;

/// Columns between items
const GAP: usize = 2;

/// Whether the element is marked as a gallery (its own class, not an item's or caption's, like
/// Wikipedia's "gallerybox" and "gallerytext")
pub(super) fn is_gallery(elem: &scraper::node::Element) -> bool {
    elem.attr("class").is_some_and(|class| {
        class.split_whitespace().any(|name| {
            let name = name.to_ascii_lowercase();
            name == "gallery" || name.ends_with("-gallery") || name.ends_with("-gallery-grid")
        })
    })
}

/// The gallery's items, in order
fn items(container: ElementRef) -> Vec<ElementRef> {
    container.children().filter_map(ElementRef::wrap).filter(|item| !is_hidden(item.value())).collect()
}

impl DomRenderer {
    /// Lay out a gallery whose images are loaded, returning whether it was (if not, it's drawn as
    /// usual)
    pub(super) fn render_gallery(&mut self, container: ElementRef) -> bool {
        let items = items(container);
        let sizes = Arc::clone(&self.image_sizes);
        if items.len() < 2 || items.iter().all(|item| image_columns(*item, &sizes) == 0) {
            return false;
        }
        let prefix_width: usize = self.prefix_spans().iter().map(|s| text_width(&s.content)).sum();
        let available = self.max_width.saturating_sub(prefix_width).max(1);
        // Each item as wide as its image, or its widest word (capped) if that's wider
        let widths: Vec<usize> = items
            .iter()
            .map(|item| image_columns(*item, &sizes).max(measure_cell(*item, &sizes).1).clamp(1, available))
            .collect();

        self.image_row = None;
        self.add_vertical_space();
        if let Some(id) = container.value().attr("id") {
            self.anchors.entry(id.to_string()).or_insert(self.lines.len());
        }
        let mut start = 0;
        while start < items.len() {
            // As many items as fit on the row
            let mut end = start + 1;
            let mut used = widths[start];
            while end < items.len() && used + GAP + widths[end] <= available {
                used += GAP + widths[end];
                end += 1;
            }
            let mut x = 0;
            let row: Vec<(usize, RenderedCell)> = (start..end)
                .map(|i| {
                    let at = x;
                    x += widths[i] + GAP;
                    (at, self.render_block(items[i], false, 0, widths[i]))
                })
                .collect();
            if start > 0 {
                self.add_vertical_space();
            }
            self.place_side_by_side(&row);
            start = end;
        }
        self.add_vertical_space();
        true
    }

    /// Draw rendered blocks side by side, each at its column, with their links and images
    fn place_side_by_side(&mut self, blocks: &[(usize, RenderedCell)]) {
        let first_row = self.lines.len();
        let height = blocks.iter().map(|(_, block)| block.lines.len()).max().unwrap_or(0);
        let mut base = 0;
        for k in 0..height {
            self.flush_line();
            self.apply_prefix();
            base = self.current_line_width;
            for (x, block) in blocks {
                let Some(line) = block.lines.get(k).filter(|l| !l.spans.is_empty()) else {
                    continue;
                };
                let pad = (base + x).saturating_sub(self.current_line_width);
                self.emit(&" ".repeat(pad), Style::default(), None);
                for span in &line.spans {
                    self.emit(&span.content, span.style, None);
                }
            }
            self.line_has_text = true;
            self.marker_pending = false;
            self.flush_line();
        }
        for (x, block) in blocks {
            let x = base + x;
            for link in &block.links {
                self.links.push(LinkRegion {
                    line_index: first_row + link.line_index,
                    x_start: x + link.x_start,
                    x_end: x + link.x_end,
                    ..link.clone()
                });
            }
            for (name, line) in &block.anchors {
                self.anchors.entry(name.clone()).or_insert(first_row + line);
            }
            for image in &block.images {
                self.image_blocks.push(ImageBlock {
                    line_index: first_row + image.line_index,
                    x: x + image.x,
                    ..*image
                });
            }
        }
    }
}
