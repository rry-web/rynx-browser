//! Images in the page: a placeholder with the image's description, which can be selected to view
//! it, and, once the image is loaded, space reserved to draw it in the page.

use super::*;

/// Images no bigger than this (in pixels, by their width or height attribute) are tracking pixels
/// or spacers, not pictures
const TINY_IMAGE: u32 = 2;

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

        // A loaded image gets space of its own, except in table cells and one-line elements
        let size = index.and_then(|i| self.image_sizes.get(&self.images[i].src).copied());
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
        if size.is_some() && self.inline_depth == 0 {
            // The caption under a drawn image is a line of its own
            self.flush_line();
        }
    }
}
