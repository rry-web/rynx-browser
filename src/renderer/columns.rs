//! Columns for rynx's own pages: a container marked `data-rynx-columns="N"` lays its child
//! blocks out in as many columns at least N wide as fit, flowing down each column in turn like a
//! newspaper. Narrower than two columns, it is drawn as usual.

use super::tables::RenderedCell;
use super::*;

/// The attribute marking a container whose blocks flow into columns; named for rynx so a site's
/// own `data-columns` doesn't change how it's drawn
pub(super) const COLUMNS_ATTRIBUTE: &str = "data-rynx-columns";

/// Spaces between columns
const GAP: usize = 4;

/// The container's child blocks, in order
fn blocks(container: ElementRef) -> Vec<ElementRef> {
    container.children().filter_map(ElementRef::wrap).filter(|b| !is_hidden(b.value())).collect()
}

/// Split `heights` in order into at most `columns` runs, each a column with a blank line between
/// its blocks, so the tallest column is as short as can be. Returns where each column starts.
fn balance(heights: &[usize], columns: usize) -> Vec<usize> {
    let fill = |limit: usize| -> Option<Vec<usize>> {
        let mut starts = vec![0];
        let mut used = 0;
        for (i, &height) in heights.iter().enumerate() {
            let needed = if used == 0 { height } else { used + 1 + height };
            if needed <= limit {
                used = needed;
            } else {
                if starts.len() == columns {
                    return None;
                }
                starts.push(i);
                used = height;
            }
        }
        Some(starts)
    };
    let tallest = heights.iter().copied().max().unwrap_or(0);
    let total = heights.iter().sum::<usize>() + heights.len().saturating_sub(1);
    (tallest..=total).find_map(fill).unwrap_or_else(|| vec![0])
}

impl DomRenderer {
    /// How many columns a columns container gets at this width: 1 means it's drawn as usual
    pub(super) fn column_count(&self, container: ElementRef) -> usize {
        let Some(least) = container.value().attr(COLUMNS_ATTRIBUTE).and_then(|n| n.trim().parse::<usize>().ok()) else {
            return 1;
        };
        let prefix_width: usize = self.prefix_spans().iter().map(|s| text_width(&s.content)).sum();
        let available = self.max_width.saturating_sub(prefix_width);
        let fit = (available + GAP) / (least.max(1) + GAP);
        fit.min(blocks(container).len()).max(1)
    }

    pub(super) fn render_columns(&mut self, container: ElementRef, columns: usize) {
        let prefix_width: usize = self.prefix_spans().iter().map(|s| text_width(&s.content)).sum();
        let available = self.max_width.saturating_sub(prefix_width);
        let width = (available - GAP * (columns - 1)) / columns;
        let rendered: Vec<RenderedCell> =
            blocks(container).into_iter().map(|block| self.render_block(block, false, 0, width)).collect();
        let heights: Vec<usize> = rendered.iter().map(|block| block.lines.len()).collect();
        let starts = balance(&heights, columns);

        // Stack each column's blocks, a blank line apart
        let mut stacked: Vec<RenderedCell> = Vec::new();
        for (column, &start) in starts.iter().enumerate() {
            let end = starts.get(column + 1).copied().unwrap_or(rendered.len());
            let mut stack =
                RenderedCell { column, lines: Vec::new(), links: Vec::new(), anchors: Vec::new(), images: Vec::new() };
            for block in &rendered[start..end] {
                if !stack.lines.is_empty() {
                    stack.lines.push(Line::default());
                }
                let top = stack.lines.len();
                stack.lines.extend(block.lines.iter().cloned());
                stack
                    .links
                    .extend(block.links.iter().map(|l| LinkRegion { line_index: top + l.line_index, ..l.clone() }));
                stack.anchors.extend(block.anchors.iter().map(|(name, line)| (name.clone(), top + line)));
                stack.images.extend(block.images.iter().map(|b| ImageBlock { line_index: top + b.line_index, ..*b }));
            }
            stacked.push(stack);
        }

        self.add_vertical_space();
        if let Some(id) = container.value().attr("id") {
            self.anchors.entry(id.to_string()).or_insert(self.lines.len());
        }
        let first_row = self.lines.len();
        let height = stacked.iter().map(|column| column.lines.len()).max().unwrap_or(0);
        let mut base = 0;
        for k in 0..height {
            self.flush_line();
            self.apply_prefix();
            base = self.current_line_width;
            for column in &stacked {
                let Some(line) = column.lines.get(k).filter(|l| !l.spans.is_empty()) else {
                    continue;
                };
                let x = base + column.column * (width + GAP);
                let pad = x.saturating_sub(self.current_line_width);
                self.emit(&" ".repeat(pad), Style::default(), None);
                for span in &line.spans {
                    self.emit(&span.content, span.style, None);
                }
            }
            self.line_has_text = true;
            self.marker_pending = false;
            self.flush_line();
        }
        // Links in reading order, down each column in turn, so Tab follows the text
        for column in &stacked {
            let x = base + column.column * (width + GAP);
            for link in &column.links {
                self.links.push(LinkRegion {
                    line_index: first_row + link.line_index,
                    x_start: x + link.x_start,
                    x_end: x + link.x_end,
                    ..link.clone()
                });
            }
            for (name, line) in &column.anchors {
                self.anchors.entry(name.clone()).or_insert(first_row + line);
            }
            for block in &column.images {
                self.image_blocks.push(ImageBlock {
                    line_index: first_row + block.line_index,
                    x: x + block.x,
                    ..*block
                });
            }
        }
        self.add_vertical_space();
    }
}

#[cfg(test)]
mod tests {
    use super::balance;

    #[test]
    fn columns_are_balanced_in_order() {
        // Heights 5, 3, 4, 2 in two columns: 5+3 (9 with the gap) | 4+2 (7) beats 5 | 3+4+2 (11)
        assert_eq!(balance(&[5, 3, 4, 2], 2), [0, 2]);
        assert_eq!(balance(&[5, 3, 4, 2], 3), [0, 1, 2]);
        assert_eq!(balance(&[5, 3, 4, 2], 1), [0]);
        // A block taller than the rest gets a column to itself
        assert_eq!(balance(&[20, 2, 2, 2], 2), [0, 1]);
    }
}
