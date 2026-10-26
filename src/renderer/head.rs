//! What a page announces in its <head> and rynx shows above it: site tools and feeds.

use super::*;

impl DomRenderer {
    /// An MCP server the page announces with <link rel="mcp">, as a link to its tools page
    pub(super) fn render_tools_link(&mut self, document: &Html) {
        static TOOLS: OnceLock<Selector> = OnceLock::new();
        let selector = TOOLS.get_or_init(|| Selector::parse(r#"link[rel~="mcp"][href]"#).expect("valid selector"));
        let Some(href) = document.select(selector).find_map(|link| link.value().attr("href")) else {
            return;
        };
        // The address is checked against the page when the link is followed
        self.push_text("Tools: ");
        self.push_link_text("Open this site's tools", &crate::mcp::tools_address(href.trim()));
        self.flush_line();
        self.push_blank_line();
    }

    /// RSS and Atom feeds the page announces in its <head>, as links above the page
    pub(super) fn render_feed_links(&mut self, document: &Html) {
        static FEEDS: OnceLock<Selector> = OnceLock::new();
        let selector =
            FEEDS.get_or_init(|| Selector::parse(r#"link[rel~="alternate"][href]"#).expect("valid selector"));
        let feeds: Vec<(&str, &str)> = document
            .select(selector)
            .filter_map(|link| {
                let kind = match link.value().attr("type")?.trim().to_ascii_lowercase().as_str() {
                    "application/rss+xml" => "RSS",
                    "application/atom+xml" => "Atom",
                    _ => return None,
                };
                let title = link.value().attr("title").map(str::trim).filter(|t| !t.is_empty()).unwrap_or(kind);
                Some((link.value().attr("href")?, title))
            })
            .take(5)
            .collect();
        if feeds.is_empty() {
            return;
        }
        self.push_text("Feeds: ");
        for (i, (href, title)) in feeds.into_iter().enumerate() {
            if i > 0 {
                self.push_text(" · ");
            }
            self.push_link_text(title, href);
        }
        self.flush_line();
        self.push_blank_line();
    }
}
