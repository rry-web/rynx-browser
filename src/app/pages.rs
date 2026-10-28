//! Pages the browser makes itself (start page, bookmarks, history), and keeping bookmarks and
//! history up to date.

use super::*;

impl App {
    /// Show a page the browser makes itself (about:bookmarks, about:history, about:help)
    pub(super) fn show_internal_page(&mut self, address: &str) -> bool {
        if self.show_snapshot_page(address) {
            return true;
        }
        if let Some(server) = crate::mcp::server_of(address) {
            self.open_tools(&server);
            return true;
        }
        if let Some(id) = address.strip_prefix(crate::mcp::RESULT_PAGE) {
            let result = self.tool_results.iter().find(|(n, ..)| n.to_string() == id).cloned();
            let (title, html) = result.map(|(_, title, html)| (title, html)).unwrap_or_else(|| {
                let html =
                    "<title>Result gone</title><h1>Result gone</h1><p>Only the last few tool results are kept.</p>";
                (String::from("Result gone"), html.to_string())
            });
            self.show_generated_page(self.active_tab_index, address, &title, html);
            return true;
        }
        let (title, html) = match address {
            BOOKMARKS_PAGE => ("Bookmarks", self.bookmarks.page_html()),
            HISTORY_PAGE => ("History", self.history.page_html()),
            SETTINGS_PAGE => ("Settings", self.settings_page_html()),
            crate::keys::KEYS_PAGE => ("Keys", self.keymap.page_html()),
            crate::cookies::COOKIES_PAGE => ("Cookies", self.cookies_page_html()),
            crate::update::UPDATE_PAGE => {
                self.show_update_page();
                return true;
            }
            HELP_PAGE | "about:home" => ("Start", include_str!("../../assets/help.html").to_string()),
            _ => return false,
        };
        self.show_generated_page(self.active_tab_index, address, title, html);
        true
    }

    /// A: open the Internet Archive's latest copy of this page, for pages that are gone, blocked
    /// by a CAPTCHA, or need JavaScript. Works on pages that failed to load, too.
    pub fn open_archived_copy(&mut self) {
        let tab = &self.tabs[self.active_tab_index];
        let address = tab.page_url.clone().unwrap_or_else(|| tab.url_input.clone());
        let page = Url::parse(&address).ok().filter(|u| matches!(u.scheme(), "http" | "https"));
        let message = match page {
            None => "Only web pages have archived copies",
            Some(url) if url.host_str() == Some("web.archive.org") => "This is already an archived copy",
            Some(url) if url.host_str().is_some_and(|h| h.ends_with(".i2p")) => {
                "The Internet Archive has no copies of .i2p sites"
            }
            Some(url) => {
                let original = crate::network::without_credentials(url.as_str());
                return self.follow_link(format!("https://web.archive.org/web/{}", original));
            }
        };
        self.current_tab().status_message = message.to_string();
    }

    /// Show a page the browser made in tab `index`, at `address`
    pub fn show_generated_page(&mut self, index: usize, address: &str, title: &str, html: String) {
        let width = self.terminal_width;
        let tab = &mut self.tabs[index];
        tab.url_input = address.to_string();
        tab.page_title = title.to_string();
        tab.page_url = None;
        tab.set_content(html, PageKind::Html);
        tab.reset_page_state();
        tab.restore_position_after_render();
        tab.status_message = String::from("Ready");
        self.render_tab(index, width);
    }

    /// Bookmark the current page, or remove its bookmark
    pub fn toggle_bookmark(&mut self) {
        let tab = &self.tabs[self.active_tab_index];
        // A user name or password typed into the address must never be written to disk
        let Some(url) = tab.page_url.as_deref().map(crate::network::without_credentials) else {
            self.current_tab().status_message = String::from("Only web pages can be bookmarked");
            return;
        };
        let title = tab.page_title.clone();
        let message = match self.bookmarks.toggle(&url, &title) {
            Ok(true) => String::from("Bookmarked (B shows bookmarks)"),
            Ok(false) => String::from("Bookmark removed"),
            Err(e) => e,
        };
        self.current_tab().status_message = message;
    }

    /// Add the page a tab just loaded to the history, once it's laid out
    pub(super) fn record_visit_if_pending(&mut self, tab_index: usize) {
        let tab = &mut self.tabs[tab_index];
        if !std::mem::take(&mut tab.record_visit_after_render) {
            return;
        }
        // A user name or password typed into the address must never be written to disk
        let Some(url) = tab.page_url.as_deref().map(crate::network::without_credentials) else {
            return;
        };
        let title = tab.page_title.clone();
        if let Err(e) = self.history.record(&url, &title, self.i2p_mode) {
            self.tabs[tab_index].status_message = e;
        }
    }

    /// On the history page, `D` asks for confirmation and a second `D` clears the history
    pub fn clear_history_key(&mut self) {
        if self.current_tab().url_input != HISTORY_PAGE {
            return;
        }
        if !self.confirm_clear_history {
            self.confirm_clear_history = true;
            self.current_tab().status_message = String::from("Press D again to clear all history");
            return;
        }
        self.confirm_clear_history = false;
        let message = match self.history.clear() {
            Ok(()) => String::from("History cleared"),
            Err(e) => e,
        };
        self.show_internal_page(HISTORY_PAGE);
        self.current_tab().status_message = message;
    }
}
