//! Bookmarks, kept in a plain text file in the config folder: one "URL<TAB>title" per line.

use std::path::PathBuf;

pub const BOOKMARKS_FILE: &str = "bookmarks.txt";

/// The address of the bookmarks page
pub const BOOKMARKS_PAGE: &str = "about:bookmarks";

#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Default)]
pub struct Bookmarks {
    pub entries: Vec<Bookmark>,
    /// Where they're saved; None keeps them in memory only (no config folder, or tests)
    path: Option<PathBuf>,
}

impl Bookmarks {
    pub fn load(path: PathBuf) -> Bookmarks {
        let entries = std::fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .filter_map(|line| {
                        let (url, title) = line.split_once('\t').unwrap_or((line, ""));
                        let url = url.trim();
                        (!url.is_empty()).then(|| Bookmark { url: url.to_string(), title: title.trim().to_string() })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Bookmarks { entries, path: Some(path) }
    }

    pub fn contains(&self, url: &str) -> bool {
        self.entries.iter().any(|b| b.url == url)
    }

    /// Add `url` if it isn't bookmarked, or remove it if it is. Returns whether it's now
    /// bookmarked, or an error if it couldn't be saved.
    pub fn toggle(&mut self, url: &str, title: &str) -> Result<bool, String> {
        let added = if let Some(i) = self.entries.iter().position(|b| b.url == url) {
            self.entries.remove(i);
            false
        } else {
            // Tabs and line breaks would corrupt the file format
            let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
            self.entries.push(Bookmark { url: clean(url), title: clean(title) });
            true
        };
        self.save()?;
        Ok(added)
    }

    /// Write the file atomically: a crash mid-write leaves the old file, not a truncated one
    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text: String = self.entries.iter().map(|b| format!("{}\t{}\n", b.url, b.title)).collect();
        let temporary = path.with_extension("txt.tmp");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save bookmarks: {}", e))?;
        }
        std::fs::write(&temporary, text)
            .and_then(|_| std::fs::rename(&temporary, path))
            .map_err(|e| format!("Couldn't save bookmarks: {}", e))
    }

    /// The bookmarks page
    pub fn page_html(&self) -> String {
        use crate::network::escape_html;
        let mut html = String::from("<title>Bookmarks</title><h1>Bookmarks</h1>");
        if self.entries.is_empty() {
            html.push_str("<p>No bookmarks yet. Press <b>b</b> on a page to bookmark it, and again to remove it.</p>");
        } else {
            html.push_str("<ul>");
            for bookmark in &self.entries {
                let title = if bookmark.title.is_empty() { &bookmark.url } else { &bookmark.title };
                html.push_str(&format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    escape_html(&bookmark.url),
                    escape_html(title)
                ));
            }
            html.push_str("</ul><p>To remove one, open it and press <b>b</b>.</p>");
        }
        html
    }
}
