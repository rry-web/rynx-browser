//! The settings file: a few `key = value` lines in the user's config folder.

use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "settings.conf";

/// Written on first run so the options are easy to discover
const TEMPLATE: &str = "\
# rynx-browser settings. Lines starting with # are comments; remove the # to use a setting.

# Page opened in new tabs (by default, the start page with the key bindings)
# home_page = https://en.wikipedia.org/

# Search engine for words typed in the address bar. {query} is replaced by the search words.
# search_url = https://html.duckduckgo.com/html/?q={query}
# search_url = https://search.marginalia.nu/search?query={query}

# Address of the I2P router's HTTP proxy, used in I2P mode
# i2p_proxy = http://127.0.0.1:4444

# Where downloads are saved (by default, your Downloads folder)
# download_dir = /path/to/folder
";

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub home_page: Option<String>,
    /// Search URL with {query} where the search words go
    pub search_url: String,
    pub i2p_proxy: String,
    pub download_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            home_page: None,
            search_url: crate::constants::DEFAULT_SEARCH_URL.to_string(),
            i2p_proxy: crate::constants::I2P_PROXY_URL.to_string(),
            download_dir: None,
        }
    }
}

/// The folder holding settings and bookmarks
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "rynx-browser").map(|dirs| dirs.config_dir().to_path_buf())
}

fn is_web_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
}

impl Settings {
    /// Read settings from text. Problems (unknown keys, invalid values) are returned as messages
    /// and the defaults are kept for those settings.
    pub fn parse(text: &str) -> (Settings, Vec<String>) {
        let mut settings = Settings::default();
        let mut problems = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                problems.push(format!("line {}: expected key = value", number + 1));
                continue;
            };
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim();
            match key {
                "home_page" if is_web_url(value) => settings.home_page = Some(value.to_string()),
                "search_url" if is_web_url(&value.replace("{query}", "x")) => {
                    settings.search_url = value.to_string()
                }
                "i2p_proxy" if is_web_url(value) && value.starts_with("http://") => {
                    settings.i2p_proxy = value.to_string()
                }
                "download_dir" if !value.is_empty() => settings.download_dir = Some(PathBuf::from(value)),
                "home_page" | "search_url" | "i2p_proxy" | "download_dir" => {
                    problems.push(format!("line {}: invalid {} \"{}\"", number + 1, key, value))
                }
                _ => problems.push(format!("line {}: unknown setting \"{}\"", number + 1, key)),
            }
        }
        (settings, problems)
    }

    /// Load the settings file from `dir`, writing the commented template there if it's missing
    pub fn load_or_create(dir: &Path) -> (Settings, Vec<String>) {
        let path = dir.join(SETTINGS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Settings::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, TEMPLATE));
                (Settings::default(), Vec::new())
            }
            Err(e) => (Settings::default(), vec![format!("couldn't read {}: {}", path.display(), e)]),
        }
    }

    /// The address that searches for `words`
    pub fn search_address(&self, words: &str) -> String {
        let query: String = url::form_urlencoded::byte_serialize(words.as_bytes()).collect();
        if self.search_url.contains("{query}") {
            self.search_url.replace("{query}", &query)
        } else {
            format!("{}{}", self.search_url, query)
        }
    }
}
