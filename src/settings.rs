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
        key: "i2p_router",
        about: "Start the I2P router, i2pd, when rynx starts, if it's installed and no router is already\n\
                running at i2p_proxy. Off by default. It has its own data folder, apart from any i2pd set up by hand.",
        examples: &["start"],
    },
    Documented {
        key: "i2p_router_keep",
        about: "Keep that router running after rynx quits (so I2P is ready sooner next time, and it can carry\n\
                other people's traffic, as routers do). Off by default: it stops when rynx quits.",
        examples: &["on"],
    },
    Documented {
        key: "i2pd_path",
        about: "Where the i2pd program is, if it isn't on PATH or in the usual places",
        examples: &["/opt/i2pd/i2pd"],
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
        key: "updates",
        about: "Check GitHub for a new rynx once a day (\"daily\", the default) or never (\"off\"; U still checks).\n\
                Never while locked to I2P or started in I2P mode. An update is only installed when you agree.",
        examples: &["off"],
    },
    Documented {
        key: "update_channel",
        about: "Which releases to update to: \"beta\" (the default, while rynx is in beta) or \"stable\".",
        examples: &["stable"],
    },
    Documented {
        key: "images",
        about: "Which images load by themselves: \"ask\" (the default: none; I loads a page's images),\n\
                \"site\" (those from the page's own site; others are often trackers), or \"all\".",
        examples: &["site"],
    },
    Documented {
        key: "image_display",
        about: "Where loaded images are shown: \"inline\" (the default, in the page) or \"viewer\" (only\n\
                full screen, with i).",
        examples: &["viewer"],
    },
    Documented {
        key: "image_protocol",
        about: "How images are drawn: \"auto\" (the default: asks the terminal), \"kitty\", \"iterm2\",\n\
                \"sixel\", or \"blocks\" (coloured text blocks, for terminals without graphics, like macOS Terminal).",
        examples: &["blocks"],
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

/// Which images load by themselves
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageLoading {
    /// None; I loads a page's images
    Ask,
    /// Those from the page's own site
    Site,
    All,
}

/// Where loaded images are shown
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageDisplay {
    Inline,
    Viewer,
}

/// How images are drawn
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageProtocol {
    /// Ask the terminal what it supports
    Auto,
    Kitty,
    Iterm2,
    Sixel,
    /// Coloured text blocks
    Blocks,
}

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
    pub images: ImageLoading,
    pub image_display: ImageDisplay,
    pub image_protocol: ImageProtocol,
    /// Check for updates once a day
    pub check_updates: bool,
    /// Update to pre-releases (betas) too
    pub beta_updates: bool,
    /// Start i2pd when rynx starts, and keep it running after
    pub start_i2p_router: bool,
    pub keep_i2p_router: bool,
    pub i2pd_path: Option<PathBuf>,
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
            images: ImageLoading::Ask,
            image_display: ImageDisplay::Inline,
            image_protocol: ImageProtocol::Auto,
            check_updates: true,
            beta_updates: true,
            start_i2p_router: false,
            keep_i2p_router: false,
            i2pd_path: None,
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
                "i2p_router" if matches!(value, "start" | "off") => settings.start_i2p_router = value == "start",
                "i2p_router_keep" if matches!(value, "on" | "off") => settings.keep_i2p_router = value == "on",
                "i2pd_path" if !value.is_empty() => settings.i2pd_path = Some(PathBuf::from(value)),
                "history" if matches!(value, "on" | "off") => settings.save_history = value == "on",
                "anubis" if matches!(value, "on" | "off") => settings.solve_anubis = value == "on",
                "images" if matches!(value, "ask" | "site" | "all") => {
                    settings.images = match value {
                        "site" => ImageLoading::Site,
                        "all" => ImageLoading::All,
                        _ => ImageLoading::Ask,
                    }
                }
                "image_display" if matches!(value, "inline" | "viewer") => {
                    settings.image_display = if value == "viewer" { ImageDisplay::Viewer } else { ImageDisplay::Inline }
                }
                "image_protocol" if matches!(value, "auto" | "kitty" | "iterm2" | "sixel" | "blocks") => {
                    settings.image_protocol = match value {
                        "kitty" => ImageProtocol::Kitty,
                        "iterm2" => ImageProtocol::Iterm2,
                        "sixel" => ImageProtocol::Sixel,
                        "blocks" => ImageProtocol::Blocks,
                        _ => ImageProtocol::Auto,
                    }
                }
                "updates" if matches!(value, "daily" | "off") => settings.check_updates = value == "daily",
                "update_channel" if matches!(value, "beta" | "stable") => settings.beta_updates = value == "beta",
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
                | "cache" | "cache_i2p" | "cache_size" | "images" | "image_display" | "image_protocol" | "updates"
                | "update_channel" | "i2p_router" | "i2p_router_keep" | "i2pd_path" => {
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
            "images" => match self.images {
                ImageLoading::Ask => None,
                ImageLoading::Site => Some("site".to_string()),
                ImageLoading::All => Some("all".to_string()),
            },
            "image_display" => (self.image_display == ImageDisplay::Viewer).then(|| "viewer".to_string()),
            "image_protocol" => match self.image_protocol {
                ImageProtocol::Auto => None,
                ImageProtocol::Kitty => Some("kitty".to_string()),
                ImageProtocol::Iterm2 => Some("iterm2".to_string()),
                ImageProtocol::Sixel => Some("sixel".to_string()),
                ImageProtocol::Blocks => Some("blocks".to_string()),
            },
            "updates" => (!self.check_updates).then(|| "off".to_string()),
            "update_channel" => (!self.beta_updates).then(|| "stable".to_string()),
            "cache" => self.disk_cache.then(|| "on".to_string()),
            "cache_i2p" => self.disk_cache_i2p.then(|| "on".to_string()),
            "i2p_router" => self.start_i2p_router.then(|| "start".to_string()),
            "i2p_router_keep" => self.keep_i2p_router.then(|| "on".to_string()),
            "i2pd_path" => self.i2pd_path.as_ref().map(|d| d.to_string_lossy().into_owned()),
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
