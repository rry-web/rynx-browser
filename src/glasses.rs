//! Glasses mode's layout: the screen a display shows and what goes on it, read from glasses.conf
//! in the settings folder (or a file named with --glasses-layout), so layouts for different
//! displays can be tried on an ordinary terminal.

use std::path::Path;

/// The layout file in the settings folder
pub const LAYOUT_FILE: &str = "glasses.conf";

/// Smallest screen taken: room for a row of page between the title and status rows
const MIN_COLUMNS: u16 = 10;
const MIN_ROWS: u16 = 3;
const MAX_SIZE: u16 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Columns and rows drawn in, from the window's top left corner; None for the whole window
    pub screen: Option<(u16, u16)>,
    /// The row at the top with the mode and the page's title (or the address being typed)
    pub title_row: bool,
    /// The row at the bottom with the status line, where what the voice controller heard shows
    pub status_row: bool,
    /// Blank columns either side of the page's text
    pub margin: u16,
    /// Colour kept, for colour displays; otherwise what colour marked is reversed
    pub colour: bool,
    /// Every tab in reader mode
    pub reader_mode: bool,
}

impl Default for Layout {
    /// A 640x480 display with an 8x16 font
    fn default() -> Layout {
        Layout {
            screen: Some((80, 30)),
            title_row: true,
            status_row: true,
            margin: 1,
            colour: false,
            reader_mode: true,
        }
    }
}

/// The file written when there's none, which says what each line does
pub const DEFAULT_TEXT: &str = "\
# rynx's glasses mode (rynx --glasses): what the glasses' display shows.
# Read when rynx starts; restart it to try a change. Another file can be used instead with
#   rynx --glasses-layout FILE      (or scripts/rynx-glasses --layout NAME, for glasses/layouts/)

# The screen: columns x rows of text, or the display's pixels and the font's size in pixels
# (rows and columns are worked out from them), or window for the whole terminal window. rynx
# draws in the window's top left corner, with a line along the screen's edge when the window is
# larger, so any terminal shows what the display would.
#   screen = 80x30
#   screen = 640x480 pixels
#   font = 8x16
screen = 80x30

# The top row: the mode, and the page's title or the address being typed
title_row = on
# The bottom row: the status line, where what was heard shows
status_row = on
# Blank columns either side of the page
margin = 1
# off: what colour marked is drawn reversed, for monochrome displays
colour = off
# Every tab in reader mode: only a page's main text
reader_mode = on
";

/// A size written WxH (or W×H)
fn size(value: &str) -> Option<(u16, u16)> {
    let (w, h) = value.split_once(['x', 'X', '×'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn on_off(value: &str) -> Option<bool> {
    match value {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

impl Layout {
    /// Read a layout. Problems (unknown keys, values that can't be used) are returned as messages,
    /// and the defaults kept for those lines.
    pub fn parse(text: &str) -> (Layout, Vec<String>) {
        let mut layout = Layout::default();
        let mut problems = Vec::new();
        // In pixels, until the font's size is known
        let (mut pixels, mut font) = (None, (8, 16));
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                problems.push(format!("line {}: expected key = value", number + 1));
                continue;
            };
            let (key, value) = (key.trim(), value.trim().trim_matches('"').trim());
            let fits = |(w, h): (u16, u16)| w > 0 && h > 0 && w <= MAX_SIZE * 32 && h <= MAX_SIZE * 32;
            match key {
                "screen" if value == "window" => (layout.screen, pixels) = (None, None),
                "screen" if value.ends_with("pixels") => match size(value.trim_end_matches("pixels")) {
                    Some(px) if fits(px) => pixels = Some(px),
                    _ => problems.push(format!(
                        "line {}: screen = {} isn't a size like 640x480 pixels",
                        number + 1,
                        value
                    )),
                },
                "screen" => match size(value) {
                    Some(s) => (layout.screen, pixels) = (Some(s), None),
                    None => problems.push(format!("line {}: screen = {} isn't a size like 80x30", number + 1, value)),
                },
                "font" => match size(value) {
                    Some(f) if fits(f) => font = f,
                    _ => problems.push(format!("line {}: font = {} isn't a size like 8x16", number + 1, value)),
                },
                "title_row" | "status_row" | "colour" | "color" | "reader_mode" => match on_off(value) {
                    Some(on) => match key {
                        "title_row" => layout.title_row = on,
                        "status_row" => layout.status_row = on,
                        "reader_mode" => layout.reader_mode = on,
                        _ => layout.colour = on,
                    },
                    None => problems.push(format!("line {}: {} is on or off", number + 1, key)),
                },
                "margin" => match value.parse::<u16>() {
                    Ok(m) if m <= 10 => layout.margin = m,
                    _ => problems.push(format!("line {}: margin is a number of columns, 0 to 10", number + 1)),
                },
                _ => problems.push(format!("line {}: unknown setting \"{}\"", number + 1, key)),
            }
        }
        if let Some((w, h)) = pixels {
            layout.screen = Some((w / font.0, h / font.1));
        }
        if let Some((w, h)) = layout.screen {
            let fixed = (w.clamp(MIN_COLUMNS, MAX_SIZE), h.clamp(MIN_ROWS, MAX_SIZE));
            if fixed != (w, h) {
                problems.push(format!("a {}x{} screen is too small or large: {}x{} is used", w, h, fixed.0, fixed.1));
                layout.screen = Some(fixed);
            }
        }
        // Wider margins than leave ten columns of page are narrowed
        if let Some((w, _)) = layout.screen {
            layout.margin = layout.margin.min(w.saturating_sub(MIN_COLUMNS) / 2);
        }
        (layout, problems)
    }

    /// Read a layout file named on the command line
    pub fn load(path: &Path) -> Result<(Layout, Vec<String>), String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("couldn't read {}: {}", path.display(), e))?;
        Ok(Layout::parse(&text))
    }

    /// The layout file in settings folder `dir`, written with the defaults if it's missing
    pub fn load_or_create(dir: &Path) -> (Layout, Vec<String>) {
        let path = dir.join(LAYOUT_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Layout::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, DEFAULT_TEXT));
                (Layout::default(), Vec::new())
            }
            Err(e) => (Layout::default(), vec![format!("couldn't read {}: {}", path.display(), e)]),
        }
    }

    /// The screen, as the status line says it: "80x30" or "the whole window"
    pub fn describe(&self) -> String {
        match self.screen {
            Some((w, h)) => format!("{}x{}", w, h),
            None => "the whole window".into(),
        }
    }
}
