//! The settings file: a few `key = value` lines in the user's config folder.

use std::path::{Path, PathBuf};

pub const SETTINGS_FILE: &str = "settings.conf";

/// One setting as the file documents it: its key, the comment above it, and examples shown
/// commented out while the setting is left at its default
struct Documented {
    key: &'static str,
    about: &'static str,
    examples: &'static [&'static str],
}

const HEADER: &str = "# rynx-browser settings. Lines starting with # are comments; remove the # to use a setting.\n\
                      # They can also be changed in the browser: press S. Saving there rewrites this file.\n";

const DOCUMENTED: &[Documented] = &[
    Documented {
        key: "home_page",
        about: "Page opened in new tabs (by default, the start page with the key bindings)",
        examples: &["https://en.wikipedia.org/"],
    },
    Documented {
        key: "network",
        about: "Clearweb or I2P: \"switchable\" (the default) switches with p; \"clearweb\" or \"i2p\" stays\n\
                there, so a stray p can't leave I2P (or enter it). i2p also starts in I2P mode.",
        examples: &["i2p"],
    },
    Documented {
        key: "search_url",
        about: "Search engine for words typed in the address bar. {query} is replaced by the search words.",
        examples: &[
            "https://html.duckduckgo.com/html/?q={query}",
            "https://old-search.marginalia.nu/search?query={query}",
        ],
    },
    Documented {
        key: "i2p_proxy",
        about: "Address of the I2P router's HTTP proxy, used in I2P mode",
        examples: &["http://127.0.0.1:4444"],
    },
    Documented {
        key: "download_dir",
        about: "Where downloads are saved (by default, your Downloads folder). Takes effect when rynx starts.",
        examples: &["/path/to/folder"],
    },
    Documented {
        key: "user_agent",
        about: "How rynx introduces itself to websites (by default rynx/<version> (+<project page>)).\n\
                Sites that refuse unknown browsers may work with a common browser's, but bot walls like Anubis\n\
                then demand JavaScript. I2P mode always sends a generic one instead, so visits don't stand out.",
        examples: &[
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
        ],
    },
    Documented {
        key: "anubis",
        about: "Anubis bot checks (\"Making sure you're not a bot\") are passed by doing their proof of work\n\
                natively, without JavaScript. Turn it off to see the check's page instead.",
        examples: &["off"],
    },
    Documented {
        key: "cache",
        about: "Keep pages on disk (compressed, in rynx's cache folder), so Back, unchanged pages and sites\n\
                that can't be reached are served from it. Off by default: pages are then kept only in memory,\n\
                until you quit. A file per page visited is a record of the visit.",
        examples: &["on"],
    },
    Documented {
        key: "cache_i2p",
        about: "Keep I2P pages on disk too, where they help most on slow connections. Off by default,\n\
                separately from cache, since a disk record of I2P visits is more sensitive.",
        examples: &["on"],
    },
    Documented {
        key: "cache_size",
        about: "Most disk space the cache may use, in megabytes; the oldest pages are removed beyond it.",
        examples: &["200"],
    },
    Documented {
        key: "history",
        about: "Save the pages you visit to history.txt, so the history page (s) keeps them after you quit.\n\
                Off by default: history is then kept only until you quit. Pages visited in I2P mode are never saved.",
        examples: &["on"],
    },
];

/// Whether the browser may switch between the clearweb and I2P
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NetworkMode {
    /// `p` switches; starts on the clearweb
    Switchable,
    /// Always the clearweb; `p` does nothing
    Clearweb,
    /// Always through I2P, from the start; `p` does nothing
    I2p,
}

impl NetworkMode {
    fn name(self) -> &'static str {
        match self {
            NetworkMode::Switchable => "switchable",
            NetworkMode::Clearweb => "clearweb",
            NetworkMode::I2p => "i2p",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub home_page: Option<String>,
    /// Search URL with {query} where the search words go
    pub search_url: String,
    pub i2p_proxy: String,
    pub download_dir: Option<PathBuf>,
    /// Save visited pages to disk
    pub save_history: bool,
    /// The User-Agent sent on the clearweb
    pub user_agent: String,
    /// Pass Anubis bot checks by doing their proof of work
    pub solve_anubis: bool,
    pub network: NetworkMode,
    /// Keep pages on disk, for the clearweb and for I2P
    pub disk_cache: bool,
    pub disk_cache_i2p: bool,
    /// The disk cache's limit, in megabytes
    pub cache_size_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            home_page: None,
            search_url: crate::constants::DEFAULT_SEARCH_URL.to_string(),
            i2p_proxy: crate::constants::I2P_PROXY_URL.to_string(),
            download_dir: None,
            save_history: false,
            user_agent: crate::constants::USER_AGENT.to_string(),
            solve_anubis: true,
            network: NetworkMode::Switchable,
            disk_cache: false,
            disk_cache_i2p: false,
            cache_size_mb: crate::cache::DEFAULT_DISK_MB,
        }
    }
}

/// The folder holding settings and bookmarks
pub fn config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "rynx-browser").map(|dirs| dirs.config_dir().to_path_buf())
}

/// The folder the disk cache goes in (which the system may clear)
pub fn cache_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "rynx-browser").map(|dirs| dirs.cache_dir().to_path_buf())
}

/// The folder snapshots are kept in: the user's own data, unlike the cache
pub fn snapshots_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "rynx-browser").map(|dirs| dirs.data_dir().join("snapshots"))
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
                "search_url" if is_web_url(&value.replace("{query}", "x")) => settings.search_url = value.to_string(),
                "i2p_proxy" if is_web_url(value) && value.starts_with("http://") => {
                    settings.i2p_proxy = value.to_string()
                }
                "download_dir" if !value.is_empty() => settings.download_dir = Some(PathBuf::from(value)),
                "history" if matches!(value, "on" | "off") => settings.save_history = value == "on",
                "anubis" if matches!(value, "on" | "off") => settings.solve_anubis = value == "on",
                "cache" if matches!(value, "on" | "off") => settings.disk_cache = value == "on",
                "cache_i2p" if matches!(value, "on" | "off") => settings.disk_cache_i2p = value == "on",
                "cache_size" if value.parse::<u64>().is_ok_and(|mb| (1..=1_000_000).contains(&mb)) => {
                    settings.cache_size_mb = value.parse().unwrap_or(crate::cache::DEFAULT_DISK_MB)
                }
                "network" => match value {
                    "switchable" => settings.network = NetworkMode::Switchable,
                    "clearweb" => settings.network = NetworkMode::Clearweb,
                    "i2p" => settings.network = NetworkMode::I2p,
                    _ => problems.push(format!(
                        "line {}: invalid network \"{}\" (switchable, clearweb or i2p)",
                        number + 1,
                        value
                    )),
                },
                // Sent as a header: printable ASCII only, so it can't add headers or break requests
                "user_agent"
                    if !value.is_empty() && value.len() <= 256 && value.bytes().all(|b| (0x20..0x7f).contains(&b)) =>
                {
                    settings.user_agent = value.to_string()
                }
                "home_page" | "search_url" | "i2p_proxy" | "download_dir" | "history" | "user_agent" | "anubis"
                | "cache" | "cache_i2p" | "cache_size" => {
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
                let _ = std::fs::create_dir_all(dir)
                    .and_then(|_| std::fs::write(&path, Settings::default().to_file_text()));
                (Settings::default(), Vec::new())
            }
            Err(e) => (Settings::default(), vec![format!("couldn't read {}: {}", path.display(), e)]),
        }
    }

    /// The value written to the file for `key`, if it isn't the default
    fn file_value(&self, key: &str) -> Option<String> {
        let default = Settings::default();
        let changed = |value: String, default_value: String| (value != default_value).then_some(value);
        match key {
            "home_page" => self.home_page.clone(),
            "network" => changed(self.network.name().into(), default.network.name().into()),
            "search_url" => changed(self.search_url.clone(), default.search_url),
            "i2p_proxy" => changed(self.i2p_proxy.clone(), default.i2p_proxy),
            "download_dir" => self.download_dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
            "user_agent" => changed(self.user_agent.clone(), default.user_agent),
            "anubis" => (!self.solve_anubis).then(|| "off".to_string()),
            "history" => self.save_history.then(|| "on".to_string()),
            "cache" => self.disk_cache.then(|| "on".to_string()),
            "cache_i2p" => self.disk_cache_i2p.then(|| "on".to_string()),
            "cache_size" => changed(self.cache_size_mb.to_string(), default.cache_size_mb.to_string()),
            _ => None,
        }
    }

    /// The settings file's text: every setting documented, with those that differ from the
    /// defaults set and the rest shown as commented-out examples
    pub fn to_file_text(&self) -> String {
        let mut text = String::from(HEADER);
        for setting in DOCUMENTED {
            text.push('\n');
            for line in setting.about.lines() {
                text.push_str(&format!("# {}\n", line.trim()));
            }
            match self.file_value(setting.key) {
                Some(value) => text.push_str(&format!("{} = {}\n", setting.key, value)),
                None => {
                    for example in setting.examples {
                        text.push_str(&format!("# {} = {}\n", setting.key, example));
                    }
                }
            }
        }
        text
    }

    /// Write these settings to the settings file in `dir`, atomically
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(SETTINGS_FILE);
        let temporary = path.with_extension("conf.tmp");
        std::fs::create_dir_all(dir)
            .and_then(|_| std::fs::write(&temporary, self.to_file_text()))
            .and_then(|_| std::fs::rename(&temporary, &path))
            .map_err(|e| format!("Couldn't save settings: {}", e))
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
