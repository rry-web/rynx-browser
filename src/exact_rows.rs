//! Rows of text whose width terminals disagree about, put exactly where they belong.
//!
//! The screen is drawn by writing runs of cells and letting the terminal's cursor advance by each
//! character's width. For most text every terminal agrees on that width, but not for some:
//! Indic and Southeast Asian scripts' vowel signs and conjuncts, emoji sequences, zero-width
//! characters. Kitty, for one, draws a whole cluster in the width of its first character;
//! macOS Terminal counts each character; rynx counts as Unicode's tables say. Where they differ,
//! everything after such a cluster lands a column or two off, and cells the program believes it
//! filled keep whatever was there before (stray letters left down the page as you scroll).
//!
//! So after each frame, the rows containing such text that changed are written again cell by
//! cell, each at its own position, with the columns a cluster was given cleared first: whatever
//! width the terminal gives a cluster, the next cell starts where it should, and nothing stale
//! is left. (A terminal drawing a cluster wider than it was given clips it; nothing moves.)

use ratatui::buffer::{Buffer, Cell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use unicode_width::UnicodeWidthStr;

/// Kitty's image placeholder character: such cells are the image's, and never rewritten
const IMAGE_PLACEHOLDER: char = '\u{10EEEE}';

/// Whether terminals may disagree about how wide `symbol` (one cell's grapheme cluster) is
pub fn uncertain_width(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    let Some(first) = chars.next() else { return false };
    if first == IMAGE_PLACEHOLDER {
        return false;
    }
    // More than one character in a cell: combining marks, joiners, modifiers, flags
    if chars.next().is_some() {
        return true;
    }
    let c = first as u32;
    matches!(c,
        // Zero-width characters and joiners, variation selectors
        0x200b..=0x200f | 0x2060..=0x2064 | 0xfe00..=0xfe0f | 0xfeff
        // Indic scripts (Devanagari to Malayalam, Sinhala), Myanmar, Khmer, Tai scripts, and others
        // whose vowel signs and viramas terminals draw differently
        | 0x0900..=0x0dff | 0x1000..=0x109f | 0x1780..=0x17ff | 0x1a00..=0x1aaf | 0x1b00..=0x1bff
        | 0xa8e0..=0xa8ff | 0xa980..=0xa9ff | 0xaa00..=0xaadf | 0x11000..=0x11fff
        // Emoji and pictographs, whose presentation varies
        | 0x2600..=0x27bf | 0x1f000..=0x1faff
    )
}

/// What was last written exactly, by row, so unchanged rows aren't written again
#[derive(Default)]
pub struct ExactRows {
    written: HashMap<u16, u64>,
}

impl ExactRows {
    /// Forget what was written (the terminal was resized or cleared, and everything redrawn)
    pub fn reset(&mut self) {
        self.written.clear();
    }

    /// The cells to write again for this frame, each at its own position: (column, row, cell).
    /// Columns a cluster spans past its first are cleared (written as spaces) before it.
    pub fn cells_to_rewrite(&mut self, buffer: &Buffer) -> Vec<(u16, u16, Cell)> {
        let area = buffer.area;
        let mut out = Vec::new();
        for y in area.top()..area.bottom() {
            let row: Vec<&Cell> = (area.left()..area.right()).filter_map(|x| buffer.cell((x, y))).collect();
            if !row.iter().any(|cell| uncertain_width(cell.symbol())) {
                self.written.remove(&y);
                continue;
            }
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for cell in &row {
                cell.symbol().hash(&mut hasher);
                format!("{:?}{:?}{:?}", cell.fg, cell.bg, cell.modifier).hash(&mut hasher);
            }
            let digest = hasher.finish();
            if self.written.get(&y) == Some(&digest) {
                continue;
            }
            self.written.insert(y, digest);
            let mut x = 0usize;
            while x < row.len() {
                let cell = row[x];
                let symbol = cell.symbol();
                let width = symbol.width().max(1);
                let column = area.left() + x as u16;
                if symbol.starts_with(IMAGE_PLACEHOLDER) {
                    x += 1;
                    continue;
                }
                if uncertain_width(symbol) {
                    // The columns it was given past the first, cleared first
                    for extra in 1..width {
                        if x + extra < row.len() {
                            let mut blank = row[x].clone();
                            blank.set_symbol(" ");
                            out.push((column + extra as u16, y, blank));
                        }
                    }
                }
                // An empty symbol is the second half of a wide character: written with it
                if !symbol.is_empty() {
                    out.push((column, y, cell.clone()));
                }
                x += width;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn widths_terminals_disagree_about() {
        for uncertain in ["मो", "ம", "ാ", "မ", "\u{200b}", "👍🏽", "e\u{301}", "🇫🇷", "☀"] {
            assert!(uncertain_width(uncertain), "{uncertain:?}");
        }
        for certain in ["a", "Ж", "日", "한", "ع", "ไ", " ", "", "\u{10EEEE}\u{305}"] {
            assert!(!uncertain_width(certain), "{certain:?}");
        }
    }

    #[test]
    fn rows_with_uncertain_text_are_rewritten_cell_by_cell_once() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 3));
        buffer.set_string(0, 0, "plain row", Style::default());
        buffer.set_string(0, 1, "മലയാളം · CJK 日本", Style::default());
        let mut exact = ExactRows::default();
        let cells = exact.cells_to_rewrite(&buffer);
        assert!(!cells.is_empty());
        assert!(cells.iter().all(|(_, y, _)| *y == 1), "only the row with uncertain text");
        // Every column of that row is written once (wide 日 and 本 cover their second columns)
        let mut columns: Vec<u16> = cells.iter().map(|(x, _, _)| *x).collect();
        columns.sort();
        columns.dedup();
        let wide_halves = 2;
        assert_eq!(columns.len() + wide_halves, 20, "{columns:?}");
        // Unchanged, it isn't written again; changed, it is; a reset writes it again
        assert!(exact.cells_to_rewrite(&buffer).is_empty());
        buffer.set_string(15, 1, "x", Style::default());
        assert!(!exact.cells_to_rewrite(&buffer).is_empty());
        exact.reset();
        assert!(!exact.cells_to_rewrite(&buffer).is_empty());
    }

    #[test]
    fn image_cells_are_left_alone() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        buffer.set_string(0, 0, "ம", Style::default());
        buffer[(3, 0)].set_symbol("\u{10EEEE}\u{305}\u{305}");
        let cells = ExactRows::default().cells_to_rewrite(&buffer);
        assert!(!cells.iter().any(|(x, _, _)| *x == 3), "an image cell was rewritten");
    }
}
