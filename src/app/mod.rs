use crate::bookmarks::{BOOKMARKS_PAGE, Bookmarks};
use crate::constants::{
    DEFAULT_TAB_INDEX, DOWNLOAD_PROGRESS_INTERVAL_MS, INITIAL_ID_COUNTER, INITIAL_TAB_ID, MAX_PAGE_SIZE_BYTES,
};
use crate::forms::{ControlKind, ControlValue, Form, FormControl, FormMethod};
use crate::history::{HISTORY_PAGE, History};
use crate::models::{HistoryEntry, InputMode, LinkRegion, SearchState, Selection};
use crate::network::{
    ContentClass, NetworkManager, NetworkResponse, PageKind, attempt_jump, classify_content_type, fragment_of,
    should_attempt_jump,
};
use crate::renderer::DomRenderer;
use crate::renderer::{RenderJob, RenderedPage, render_page};
use crate::settings::Settings;

use ratatui::text::Line;
use scraper::Html;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use url::Url;

use directories::UserDirs;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt; // Required for streaming to file

mod cookies_page;
mod downloads;
mod fetch;
mod images;
mod pages;
mod router;
mod settings_page;
mod snapshot_pages;
mod tab;
mod tools;
mod update_page;

pub use images::draws_images;
pub use settings_page::SETTINGS_PAGE;
pub use tab::BrowserTab;

/// Status shown while a new page is laid out in the background
pub const RENDERING_STATUS: &str = "Rendering...";

pub struct App {
    pub tabs: Vec<BrowserTab>,
    pub active_tab_index: usize,
    pub id_counter: usize,
    pub tx: mpsc::Sender<NetworkResponse>,
    pub rx: mpsc::Receiver<NetworkResponse>,
    pub i2p_mode: bool,
    /// Where settings, bookmarks and history are saved; None if the system has no such folder
    pub config_dir: Option<PathBuf>,
    /// None when no system clipboard is available (e.g. SSH or a headless session)
    pub clipboard: Option<arboard::Clipboard>,
    pub network_manager: Arc<NetworkManager>,
    pub download_dir: PathBuf,
    /// Lay pages out on a background thread (the default), so slow pages never freeze the UI.
    /// When false, rendering happens inline, which tests use for deterministic results.
    pub render_in_background: bool,
    pub settings: Settings,
    pub bookmarks: Bookmarks,
    pub history: History,
    /// Pages kept for Back/Forward, for not downloading unchanged pages, and for when a site can't
    /// be reached
    pub cache: Arc<crate::cache::Cache>,
    /// Dated copies of tracked sites' pages
    pub snapshots: Arc<crate::snapshots::Snapshots>,
    /// Images: decoded, loading, and how they're drawn on this terminal
    pub images: images::Images,
    /// How this copy was installed (set at startup, before the sandbox), and the newest release
    /// found; where updates are downloaded to; a note from installing one at startup
    pub installed: crate::update::Installed,
    pub update_found: Option<crate::update::Release>,
    pub update_dir: Option<PathBuf>,
    pub update_note: Option<String>,
    /// Where releases are listed, and the key their checksums must be signed with (tests change them)
    pub update_api: String,
    /// The keys an update must be signed with (one of)
    pub release_keys: Vec<[u8; 32]>,
    /// The I2P router rynx started, if any, and how far it has got
    pub router_state: crate::i2pd::RouterState,
    pub router_progress: Option<crate::i2pd::Progress>,
    router_ready_announced: bool,
    /// Tabs closed lately, to reopen with X (the last CLOSED_TABS_KEPT)
    pub closed_tabs: Vec<BrowserTab>,
    /// The browsing keys, from keys.conf
    pub keymap: crate::keys::Keymap,
    /// Certificates seen for Gemini hosts (trust on first use)
    pub gemini_hosts: Arc<std::sync::Mutex<crate::gemini::KnownHosts>>,
    /// MCP sessions with the servers whose tools have been used
    pub mcp: Arc<crate::mcp::Client>,
    /// The tools each server listed, for turning a form into a tool call
    pub mcp_tools: HashMap<String, Vec<crate::mcp::Tool>>,
    /// Recent tool results, shown at about:tool-result?id=<n>: (n, title, page)
    pub tool_results: std::collections::VecDeque<(u64, String, String)>,
    next_result_id: u64,
    /// The site whose Delete snapshots button was pressed once; a second press deletes them
    pub confirm_delete_snapshots: Option<String>,
    /// Restore defaults was pressed once on the settings page; a second press restores them
    pub confirm_reset_settings: bool,
    /// `D` was pressed once on the history page; a second press clears it
    pub confirm_clear_history: bool,
    /// Latest terminal width, for laying out pages the browser makes itself
    pub terminal_width: u16,
    /// The terminal's height, for drawing images no taller than the screen
    pub terminal_height: u16,
}

/// The start page with the key bindings
pub const HELP_PAGE: &str = "about:help";

/// How many closed tabs are kept to reopen
const CLOSED_TABS_KEPT: usize = 10;

impl App {
    pub fn new(
        tx: mpsc::Sender<NetworkResponse>,
        rx: mpsc::Receiver<NetworkResponse>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_settings(tx, rx, Settings::default())
    }

    pub fn with_settings(
        tx: mpsc::Sender<NetworkResponse>,
        rx: mpsc::Receiver<NetworkResponse>,
        settings: Settings,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let initial_tab = BrowserTab::new(INITIAL_TAB_ID, HELP_PAGE.to_string());
        let network_manager = Arc::new(NetworkManager::with_settings(&settings.i2p_proxy, &settings.user_agent)?);
        crate::reticulum::set_bridge(&settings.reticulum_bridge);
        let download_dir = settings.download_dir.clone().unwrap_or_else(Self::default_download_dir);
        Ok(Self {
            tabs: vec![initial_tab],
            active_tab_index: DEFAULT_TAB_INDEX,
            id_counter: INITIAL_ID_COUNTER,
            tx,
            rx,
            // Locked to I2P in the settings: start there
            i2p_mode: settings.network == crate::settings::NetworkMode::I2p,
            config_dir: None,
            clipboard: arboard::Clipboard::new().ok(),
            network_manager,
            download_dir,
            render_in_background: true,
            settings,
            bookmarks: Bookmarks::default(),
            history: History::default(),
            gemini_hosts: Arc::default(),
            cache: Arc::default(),
            snapshots: Arc::default(),
            images: images::Images::default(),
            installed: crate::update::Installed::Elsewhere(String::from("not checked")),
            update_found: None,
            update_dir: None,
            update_note: None,
            update_api: crate::update::RELEASES_API.to_string(),
            release_keys: crate::update::RELEASE_KEYS.to_vec(),
            keymap: crate::keys::Keymap::default(),
            closed_tabs: Vec::new(),
            router_state: crate::i2pd::RouterState::NotManaged,
            router_progress: None,
            router_ready_announced: false,
            confirm_clear_history: false,
            confirm_reset_settings: false,
            confirm_delete_snapshots: None,
            mcp: Arc::default(),
            mcp_tools: HashMap::new(),
            tool_results: std::collections::VecDeque::new(),
            next_result_id: 0,
            terminal_width: 100,
            terminal_height: 30,
        })
    }

    /// Load the configured home page in the current tab, if one is set
    pub fn open_home_page(&mut self) {
        if let Some(home) = self.settings.home_page.clone() {
            self.current_tab().url_input = home;
            self.submit_request();
        }
    }

    pub fn copy_to_clipboard(&mut self, text: String) -> Result<(), String> {
        let clipboard = self.clipboard.as_mut().ok_or("Clipboard unavailable")?;
        clipboard.set_text(text).map_err(|e| format!("Clipboard error: {}", e))
    }

    pub fn paste_from_clipboard(&mut self) -> Result<String, String> {
        let clipboard = self.clipboard.as_mut().ok_or("Clipboard unavailable")?;
        clipboard.get_text().map_err(|e| format!("Clipboard error: {}", e))
    }

    /// The user's Downloads folder. Minimal Linux systems without XDG user dirs don't define
    /// one; ~/Downloads is used there, as other browsers do.
    pub fn default_download_dir() -> PathBuf {
        let dirs = UserDirs::new();
        dirs.as_ref()
            .and_then(|d| d.download_dir().map(|p| p.to_path_buf()))
            .or_else(|| dirs.as_ref().map(|d| d.home_dir().join("Downloads")))
            .unwrap_or_default()
    }

    pub fn current_tab(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active_tab_index]
    }

    /// Lay out a tab's content for a terminal `width` columns wide
    pub fn render_tab(&mut self, tab_index: usize, width: u16) {
        self.terminal_width = width;
        let background = self.render_in_background;
        let tx = self.tx.clone();
        let Some(tab) = self.tabs.get_mut(tab_index) else {
            return;
        };
        let job = RenderJob {
            source: Arc::clone(&tab.html_source),
            kind: tab.page_kind,
            source_view: tab.is_source_view,
            reader: tab.reader_mode,
            image_sizes: Arc::clone(&tab.image_sizes),
            width,
        };
        if !background {
            tab.apply_render(render_page(&job));
            self.record_visit_if_pending(tab_index);
            self.load_images_if_pending(tab_index);
            return;
        }

        let version = tab.content_version;
        if tab.render_in_flight == Some(version) {
            // This content is already being laid out; render once more at the latest width when
            // that finishes, instead of piling up renders during a resize
            tab.render_pending_width = Some(width);
            return;
        }
        // New content starts right away; an older render still running is discarded on arrival
        tab.render_in_flight = Some(version);
        tab.render_pending_width = None;
        let tab_id = tab.id;
        tokio::task::spawn_blocking(move || {
            let page = render_page(&job);
            let _ = tx.blocking_send(NetworkResponse::Rendered(tab_id, version, Box::new(page)));
        });
    }

    /// A background render finished: show it if it's still current, then run any queued render
    pub fn finish_render(&mut self, tab_index: usize, version: u64, page: RenderedPage) {
        let Some(tab) = self.tabs.get_mut(tab_index) else {
            return;
        };
        if version == tab.content_version {
            tab.apply_render(page);
            if tab.status_message == RENDERING_STATUS {
                tab.status_message = String::from("Loaded");
                self.hint_cookies_for_sign_in(tab_index);
            }
            self.record_visit_if_pending(tab_index);
            self.load_images_if_pending(tab_index);
        }
        let tab = &mut self.tabs[tab_index];
        if tab.render_in_flight == Some(version) {
            tab.render_in_flight = None;
            if let Some(width) = tab.render_pending_width.take() {
                self.render_tab(tab_index, width);
            }
        }
    }

    /// Whether a background render is running for the tab
    pub fn is_rendering(&self, tab_index: usize) -> bool {
        self.tabs.get(tab_index).is_some_and(|t| t.render_in_flight.is_some())
    }

    pub fn resize_all_tabs(&mut self, width: u16) {
        for i in 0..self.tabs.len() {
            self.render_tab(i, width);
        }
    }

    /// The terminal changed size: lay every tab out again, with its images sized for the screen
    pub fn resize(&mut self, width: u16, height: u16) {
        self.terminal_width = width;
        self.terminal_height = height;
        for i in 0..self.tabs.len() {
            if self.tabs[i].image_sizes.is_empty() {
                self.render_tab(i, width);
            } else {
                self.update_image_sizes(i);
            }
        }
    }

    pub fn add_tab(&mut self, url: Option<String>) {
        let start_url = url.unwrap_or_else(|| HELP_PAGE.to_string());
        let new_tab = BrowserTab::new(self.id_counter, start_url);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.id_counter += 1;
        self.render_tab(self.active_tab_index, self.terminal_width);
        self.open_home_page();
    }

    pub fn close_tab(&mut self) {
        if self.tabs.len() > 1 {
            let closed = self.tabs.remove(self.active_tab_index);
            // Kept to reopen (X), the last few
            self.closed_tabs.push(closed);
            if self.closed_tabs.len() > CLOSED_TABS_KEPT {
                self.closed_tabs.remove(0);
            }
            if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
            }
        }
    }

    /// X: the tab closed last, back where it was shown, with its page and history
    pub fn reopen_tab(&mut self) {
        let Some(tab) = self.closed_tabs.pop() else {
            self.current_tab().status_message = String::from("No closed tab to reopen");
            return;
        };
        self.active_tab_index = (self.active_tab_index + 1).min(self.tabs.len());
        self.tabs.insert(self.active_tab_index, tab);
        self.render_tab(self.active_tab_index, self.terminal_width);
        self.current_tab().status_message = String::from("Reopened the tab closed last");
    }

    /// Y: copy the page's address (a password in it isn't copied as typed: the page's address is)
    pub fn copy_address(&mut self) {
        let tab = self.current_tab();
        let address = tab.page_url.clone().unwrap_or_else(|| tab.url_input.clone());
        let message = match self.copy_to_clipboard(address.clone()) {
            Ok(()) => format!("Copied {}", address),
            Err(e) => e,
        };
        self.current_tab().status_message = message;
    }

    pub fn open_link_in_new_tab(&mut self, url: String) {
        if url.starts_with("about:tools?") {
            let Some(address) = self.checked_tools_link(&url) else { return };
            self.tabs.push(BrowserTab::new(self.id_counter, address));
            self.active_tab_index = self.tabs.len() - 1;
            self.id_counter += 1;
            self.load_url_input(false);
            return;
        }
        if self.reject_unsupported_link(&url) {
            return;
        }
        let new_tab = BrowserTab::new(self.id_counter, url);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.id_counter += 1;
        self.submit_request();
    }

    /// Refuse links the browser can't open, explaining why in the status line. A magnet link is
    /// for a torrent client: it's copied, to paste into one.
    fn reject_unsupported_link(&mut self, url: &str) -> bool {
        if url.get(..7).is_some_and(|scheme| scheme.eq_ignore_ascii_case("magnet:")) {
            self.current_tab().status_message = match self.copy_to_clipboard(url.to_string()) {
                Ok(()) => format!("Copied the magnet link: paste it into a torrent client ({})", router::I2P_PAGE),
                Err(e) => format!("Magnet links open in a torrent client, and this one couldn't be copied: {}", e),
            };
            return true;
        }
        if let Some(scheme) = crate::network::unsupported_link_scheme(url) {
            self.current_tab().status_message = format!("Unsupported link type: {}", scheme);
            return true;
        }
        // https://paypal.com@evil.example/ goes to evil.example: a classic phishing disguise
        if let Some(host) = crate::network::hidden_destination(url) {
            self.current_tab().status_message =
                format!("Blocked: this link hides where it really goes ({}). Type the address if you trust it.", host);
            return true;
        }
        // A Gopher or Gemini link sends its address to the server as a raw line of text, which a
        // public page could aim at services on your computer or network that read text commands
        let from_public_page = self.tabs[self.active_tab_index]
            .page_url
            .as_deref()
            .is_some_and(|page| !crate::network::is_local_network_url(page));
        // In I2P mode this computer's addresses are reached directly, around I2P, so only typed
        // addresses, bookmarks and history open them: an I2P site's link could aim at the router's
        // console
        let from_site = self.tabs[self.active_tab_index]
            .page_url
            .as_deref()
            .is_some_and(|page| !crate::network::is_loopback_url(page));
        if self.i2p_mode && crate::network::is_loopback_url(url) && from_site {
            self.current_tab().status_message = String::from(
                "Blocked: in I2P mode, a site can't link to this computer. Type the address if you trust it.",
            );
            return true;
        }
        if crate::network::is_small_web(url) && crate::network::is_local_network_url(url) && from_public_page {
            self.current_tab().status_message = String::from(
                "Blocked: a public page can't link to Gopher or Gemini on your local network. Type the address if you trust it.",
            );
            return true;
        }
        false
    }

    /// Follow a link in the current tab, recording the current page in history
    pub fn follow_link(&mut self, url: String) {
        // A tools link is checked against the page offering it, then opens the tools page
        let url = if url.starts_with("about:tools?") {
            match self.checked_tools_link(&url) {
                Some(address) => address,
                None => return,
            }
        } else {
            url
        };
        let internal = url == BOOKMARKS_PAGE
            || url == HISTORY_PAGE
            || url == HELP_PAGE
            || url == SETTINGS_PAGE
            || url == crate::update::UPDATE_PAGE
            || url == crate::keys::KEYS_PAGE
            || url == crate::cookies::COOKIES_PAGE
            || url == router::I2P_PAGE
            || url.starts_with(crate::snapshots::SNAPSHOTS_PAGE)
            || url.starts_with(crate::snapshots::SNAPSHOT)
            || url.starts_with(crate::mcp::TOOLS_PAGE);
        if !internal && self.reject_unsupported_link(&url) {
            return;
        }
        let referer = self.address_helper_referer(&url);
        let tab = self.current_tab();
        if !tab.url_input.is_empty() {
            let entry = tab.history_entry(tab.url_input.clone());
            tab.history.push(entry);
        }
        tab.forward_history.clear();
        tab.pending_restore = None;
        tab.pending_referer = referer.map(|referer| (url.clone(), referer));
        tab.url_input = url;
        self.load_url_input(false);
    }

    /// The Referer that confirms an I2P address helper: i2pd adds a name from a link's
    /// i2paddresshelper only when the Referer is on that same site, which in practice means its own
    /// "Continue" link, so that sites can't add or change names in its address book behind your
    /// back. rynx sends no Referer otherwise; for that link it sends the site's address alone.
    fn address_helper_referer(&self, url: &str) -> Option<String> {
        if !self.i2p_mode {
            return None;
        }
        let target = Url::parse(url).ok()?;
        let page = Url::parse(self.tabs[self.active_tab_index].page_url.as_deref()?).ok()?;
        let helper = target.query_pairs().any(|(key, _)| key == "i2paddresshelper");
        (helper && target.origin() == page.origin()).then(|| format!("{}/", target.origin().ascii_serialization()))
    }

    /// Return to the previous page, where it was scrolled to
    pub fn go_back(&mut self) {
        let tab = self.current_tab();
        if let Some(entry) = tab.history.pop() {
            let current = tab.history_entry(tab.url_input.clone());
            tab.forward_history.push(current);
            self.revisit(entry);
        }
    }

    /// Return to the page left by going back
    pub fn go_forward(&mut self) {
        let tab = self.current_tab();
        if let Some(entry) = tab.forward_history.pop() {
            let current = tab.history_entry(tab.url_input.clone());
            tab.history.push(current);
            self.revisit(entry);
        }
    }

    fn revisit(&mut self, entry: HistoryEntry) {
        let tab = self.current_tab();
        tab.url_input = entry.url;
        tab.pending_cache_use = fetch::CacheUse::History;
        tab.pending_restore = Some((entry.scroll, entry.cursor_line));
        self.load_url_input(true);
    }

    /// Use form control `index`: start typing into a field, toggle a checkbox, pick a radio button,
    /// move to a choice's next option, or press a button
    pub fn activate_control(&mut self, index: usize) {
        let tab = self.current_tab();
        let Some(control) = tab.controls.get(index).cloned() else {
            return;
        };
        match &control.kind {
            ControlKind::Text { .. } | ControlKind::TextArea => {
                tab.input_mode = InputMode::FormEdit;
                tab.editing_control = Some(index);
                tab.status_message = if control.kind == ControlKind::TextArea {
                    String::from("Typing: Enter adds a line, Esc stops typing")
                } else {
                    String::from("Typing: Enter submits the form, Esc stops typing")
                };
            }
            ControlKind::Checkbox => {
                if let Some(ControlValue::Checked(on)) = tab.control_values.get_mut(index) {
                    *on = !*on;
                }
            }
            ControlKind::Radio => {
                // Picking one option clears the others in its group
                for (i, other) in tab.controls.iter().enumerate() {
                    let same_group = other.kind == ControlKind::Radio
                        && other.form == control.form
                        && other.name == control.name
                        && !control.name.is_empty();
                    if i == index || same_group {
                        tab.control_values[i] = ControlValue::Checked(i == index);
                    }
                }
            }
            ControlKind::Select { options } => {
                if let Some(ControlValue::Selected(selected)) = tab.control_values.get_mut(index) {
                    *selected = (*selected + 1) % options.len().max(1);
                }
            }
            ControlKind::Submit => match control.form {
                Some(form) => self.submit_form(form, Some(index)),
                None => tab.status_message = String::from("This button isn't part of a form"),
            },
            ControlKind::Hidden => {}
        }
    }

    /// Send form `form`, pressed with button `submitter` if any
    pub fn submit_form(&mut self, form: usize, submitter: Option<usize>) {
        let tab = self.current_tab();
        tab.stop_editing();
        let base = tab.link_base().to_string();
        // The settings page's form saves settings; like the tools page below, only the browser's
        // own page can do this, never a web page's form
        if tab.page_url.is_none() && tab.url_input == SETTINGS_PAGE {
            self.save_settings_form(form, submitter);
            return;
        }
        if tab.page_url.is_none() && tab.url_input == crate::update::UPDATE_PAGE {
            self.update_form(submitter);
            return;
        }
        if tab.page_url.is_none() && tab.url_input == crate::cookies::COOKIES_PAGE {
            self.cookies_form(submitter);
            return;
        }
        if tab.page_url.is_none() && tab.url_input == crate::snapshots::SNAPSHOTS_PAGE {
            self.delete_snapshots_form(form);
            return;
        }
        // Forms on the browser's tools page run tools; a web page's forms can never reach them
        if tab.page_url.is_none() && tab.url_input.starts_with(crate::mcp::TOOLS_PAGE) {
            let tool = tab
                .forms
                .get(form)
                .map(|f| crate::forms::form_target(f, &base))
                .and_then(|t| crate::mcp::tool_of_action(&t));
            if let Some(tool) = tool {
                self.prepare_tool_call(form, &tool);
            }
            return;
        }
        let submission =
            crate::forms::build_submission(&tab.forms, &tab.controls, &tab.control_values, form, submitter, &base);
        match submission {
            Err(message) => tab.status_message = message,
            Ok(submission) if submission.method == FormMethod::Get => self.follow_link(submission.url),
            Ok(submission) => {
                // A POST always goes to the server, even for the same address
                let entry = tab.history_entry(tab.url_input.clone());
                if !tab.url_input.is_empty() {
                    tab.history.push(entry);
                }
                tab.forward_history.clear();
                tab.pending_restore = None;
                tab.url_input = submission.url;
                tab.pending_post = submission.body;
                self.submit_request();
            }
        }
    }

    /// Fetch the current page again, keeping the scroll position
    pub fn reload(&mut self) {
        let tab = self.current_tab();
        let Some(page) = tab.page_url.clone() else {
            // The start page or an error page; nothing to fetch
            return;
        };
        tab.pending_restore = Some((tab.scroll, tab.cursor_line));
        // The page's own address: on a snapshot, the address bar holds the snapshot's
        tab.url_input = page;
        tab.pending_cache_use = fetch::CacheUse::Reload;
        self.submit_request();
    }

    /// Load the address in url_input, or jump within the displayed page when only the
    /// #fragment differs. A link to the same page without a fragment reloads it, but going
    /// back through history to it does not.
    pub fn load_url_input(&mut self, from_history: bool) {
        let url = self.current_tab().url_input.trim().to_string();
        if self.show_internal_page(&url) {
            return;
        }
        // The browser's own pages are all known; another about: address isn't searched for
        if url.to_ascii_lowercase().starts_with("about:") {
            self.current_tab().status_message = format!("There's no page {} in rynx", url);
            return;
        }
        let tab = self.current_tab();
        if tab.is_current_document(&url) {
            let fragment = fragment_of(&url);
            if from_history || fragment.is_some() {
                tab.page_url = Some(url);
                match (tab.pending_restore.take(), fragment) {
                    (Some(position), _) => tab.restore_position(position),
                    (None, Some(fragment)) => {
                        tab.jump_to_fragment(&fragment);
                    }
                    (None, None) => tab.scroll_to_top(),
                }
                return;
            }
        }
        self.submit_request();
    }

    pub fn submit_request(&mut self) {
        // Reticulum isn't I2P: locked to I2P, rynx doesn't open NomadNet addresses
        let address = self.tabs[self.active_tab_index].url_input.clone();
        if crate::reticulum::is_nomadnet(&address) && self.settings.network == crate::settings::NetworkMode::I2p {
            self.current_tab().status_message =
                String::from("Blocked: NomadNet pages travel over Reticulum, not I2P, and settings lock rynx to I2P");
            return;
        }
        let i2p_mode = self.i2p_mode;
        let solve_anubis = self.settings.solve_anubis;
        let url = fetch::address_for(&self.tabs[self.active_tab_index].url_input, &self.settings, i2p_mode);
        let tab = self.current_tab();
        tab.url_input = url.clone();
        // The server never sees the #fragment, so remember it to jump to after loading
        tab.pending_fragment = fragment_of(&url);
        let referer = tab.pending_referer.take().filter(|(for_url, _)| *for_url == url).map(|(_, referer)| referer);
        let request = fetch::PageRequest {
            tab_id: tab.id,
            url,
            form_body: tab.pending_post.take(),
            referer,
            i2p_mode,
            solve_anubis,
            cache_use: std::mem::take(&mut tab.pending_cache_use),
        };
        let shared = fetch::Shared {
            network: Arc::clone(&self.network_manager),
            gemini_hosts: Arc::clone(&self.gemini_hosts),
            cache: Arc::clone(&self.cache),
            snapshots: Arc::clone(&self.snapshots),
            tx: self.tx.clone(),
        };
        tokio::spawn(fetch::fetch(request, shared));
    }
}
