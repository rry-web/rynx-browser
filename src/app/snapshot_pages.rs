//! The snapshot pages: tracked sites and their pages (about:snapshots), a page's snapshots, and one
//! snapshot shown as the page was. T tracks a site; Z shows the current page's snapshots.

use super::*;
use crate::network::escape_html;
use crate::snapshots::{
    PAGE_SNAPSHOTS, SNAPSHOT, SNAPSHOTS_PAGE, page_snapshots_address, parse_page_snapshots_address,
    parse_snapshot_address, site_of, snapshot_address,
};

impl App {
    /// Show a snapshot page, if `address` is one
    pub(super) fn show_snapshot_page(&mut self, address: &str) -> bool {
        let index = self.active_tab_index;
        if address == SNAPSHOTS_PAGE {
            let html = self.snapshots_overview_html();
            self.show_generated_page(index, address, "Snapshots", html);
            return true;
        }
        if address.starts_with(PAGE_SNAPSHOTS) {
            let Some(url) = parse_page_snapshots_address(address) else { return false };
            let html = self.page_snapshots_html(&url);
            self.show_generated_page(index, address, "Snapshots", html);
            return true;
        }
        if address.starts_with(SNAPSHOT) {
            let Some((url, time)) = parse_snapshot_address(address) else { return false };
            let Some(snapshot) = self.snapshots.open(&url, time) else {
                let html = format!(
                    "<title>Snapshot gone</title><h1>Snapshot gone</h1><p>There's no snapshot of {} from {}.</p>",
                    escape_html(&url),
                    fetch::when(time)
                );
                self.show_generated_page(index, address, "Snapshot gone", html);
                return true;
            };
            let page = snapshot.page;
            let banner = format!(
                "<p><b>Snapshot from {}</b> of <a href=\"{}\">{}</a>. r loads the live page; Z lists every \
                 snapshot.</p><hr>",
                fetch::when(time),
                escape_html(&url),
                escape_html(&url)
            );
            let body = match page.kind {
                PageKind::Html => page.body,
                PageKind::PlainText => format!("<pre>{}</pre>", escape_html(&page.body)),
            };
            let title = format!("Snapshot: {}", page.title);
            self.show_generated_page(index, address, &title, banner + &body);
            // Links on it lead where the page's own did, and r reloads the live page
            self.tabs[index].page_url = Some(url);
            return true;
        }
        false
    }

    fn snapshots_overview_html(&self) -> String {
        let mut html = String::from(
            "<title>Snapshots</title><h1>Snapshots</h1>\
             <p>Snapshots are kept for sites you track: press T on a page to track its site (again to stop). \
             Each visit then saves a snapshot if the page changed. Stopping keeps the snapshots taken.</p>",
        );
        let sites = self.snapshots.tracked_sites();
        if sites.is_empty() {
            html.push_str("<p>No sites are tracked yet.</p>");
        }
        for site in sites {
            html.push_str(&format!("<h2>{}</h2>", escape_html(&site)));
            let pages = self.snapshots.site_pages(&site);
            if pages.is_empty() {
                html.push_str("<p>No snapshots yet: visit a page on it.</p>");
                continue;
            }
            html.push_str("<ul>");
            for (url, count, newest) in pages {
                html.push_str(&format!(
                    "<li><a href=\"{}\">{}</a>: {} {}, latest {}</li>",
                    escape_html(&page_snapshots_address(&url)),
                    escape_html(&url),
                    count,
                    if count == 1 { "snapshot" } else { "snapshots" },
                    fetch::when(newest)
                ));
            }
            html.push_str(&format!(
                "</ul><form action=\"about:snapshots-delete\"><input type=\"hidden\" name=\"site\" value=\"{}\">\
                 <input type=\"submit\" value=\"Delete {}'s snapshots\"></form>",
                escape_html(&site),
                escape_html(&site)
            ));
        }
        html
    }

    fn page_snapshots_html(&self, url: &str) -> String {
        let mut html = format!(
            "<title>Snapshots</title><h1>Snapshots of {0}</h1><p><a href=\"{0}\">The live page</a> · \
             <a href=\"about:snapshots\">All tracked sites</a></p>",
            escape_html(url)
        );
        let times = self.snapshots.times(url);
        if times.is_empty() {
            html.push_str("<p>No snapshots of this page yet.");
            if !self.snapshots.is_tracked(url) {
                html.push_str(" Its site isn't tracked: press T on the page to track it.");
            }
            return html + "</p>";
        }
        html.push_str("<ul>");
        for time in times {
            html.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>",
                escape_html(&snapshot_address(url, time)),
                fetch::when(time)
            ));
        }
        html + "</ul>"
    }

    /// The Delete snapshots button: pressed twice in a row, it deletes the site's snapshots
    pub(super) fn delete_snapshots_form(&mut self, form: usize) {
        let tab = &self.tabs[self.active_tab_index];
        let site = tab
            .controls
            .iter()
            .find(|c| c.form == Some(form) && c.name == "site")
            .map(|c| c.value.clone())
            .unwrap_or_default();
        if self.confirm_delete_snapshots.as_deref() != Some(site.as_str()) {
            self.current_tab().status_message = format!("Press the button again to delete every snapshot of {}", site);
            self.confirm_delete_snapshots = Some(site);
            return;
        }
        self.confirm_delete_snapshots = None;
        let message = match self.snapshots.delete_site(&site) {
            Ok(()) => format!("Deleted {}'s snapshots", site),
            Err(e) => e,
        };
        self.show_snapshot_page(SNAPSHOTS_PAGE);
        self.current_tab().status_message = message;
    }

    /// T: start or stop tracking the current page's site
    pub fn toggle_tracking(&mut self) {
        let Some(url) = self.tabs[self.active_tab_index].page_url.clone() else {
            self.current_tab().status_message = String::from("Only web, Gemini and Gopher pages can be tracked");
            return;
        };
        let site = site_of(&url).unwrap_or_default();
        let message = match self.snapshots.toggle(&url) {
            Ok(true) if crate::cache::is_i2p(&url, false) => {
                format!(
                    "Tracking {}: its snapshots are saved to disk, a record of these I2P visits (Z lists them)",
                    site
                )
            }
            Ok(true) => format!("Tracking {}: visits save a snapshot when a page changed (Z lists them)", site),
            Ok(false) => format!("Stopped tracking {}; its snapshots are kept", site),
            Err(e) => e,
        };
        self.current_tab().status_message = message;
    }

    /// Z: the current page's snapshots, or every tracked site's
    pub fn open_snapshots(&mut self) {
        let page = self.tabs[self.active_tab_index].page_url.clone().filter(|url| site_of(url).is_some());
        let address = match page {
            Some(url) => page_snapshots_address(&url),
            None => SNAPSHOTS_PAGE.to_string(),
        };
        self.follow_link(address);
    }
}
