#[derive(Clone)]
pub struct LinkRegion {
    pub url: String,
    pub line_index: usize,
    pub x_start: usize,
    pub x_end: usize,
}

pub struct PageMetadata {
    pub title: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
    Visual,
}

pub struct Selection {
    pub start_line: usize,
    pub start_char: usize,
    pub end_line: usize,
    pub end_char: usize,
}
