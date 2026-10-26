use ratatui::text::Line;

/// A page in a tab's history, with where it was scrolled to
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub url: String,
    pub scroll: usize,
    pub cursor_line: usize,
}

#[derive(Clone)]
pub struct LinkRegion {
    /// Where the link goes; empty for a form control
    pub url: String,
    /// The form control drawn here (index into the tab's controls), instead of a link
    pub control: Option<usize>,
    pub line_index: usize,
    pub x_start: usize,
    pub x_end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
    Visual,
    Search,
    /// Typing into a form field
    FormEdit,
}

pub struct Selection {
    pub start_line: usize,
    pub start_char: usize,
    pub end_line: usize,
    pub end_char: usize,
}

pub struct SearchMatch {
    pub line_index: usize,
    /// Screen columns of the match on its line
    pub start_char: usize,
    pub end_char: usize,
}

pub struct SearchState {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current_match_index: usize,
}

impl Selection {
    /// Extract the selected text from rendered content lines
    pub fn extract_text(&self, rendered_content: &[Line]) -> String {
        // Normalize selection (handle backwards selection)
        let (s_line, s_char, e_line, e_char) =
            if (self.start_line, self.start_char) <= (self.end_line, self.end_char) {
                (
                    self.start_line,
                    self.start_char,
                    self.end_line,
                    self.end_char,
                )
            } else {
                (
                    self.end_line,
                    self.end_char,
                    self.start_line,
                    self.start_char,
                )
            };

        let mut result = String::new();
        for i in s_line..=e_line {
            if let Some(line) = rendered_content.get(i) {
                let line_str = line.to_string();
                let start = if i == s_line { s_char } else { 0 };
                let end = if i == e_line {
                    e_char
                } else {
                    line_str.chars().count()
                };

                // Map char index to byte index for proper UTF-8 handling
                let byte_start = line_str
                    .char_indices()
                    .nth(start)
                    .map(|(idx, _)| idx)
                    .unwrap_or(0);
                let byte_end = line_str
                    .char_indices()
                    .nth(end)
                    .map(|(idx, _)| idx)
                    .unwrap_or(line_str.len());

                result.push_str(&line_str[byte_start..byte_end]);
                if i < e_line {
                    result.push('\n');
                }
            }
        }
        result
    }
}

pub enum DownloadStatus {
    Active,
    Completed,
    Failed(String),
}

pub struct Download {
    pub _id: usize,
    pub filename: String,
    pub bytes_downloaded: u64,
    pub total_size: Option<u64>,
    pub status: DownloadStatus,
}

pub struct DownloadPrompt {
    pub url: String,
    pub filename: String,
    pub target_path: std::path::PathBuf,
    pub file_exists: bool,
}
