use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use scraper::{Html, Node};

pub struct DomRenderer {
    pub lines: Vec<Line<'static>>,
    current_line: Vec<Span<'static>>,
    current_style: Style,
    pub links: Vec<crate::LinkRegion>,
    max_width: usize,
    current_line_width: usize,
    active_link_url: Option<String>,
    preserve_whitespace: bool,
    list_depth: usize,
}

impl DomRenderer {
    pub fn new(width: usize) -> Self {
        Self {
            lines: Vec::new(),
            current_line: Vec::new(),
            current_style: Style::default(),
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

    fn push_word(&mut self, word: &str) {
        let span = Span::styled(word.to_string(), self.current_style);
        let word_len = span.width(); 
        
        if self.current_line_width + word_len > self.max_width {
            self.flush_line();
            // Maintain indentation for lists when wrapping
            if self.list_depth > 0 {
                let indent = "  ".repeat(self.list_depth);
                self.current_line.push(Span::from(indent.clone()));
                self.current_line_width = indent.len();
            }
        }

        let start_x = self.current_line_width;
        let end_x = start_x + word_len;

        self.current_line.push(span);
        self.current_line_width += word_len;

        if let Some(url) = &self.active_link_url {
            let line_idx = self.lines.len();
            if let Some(last) = self.links.last_mut() {
                if last.line_index == line_idx && last.url == *url && last.x_end == start_x {
                    last.x_end = end_x;
                    return;
                }
            }
            self.links.push(crate::LinkRegion {
                url: url.clone(),
                line_index: line_idx,
                x_start: start_x,
                x_end: end_x,
            });
        }
    }

    fn walk(&mut self, node: ego_tree::NodeRef<scraper::node::Node>) {
        match node.value() {
            Node::Text(text) => {
                if self.preserve_whitespace {
                    // Split by actual newlines in code blocks
                    for line in text.text.lines() {
                        self.push_word(line);
                        self.flush_line();
                    }
                } else {
                    let content = text.text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !content.is_empty() {
                        let trailing = if text.text.ends_with(char::is_whitespace) { " " } else { "" };
                        self.push_word(&format!("{}{}", content, trailing));
                    }
                }
            }
            Node::Element(elem) => {
                let tag = elem.name();
                
                // 1. Skip Data and Hidden Tags
                if tag == "script" || tag == "style" || tag == "head" || tag == "meta" || tag == "link" {
                    return;
                }
                if elem.attr("hidden").is_some() || elem.attr("aria-hidden") == Some("true") {
                    return;
                }

                let old_style = self.current_style;
                let old_link = self.active_link_url.clone();
                let old_preserve = self.preserve_whitespace;

                match tag {
                    "b" | "strong" => self.current_style = self.current_style.add_modifier(Modifier::BOLD),
                    "i" | "em" => self.current_style = self.current_style.add_modifier(Modifier::ITALIC),
                    "a" => {
                        self.current_style = self.current_style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
                        if let Some(href) = elem.attr("href") {
                            self.active_link_url = Some(href.to_string());
                        }
                    },
                    "h1" | "h2" | "h3" => {
                        self.add_vertical_space();
                        self.current_style = self.current_style.fg(Color::White).add_modifier(Modifier::BOLD);
                    }
                    "pre" | "code" => {
                        self.flush_line();
                        self.preserve_whitespace = true;
                        self.current_style = self.current_style.fg(Color::Magenta); // Distinct color for code
                    }
                    "ul" | "ol" => {
                        self.flush_line();
                        self.list_depth += 1;
                    }
                    "li" => {
                        self.flush_line();
                        let bullet = format!("{}• ", "  ".repeat(self.list_depth.saturating_sub(1)));
                        self.push_word(&bullet);
                    }
                    "img" => {
                        let alt = elem.attr("alt").unwrap_or("IMAGE");
                        self.current_style = self.current_style.fg(Color::DarkGray);
                        self.push_word(&format!("[{}] ", alt));
                        self.current_style = old_style;
                    }
                    "br" => self.flush_line(),
                    "p" | "main" | "article" | "section" | "table" | "aside" => self.add_vertical_space(),
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

                // Restore state
                self.current_style = old_style;
                self.active_link_url = old_link;
                self.preserve_whitespace = old_preserve;

                match tag {
                    "ul" | "ol" => {
                        self.list_depth = self.list_depth.saturating_sub(1);
                        self.flush_line();
                    }
                    "h1" | "h2" | "h3" | "p" | "main" | "article" | "section" | "table" | "aside" | "pre" => self.add_vertical_space(),
                    "div" | "li" | "header" | "footer" | "nav" | "tr" => self.flush_line(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
