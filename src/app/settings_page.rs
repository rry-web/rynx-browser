//! The settings page (about:settings): every setting as a form, saved to settings.conf and applied
//! straight away. Only this browser-made page can change settings; web pages' forms can't.

use super::*;
use crate::network::escape_html;
use crate::settings::NetworkMode;

/// The address of the settings page
pub const SETTINGS_PAGE: &str = "about:settings";

/// Text fields on the page: (setting key, label, explanation)
const TEXT_FIELDS: &[(&str, &str, &str)] = &[
    ("home_page", "Home page", "Opened in new tabs. Empty: the start page with the key bindings."),
    (
        "user_agent",
        "Browser identity (User-Agent)",
        "How rynx introduces itself on the clearweb. I2P mode always sends a generic one.",
    ),
    ("i2p_proxy", "I2P proxy", "The I2P router's HTTP proxy, used in I2P mode."),
    (
        "local_names",
        "Names on your own network",
        "Public-looking names allowed to point at local addresses (a company network's), comma-separated. \
         Other public names that do are refused; printer.local, router.lan and single words never need listing.",
    ),
    (
        "download_dir",
        "Downloads folder",
        "Empty: your Downloads folder. Takes effect when rynx next starts, since the sandbox fixes it then.",
    ),
];

impl App {
    /// The settings page, filled in with the current settings
    pub(super) fn settings_page_html(&self) -> String {
        let s = &self.settings;
        let mut html = String::from(
            "<title>Settings</title><h1>Settings</h1>\
             <p><a href=\"about:history\">History</a> · <a href=\"about:bookmarks\">Bookmarks</a> · \
             <a href=\"about:snapshots\">Snapshots</a> · <a href=\"about:cookies\">Cookies</a> · <a href=\"about:keys\">Keys</a> · <a href=\"about:i2p\">I2P</a> · <a href=\"about:help\">Help</a></p>\
             <form action=\"about:settings-save\">",
        );
        // The network lock first: it's the one that matters for privacy
        let network = s.network;
        let option = |value: &str, label: &str, mode: NetworkMode| {
            let selected = if network == mode { " selected" } else { "" };
            format!("<option value=\"{value}\"{selected}>{label}</option>")
        };
        html.push_str(&format!(
            "<h2>Network</h2><p><b>Clearweb or I2P</b>: choose I2P only to keep rynx in I2P. It then starts \
             in I2P mode and p can't leave it, so a stray key press can't send a page outside I2P.<br>{}{}{}</p>",
            "<select name=\"network\">",
            [
                option("switchable", "Both: start on the clearweb, p switches", NetworkMode::Switchable),
                option("clearweb", "Clearweb only", NetworkMode::Clearweb),
                option("i2p", "I2P only: p can't leave it", NetworkMode::I2p),
            ]
            .concat(),
            "</select>",
        ));

        html.push_str(&self.router_html());

        html.push_str("<h2>Browsing</h2>");
        // Search: a choice of engines that work without JavaScript, or an address of your own
        let preset = crate::constants::SEARCH_ENGINES.iter().find(|(_, _, url)| *url == s.search_url);
        let mut engines: String = crate::constants::SEARCH_ENGINES
            .iter()
            .map(|(name, label, _)| {
                let selected = if preset.is_some_and(|(n, ..)| n == name) { " selected" } else { "" };
                format!("<option value=\"{name}\"{selected}>{}</option>", escape_html(label))
            })
            .collect();
        let custom = if preset.is_none() { " selected" } else { "" };
        engines.push_str(&format!("<option value=\"custom\"{custom}>Custom: the address below</option>"));
        html.push_str(&format!(
            "<p><b>Search engine</b>: for words typed in the address bar.<br><select name=\"search_engine\">{engines}</select></p>\
             <p><b>Custom search address</b>: used when Custom is chosen; {{query}} is replaced by the search words.<br>\
             <input type=\"text\" name=\"search_url\" size=\"80\" value=\"{}\"></p>",
            escape_html(if preset.is_none() { &s.search_url } else { "" })
        ));
        for (key, label, about) in TEXT_FIELDS {
            let value = match *key {
                "home_page" => s.home_page.clone().unwrap_or_default(),
                "user_agent" => s.user_agent.clone(),
                "i2p_proxy" => s.i2p_proxy.clone(),
                "local_names" => s.local_names.join(", "),
                _ => s.download_dir.as_ref().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
            };
            html.push_str(&format!(
                "<p><b>{}</b>: {}<br><input type=\"text\" name=\"{}\" size=\"80\" value=\"{}\"></p>",
                escape_html(label),
                escape_html(about),
                key,
                escape_html(&value)
            ));
        }
        let checkbox = |name: &str, on: bool, label: &str| {
            let checked = if on { " checked" } else { "" };
            format!("<p><input type=\"checkbox\" name=\"{name}\" value=\"on\"{checked}> {label}</p>")
        };
        // Images: which load by themselves, where they're shown, and how they're drawn
        let choice = |name: &str, current: &str, options: &[(&str, &str)]| {
            let options: String = options
                .iter()
                .map(|(value, label)| {
                    let selected = if *value == current { " selected" } else { "" };
                    format!("<option value=\"{value}\"{selected}>{label}</option>")
                })
                .collect();
            format!("<select name=\"{name}\">{options}</select>")
        };
        use crate::settings::{ImageDisplay, ImageLoading, ImageProtocol};
        let loading = match s.images {
            ImageLoading::Ask => "ask",
            ImageLoading::Site => "site",
            ImageLoading::All => "all",
        };
        let display = if s.image_display == ImageDisplay::Viewer { "viewer" } else { "inline" };
        let protocol = match s.image_protocol {
            ImageProtocol::Auto => "auto",
            ImageProtocol::Kitty => "kitty",
            ImageProtocol::Iterm2 => "iterm2",
            ImageProtocol::Sixel => "sixel",
            ImageProtocol::Blocks => "blocks",
        };
        html.push_str(&format!(
            "<h2>Images</h2>\
             <p><b>Load images</b>: images can track you and use data, so by default none load until you press I.<br>{}</p>\
             <p><b>Show loaded images</b>:<br>{}</p>\
             <p><b>Draw them with</b>: now {}. Terminals like Kitty, iTerm2, Ghostty and WezTerm show real images; \
             macOS Terminal only coloured blocks.<br>{}</p>",
            choice("images", loading, &[
                ("ask", "Only when I press I (or i for one image)"),
                ("site", "Those from the page's own site"),
                ("all", "All of them"),
            ]),
            choice("image_display", display, &[("inline", "In the page"), ("viewer", "Only full screen, with i")]),
            self.images.protocol_name(),
            choice("image_protocol", protocol, &[
                ("auto", "What the terminal supports"),
                ("kitty", "Kitty graphics"),
                ("iterm2", "iTerm2 images"),
                ("sixel", "Sixel graphics"),
                ("blocks", "Coloured text blocks"),
            ]),
        ));
        html.push_str("<h2>Updates</h2>");
        html.push_str(&checkbox(
            "updates",
            s.check_updates,
            "Check for a new rynx once a day (never in I2P mode); nothing is installed unless you agree. U shows updates.",
        ));
        html.push_str(&format!(
            "<p><b>Update to</b>:<br>{}</p>",
            choice(
                "update_channel",
                if s.beta_updates { "beta" } else { "stable" },
                &[("stable", "Stable releases"), ("beta", "Betas too: test versions of what's coming next")]
            )
        ));
        html.push_str("<h2>Privacy</h2>");
        html.push_str(&checkbox(
            "history",
            s.save_history,
            "Save history to disk (otherwise it's kept until you quit; I2P visits are never saved)",
        ));
        html.push_str(&checkbox(
            "cache",
            s.disk_cache,
            "Keep pages on disk, so Back, unchanged pages and unreachable sites load from there \
             (otherwise they're kept in memory until you quit)",
        ));
        html.push_str(&checkbox(
            "cookie_encryption",
            s.encrypt_cookies,
            "Encrypt saved cookies with a key in the system keychain (asked for once a site keeps cookies; on macOS it may ask again after updates)",
        ));
        html.push_str(&checkbox(
            "cache_i2p",
            s.disk_cache_i2p,
            "Keep I2P pages on disk too (faster on slow I2P connections, but a record of I2P visits)",
        ));
        let used = self.cache.disk_usage() as f64 / 1024.0 / 1024.0;
        html.push_str(&format!(
            "<p><b>Disk cache size (MB)</b>: the oldest pages are removed beyond it. Using {:.1} MB now.<br>\
             <input type=\"text\" name=\"cache_size\" size=\"10\" value=\"{}\"></p>",
            used, s.cache_size_mb
        ));
        html.push_str(&checkbox(
            "anubis",
            s.solve_anubis,
            "Pass Anubis bot checks by doing their proof of work, without JavaScript",
        ));
        html.push_str(
            "<p><input type=\"submit\" value=\"Save settings\"> \
             <input type=\"submit\" name=\"clear_cache\" value=\"Clear cache\"> \
             <input type=\"submit\" name=\"reset\" value=\"Restore defaults\"></p></form>",
        );
        match &self.config_dir {
            Some(dir) => html.push_str(&format!(
                "<p>Saved to {}, which can also be edited by hand (saving here rewrites it).</p>",
                escape_html(&dir.join(crate::settings::SETTINGS_FILE).to_string_lossy())
            )),
            None => html.push_str("<p>There's no settings folder on this system, so changes last until you quit.</p>"),
        }
        html
    }

    /// Save the settings page's form: check it as the settings file would be, write the file and
    /// apply the new settings. Pressed with the Restore defaults button (twice, to confirm), it
    /// puts every setting back to its default instead.
    pub(super) fn save_settings_form(&mut self, form: usize, submitter: Option<usize>) {
        let tab = &self.tabs[self.active_tab_index];
        let pressed = submitter.and_then(|i| tab.controls.get(i)).map(|c| c.name.clone()).unwrap_or_default();
        if pressed == "clear_cache" {
            let message = match self.cache.clear() {
                Ok(()) => String::from("Cache cleared, in memory and on disk"),
                Err(e) => e,
            };
            self.show_internal_page(SETTINGS_PAGE);
            self.current_tab().status_message = message;
            return;
        }
        if pressed == "reset" {
            if !std::mem::replace(&mut self.confirm_reset_settings, true) {
                self.current_tab().status_message =
                    String::from("Press Restore defaults again to put every setting back to its default");
                return;
            }
            self.confirm_reset_settings = false;
            return self.store_settings(crate::settings::Settings::default(), "Settings restored to defaults");
        }
        let mut lines = Vec::new();
        let mut history = false;
        let mut anubis = false;
        let mut cache = false;
        let mut cache_i2p = false;
        let mut updates = false;
        let mut encrypt_cookies = false;
        let mut router = false;
        let mut router_keep = false;
        let mut engine = None;
        for (control, value) in tab.controls.iter().zip(&tab.control_values) {
            if control.form != Some(form) {
                continue;
            }
            match (control.name.as_str(), value) {
                ("history", ControlValue::Checked(on)) => history = *on,
                ("anubis", ControlValue::Checked(on)) => anubis = *on,
                ("cache", ControlValue::Checked(on)) => cache = *on,
                ("updates", ControlValue::Checked(on)) => updates = *on,
                ("cookie_encryption", ControlValue::Checked(on)) => encrypt_cookies = *on,
                ("i2p_router", ControlValue::Checked(on)) => router = *on,
                ("i2p_router_keep", ControlValue::Checked(on)) => router_keep = *on,
                ("cache_i2p", ControlValue::Checked(on)) => cache_i2p = *on,
                ("search_engine", ControlValue::Selected(i)) => {
                    if let ControlKind::Select { options } = &control.kind {
                        engine = options.get(*i).map(|o| o.value.clone());
                    }
                }
                (key, ControlValue::Text(text)) if !text.trim().is_empty() => {
                    // One line per setting: a line break typed in can't add a setting
                    lines.push(format!("{} = {}", key, text.replace(['\r', '\n'], " ").trim()))
                }
                (key, ControlValue::Selected(i)) => {
                    if let ControlKind::Select { options } = &control.kind
                        && let Some(option) = options.get(*i)
                    {
                        lines.push(format!("{} = {}", key, option.value));
                    }
                }
                _ => {}
            }
        }
        // A chosen engine's address wins over whatever is in the custom field
        if let Some(url) = crate::constants::SEARCH_ENGINES
            .iter()
            .find(|(name, ..)| Some(*name) == engine.as_deref())
            .map(|(_, _, url)| url)
        {
            lines.retain(|line| !line.starts_with("search_url ="));
            lines.push(format!("search_url = {}", url));
        }
        lines.push(format!("history = {}", if history { "on" } else { "off" }));
        lines.push(format!("anubis = {}", if anubis { "on" } else { "off" }));
        lines.push(format!("cache = {}", if cache { "on" } else { "off" }));
        lines.push(format!("updates = {}", if updates { "daily" } else { "off" }));
        lines.push(format!("cache_i2p = {}", if cache_i2p { "on" } else { "off" }));
        lines.push(format!("cookie_encryption = {}", if encrypt_cookies { "keychain" } else { "off" }));
        lines.push(format!("i2p_router = {}", if router { "start" } else { "off" }));
        lines.push(format!("i2p_router_keep = {}", if router_keep { "on" } else { "off" }));

        let (settings, problems) = crate::settings::Settings::parse(&lines.join("\n"));
        if let Some(problem) = problems.first() {
            // "line 3: invalid home_page ..." means nothing on this page; the rest does
            let problem = problem.split_once(": ").map(|(_, p)| p).unwrap_or(problem);
            self.current_tab().status_message = format!("Not saved: {}", problem);
            return;
        }
        self.store_settings(settings, "Settings saved");
    }

    /// Write settings to the settings file, apply them, and show the page again with `message`
    fn store_settings(&mut self, settings: crate::settings::Settings, message: &'static str) {
        let saved = match &self.config_dir {
            Some(dir) => settings.save(dir).map(|_| message),
            None => Ok("Settings applied (no settings folder to save them in)"),
        };
        match saved {
            Ok(message) => {
                let restart = settings.download_dir != self.settings.download_dir
                    || settings.start_i2p_router != self.settings.start_i2p_router
                    || settings.keep_i2p_router != self.settings.keep_i2p_router
                    || settings.i2pd_path != self.settings.i2pd_path;
                let message = match self.apply_settings(settings) {
                    Err(e) => e,
                    Ok(()) if restart => format!("{}; some changes take effect when rynx next starts", message),
                    Ok(()) => message.to_string(),
                };
                self.show_internal_page(SETTINGS_PAGE);
                self.current_tab().status_message = message;
            }
            Err(e) => self.current_tab().status_message = e,
        }
    }

    /// Put new settings into effect without restarting (except the downloads folder, which the
    /// sandbox fixes at startup)
    fn apply_settings(&mut self, new: crate::settings::Settings) -> Result<(), String> {
        if new.user_agent != self.settings.user_agent || new.i2p_proxy != self.settings.i2p_proxy {
            // The cookies carry over to the new clients
            let network = NetworkManager::with_cookies(
                &new.i2p_proxy,
                &new.user_agent,
                Arc::clone(self.network_manager.cookies(false)),
                Arc::clone(self.network_manager.cookies(true)),
            )
            .map_err(|e| format!("Settings saved, but they couldn't be applied: {}", e))?;
            self.network_manager = Arc::new(network);
        }
        self.cache.configure(new.disk_cache, new.disk_cache_i2p, new.cache_size_mb);
        crate::dns::allow_local_names(&new.local_names);
        self.images.use_protocol(new.image_protocol);
        match new.network {
            NetworkMode::I2p => self.i2p_mode = true,
            NetworkMode::Clearweb => self.i2p_mode = false,
            NetworkMode::Switchable => {}
        }
        if new.save_history != self.settings.save_history {
            let path = new
                .save_history
                .then(|| self.config_dir.as_ref().map(|d| d.join(crate::history::HISTORY_FILE)))
                .flatten();
            self.history.save_to(path)?;
        }
        self.settings = new;
        Ok(())
    }

    /// p: switch between the clearweb and I2P, unless the settings lock it
    pub fn toggle_i2p_mode(&mut self) {
        let message = match self.settings.network {
            NetworkMode::Switchable => {
                self.i2p_mode = !self.i2p_mode;
                if self.i2p_mode {
                    "I2P mode on (to stay in it, choose I2P only in settings: S)"
                } else {
                    "I2P mode off: clearweb"
                }
            }
            NetworkMode::I2p => "I2P mode is locked on in settings (S)",
            NetworkMode::Clearweb => "I2P mode is turned off in settings (S)",
        };
        self.current_tab().status_message = message.to_string();
    }
}
