use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use scraper::{Html, Node};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct DomRenderer {
    pub lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    style_stack: Vec<Style>,
    pub links: Vec<crate::LinkRegion>,
    max_width: usize,
    current_line_width: usize,
    active_link_url: Option<String>,
    preserve_whitespace: bool,
    list_depth: usize,
}

impl DomRenderer {
    /// Get the current style from the top of the stack
    fn current_style(&self) -> Style {
        *self.style_stack.last().unwrap_or(&Style::default())
    }

    /// Push a new style onto the stack
    fn push_style(&mut self, style: Style) {
        self.style_stack.push(style);
    }

    /// Pop the current style from the stack
    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    pub fn new(width: usize) -> Self {
        Self {
            lines: Vec::new(),
            current_line: Vec::new(),
            style_stack: vec![Style::default()],
            links: Vec::new(),
            max_width: width.saturating_sub(2),
            current_line_width: 0,
            active_link_url: None,
            preserve_whitespace: false,
            list_depth: 0,
        }
    }

    pub fn render(&mut self, document: &Html) {
        for node in document.tree.root().children() {
            self.walk(node);
        }
        self.flush_line();
    }

    fn flush_line(&mut self) {
        if !self.current_line.is_empty() {
            self.lines.push(Line::from(self.current_line.clone()));
            self.current_line.clear();
            self.current_line_width = 0;
        }
    }

    fn add_vertical_space(&mut self) {
        self.flush_line();
        if let Some(last) = self.lines.last() {
            if !last.spans.is_empty() {
                self.lines.push(Line::from(""));
            }
        }
    }

    /// Internal helper to push a span to the current line and track its link region
    fn push_span_to_line(&mut self, content: String) {
        let width = UnicodeWidthStr::width(content.as_str());
        let start_x = self.current_line_width;
        let end_x = start_x + width;

        self.current_line
            .push(Span::styled(content, self.current_style()));
        self.current_line_width += width;

        // Track link regions
        if let Some(url) = &self.active_link_url {
            let line_idx = self.lines.len();

            // Try to merge with the previous link region if it's on the same line and contiguous
            if let Some(last) = self.links.last_mut() {
                if last.line_index == line_idx && last.url == *url && last.x_end == start_x {
                    last.x_end = end_x;
                    return;
                }
            }

            // Otherwise, create a new link region
            self.links.push(crate::LinkRegion {
                url: url.clone(),
                line_index: line_idx,
                x_start: start_x,
                x_end: end_x,
            });
        }
    }

    /// Ensures indentation is applied at the start of a wrapped line
    fn apply_indentation(&mut self) {
        if self.current_line_width == 0 && self.list_depth > 0 {
            let indent = "  ".repeat(self.list_depth);
            self.push_span_to_line(indent);
        }
    }

    fn push_word(&mut self, word: &str) {
        // Skip leading spaces on wrapped lines to keep the left margin clean
        if word == " " && self.current_line_width == 0 && !self.preserve_whitespace {
            return;
        }

        // Ensure indentation is applied at the start of any word on a new line
        self.apply_indentation();

        let word_width = UnicodeWidthStr::width(word);

        // Case 1: Word fits on the current line
        if self.current_line_width + word_width <= self.max_width {
            self.push_span_to_line(word.to_string());
        }
        // Case 2: Word fits on a new line (Standard Wrap)
        else if word_width <= self.max_width {
            self.flush_line();
            self.apply_indentation(); // Apply indent to the new line
            self.push_span_to_line(word.to_string());
        }
        // Case 3: Word is huge (Hard Wrap - e.g., a very long URL)
        else {
            if self.current_line_width > 0 {
                self.flush_line();
            }

            let mut remaining = word;
            while !remaining.is_empty() {
                self.apply_indentation();

                let available_space = self.max_width.saturating_sub(self.current_line_width);
                if available_space == 0 {
                    self.flush_line();
                    continue;
                }

                let mut current_width = 0;
                let mut split_idx = 0;
                for (idx, ch) in remaining.char_indices() {
                    let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
                    if current_width + char_width > available_space {
                        break;
                    }
                    current_width += char_width;
                    split_idx = idx + ch.len_utf8();
                }

                if split_idx == 0 && !remaining.is_empty() {
                    if let Some((idx, ch)) = remaining.char_indices().next() {
                        split_idx = idx + ch.len_utf8();
                    }
                }

                let (chunk, rest) = remaining.split_at(split_idx);
                self.push_span_to_line(chunk.to_string());
                remaining = rest;

                if !remaining.is_empty() {
                    self.flush_line();
                }
            }
        }
    }

    fn walk(&mut self, node: ego_tree::NodeRef<scraper::node::Node>) {
        match node.value() {
            Node::Text(text) => {
                if self.preserve_whitespace {
                    for line in text.text.lines() {
                        self.push_word(line);
                        self.flush_line();
                    }
                } else {
                    for word in text.text.split_whitespace() {
                        if self.current_line_width > 0 && !self.current_line.is_empty() {
                            // Add a space between words if we aren't at the start of a line
                            self.push_word(" ");
                        }
                        self.push_word(word);
                    }
                }
            }
            Node::Element(elem) => {
                let tag = elem.name();

                // 1. Skip Data and Hidden Tags
                if tag == "script"
                    || tag == "style"
                    || tag == "head"
                    || tag == "meta"
                    || tag == "link"
                {
                    return;
                }
                if elem.attr("hidden").is_some() || elem.attr("aria-hidden") == Some("true") {
                    return;
                }

                let old_link = self.active_link_url.clone();
                let old_preserve = self.preserve_whitespace;

                match tag {
                    "b" | "strong" => {
                        let new_style = self.current_style().add_modifier(Modifier::BOLD);
                        self.push_style(new_style);
                    }
                    "i" | "em" => {
                        let new_style = self.current_style().add_modifier(Modifier::ITALIC);
                        self.push_style(new_style);
                    }
                    "a" => {
                        let new_style = self
                            .current_style()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::UNDERLINED);
                        self.push_style(new_style);
                        if let Some(href) = elem.attr("href") {
                            self.active_link_url = Some(href.to_string());
                        }
                    }
                    "h1" | "h2" | "h3" => {
                        self.add_vertical_space();
                        let new_style = self
                            .current_style()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD);
                        self.push_style(new_style);
                    }
                    "pre" | "code" => {
                        self.flush_line();
                        self.preserve_whitespace = true;
                        let new_style = self.current_style().fg(Color::Magenta); // Distinct color for code
                        self.push_style(new_style);
                    }
                    "ul" | "ol" => {
                        self.flush_line();
                        self.list_depth += 1;
                    }
                    "li" => {
                        self.flush_line();
                        let bullet =
                            format!("{}• ", "  ".repeat(self.list_depth.saturating_sub(1)));
                        self.push_word(&bullet);
                    }
                    "img" => {
                        let alt = elem.attr("alt").unwrap_or("IMAGE");
                        let new_style = self.current_style().fg(Color::DarkGray);
                        self.push_style(new_style);
                        self.push_word(&format!("[{}] ", alt));
                        self.pop_style();
                    }
                    "br" => self.flush_line(),
                    "p" | "main" | "article" | "section" | "table" | "aside" => {
                        self.add_vertical_space()
                    }
                    "div" | "header" | "footer" | "nav" | "tr" => self.flush_line(),
                    "td" | "th" => self.push_word("  "),
                    "hr" => {
                        self.add_vertical_space();
                        self.push_word(&"-".repeat(self.max_width));
                        self.add_vertical_space();
                    }
                    _ => {}
                }

                for child in node.children() {
                    self.walk(child);
                }

                // Pop style from stack for tags that push styles
                match tag {
                    "b" | "strong" | "i" | "em" | "a" | "h1" | "h2" | "h3" | "pre" | "code" => {
                        self.pop_style();
                    }
                    _ => {}
                }

                // Restore other state
                self.active_link_url = old_link;
                self.preserve_whitespace = old_preserve;

                match tag {
                    "ul" | "ol" => {
                        self.list_depth = self.list_depth.saturating_sub(1);
                        self.flush_line();
                    }
                    "h1" | "h2" | "h3" | "p" | "main" | "article" | "section" | "table"
                    | "aside" | "pre" => self.add_vertical_space(),
                    "div" | "li" | "header" | "footer" | "nav" | "tr" => self.flush_line(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
