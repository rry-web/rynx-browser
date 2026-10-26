//! Plain text (text/plain, JSON, source code): lines kept, URLs made clickable.

use super::*;

/// Byte ranges of http(s) URLs in a line of plain text, without trailing punctuation
fn find_urls(line: &str) -> Vec<(usize, usize)> {
    let mut urls = Vec::new();
    let mut from = 0;
    while let Some(found) = ["https://", "http://"].iter().filter_map(|scheme| line[from..].find(scheme)).min() {
        let start = from + found;
        let mut end = line[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
            .map_or(line.len(), |i| start + i);
        // "see https://example.com/x." ends the sentence, not the URL; a closing bracket only
        // belongs to the URL if the URL opened it
        loop {
            let url = &line[start..end];
            let trim = match url.chars().last() {
                Some('.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*') => true,
                Some(')') => url.matches('(').count() < url.matches(')').count(),
                Some(']') => url.matches('[').count() < url.matches(']').count(),
                _ => false,
            };
            if !trim {
                break;
            }
            end -= 1;
        }
        if end - start > "https://".len() {
            urls.push((start, end));
        }
        from = end.max(start + 1);
    }
    urls
}
impl DomRenderer {
    /// Render plain text (text/plain, JSON, source code...). Lines and indentation are kept, long
    /// lines wrap at spaces and continue under their own indentation, and URLs become links.
    pub fn render_plain_text(&mut self, text: &str) {
        for raw_line in text.lines() {
            let line = sanitize_for_terminal(raw_line);
            let line = line.trim_end();
            if line.is_empty() {
                self.flush_line();
                self.push_blank_line();
            } else {
                self.push_plain_line(line);
            }
        }
        self.flush_line();
    }

    fn push_plain_line(&mut self, line: &str) {
        let urls = find_urls(line);
        let indent = line.len() - line.trim_start_matches(' ').len();
        // Continuation lines start under the text, but never more than halfway across
        let hanging = indent.min(self.max_width / 2);
        let mut pos = 0;
        let mut first = true;
        while pos < line.len() {
            self.flush_line();
            let lead = if first { 0 } else { hanging };
            if lead > 0 {
                self.emit(&" ".repeat(lead), Style::default(), None);
            }
            let available = self.max_width.saturating_sub(lead).max(1);

            // The most that fits, then back up to the last space if the line continues
            let mut used = 0;
            let mut fit_end = pos;
            for (i, ch) in line[pos..].char_indices() {
                if used + char_width(ch) > available && fit_end > pos {
                    break;
                }
                used += char_width(ch);
                fit_end = pos + i + ch.len_utf8();
            }
            let mut end = fit_end;
            if fit_end < line.len() {
                let chunk = &line[pos..fit_end];
                let leading_spaces = chunk.len() - chunk.trim_start_matches(' ').len();
                if let Some(space) = chunk.rfind(' ').filter(|&i| i > leading_spaces) {
                    end = pos + space;
                }
            }

            self.emit_plain(line, pos, end, &urls);
            self.line_has_text = true;
            pos = end;
            // Continuation lines get the hanging indent instead of the spaces at the break
            while line[pos..].starts_with(' ') {
                pos += 1;
            }
            first = false;
        }
        self.flush_line();
    }

    /// Draw line[start..end], making the parts inside URL ranges links to those URLs
    fn emit_plain(&mut self, line: &str, start: usize, end: usize, urls: &[(usize, usize)]) {
        let link_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
        let mut pos = start;
        for &(url_start, url_end) in urls {
            if url_end <= pos || url_start >= end {
                continue;
            }
            let link_from = url_start.max(pos);
            let link_to = url_end.min(end);
            self.emit(&line[pos..link_from], Style::default(), None);
            self.emit(&line[link_from..link_to], link_style, Some(&line[url_start..url_end]));
            pos = link_to;
        }
        self.emit(&line[pos..end], Style::default(), None);
    }
}
