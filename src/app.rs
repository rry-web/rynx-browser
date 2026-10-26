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

/// Status shown while a new page is laid out in the background
pub const RENDERING_STATUS: &str = "Rendering...";

pub struct BrowserTab {
    pub id: usize,
    pub url_input: String,
    pub rendered_content: Vec<Line<'static>>,
    pub link_regions: Vec<LinkRegion>,
    pub page_title: String,
    pub scroll: usize,
    pub history: Vec<HistoryEntry>,
    /// Pages left by going back, for going forward again
    pub forward_history: Vec<HistoryEntry>,
    pub selected_link_index: usize,
    pub input_mode: InputMode,
    pub status_message: String,
    pub html_source: Arc<str>,
    pub is_source_view: bool,
    pub cursor_line: usize,
    pub cursor_char: usize,
    pub selection: Option<Selection>,
    pub download_state: Option<crate::models::Download>,
    pub search_state: Option<SearchState>,
    pub download_prompt: Option<crate::models::DownloadPrompt>,
    pub tool_prompt: Option<crate::models::ToolPrompt>,
    /// Address bar contents when Edit mode was entered, for history and cancelling
    pub url_before_edit: Option<String>,
    /// Line index of each in-page link target on the displayed page
    pub anchors: HashMap<String, usize>,
    /// URL of the displayed document after redirects; None for the help page and error pages
    pub page_url: Option<String>,
    /// #fragment to jump to once the page being loaded arrives
    pub pending_fragment: Option<String>,
    /// Whether html_source holds HTML or plain text
    pub page_kind: PageKind,
    /// Bumped whenever what's shown changes (new page, source view toggle), so a background
    /// render of older content is never applied
    pub content_version: u64,
    /// Version of the background render currently running, if any
    render_in_flight: Option<u64>,
    /// Width for one more render once the running one finishes (coalesces rapid resizes)
    render_pending_width: Option<u16>,
    /// The page just loaded goes into the history once laid out, when its <title> is known
    pub record_visit_after_render: bool,
    /// #fragment to jump to once the page has been laid out
    jump_after_render: Option<String>,
    /// Scroll and cursor position to return to when the page being loaded arrives (going back,
    /// forward or reloading)
    pending_restore: Option<(usize, usize)>,
    /// That position, once the page has arrived and is waiting to be laid out
    restore_after_render: Option<(usize, usize)>,
    /// The page's forms and controls, and what the user has entered into them
    pub forms: Vec<Form>,
    pub controls: Vec<FormControl>,
    pub control_values: Vec<ControlValue>,
    /// The field being typed into (in FormEdit mode)
    pub editing_control: Option<usize>,
    /// Form data to POST with the next request
    pending_post: Option<String>,
}

impl BrowserTab {
    pub fn enter_visual_mode(&mut self) {
        self.input_mode = InputMode::Visual;
        self.status_message = String::from("Copy mode: select text, then press y to copy it");
        // Anchor the selection to current cursor position
        self.selection = Some(Selection {
            start_line: self.cursor_line,
            start_char: self.cursor_char,
            end_line: self.cursor_line,
            end_char: self.cursor_char,
        });
    }
    /// Clear state that refers to positions in the previous page's content
    pub fn reset_page_state(&mut self) {
        self.scroll = 0;
        self.selected_link_index = 0;
        self.cursor_line = 0;
        self.cursor_char = 0;
        self.selection = None;
        self.search_state = None;
        // Leave Edit mode alone so a page finishing in the background doesn't eat typing
        if matches!(self.input_mode, InputMode::Visual | InputMode::Search | InputMode::FormEdit) {
            self.input_mode = InputMode::Normal;
        }
        self.editing_control = None;
        self.record_visit_after_render = false;
    }

    /// Scroll the minimum amount needed to bring `line` into a viewport of `viewport_height` rows
    pub fn scroll_to_line(&mut self, line: usize, viewport_height: usize) {
        let viewport_height = viewport_height.max(1);
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll + viewport_height {
            self.scroll = line + 1 - viewport_height;
        }
    }

    /// Scroll the view by `delta` lines without running past either end of the page.
    /// Like vim, the cursor is carried along so it stays on screen and j/k continue from there.
    pub fn scroll_by(&mut self, delta: isize, viewport_height: usize) {
        let viewport_height = viewport_height.max(1);
        let last_line = self.rendered_content.len().saturating_sub(1);
        let max_scroll = self.rendered_content.len().saturating_sub(viewport_height);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max_scroll);

        let bottom = (self.scroll + viewport_height - 1).min(last_line);
        let cursor_line = self.cursor_line.max(self.scroll).min(bottom);
        if cursor_line != self.cursor_line {
            self.cursor_line = cursor_line;
            if self.input_mode == InputMode::Visual
                && let Some(ref mut sel) = self.selection
            {
                sel.end_line = self.cursor_line;
                sel.end_char = self.cursor_char;
            }
        }
    }

    /// Base URL for links on the displayed page
    pub fn link_base(&self) -> &str {
        self.page_url.as_deref().unwrap_or(&self.url_input)
    }

    /// Whether `url` refers to the displayed document, ignoring any #fragment
    pub fn is_current_document(&self, url: &str) -> bool {
        let page = self.page_url.as_deref().and_then(|p| Url::parse(p).ok());
        match (page, Url::parse(url)) {
            (Some(mut page), Ok(mut target)) => {
                page.set_fragment(None);
                target.set_fragment(None);
                page == target
            }
            _ => false,
        }
    }

    /// Scroll to the element named by `fragment`; returns false if the page has no such anchor
    pub fn jump_to_fragment(&mut self, fragment: &str) -> bool {
        let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
        let line = match self.anchors.get(decoded.as_ref()).or_else(|| self.anchors.get(fragment)) {
            Some(&line) => line,
            // "#" and "#top" mean the top of the page unless something is named that
            None if decoded.is_empty() || decoded.eq_ignore_ascii_case("top") => 0,
            None => {
                self.status_message = format!("Anchor #{} not found on this page", decoded);
                return false;
            }
        };
        self.scroll = line;
        self.cursor_line = line.min(self.rendered_content.len().saturating_sub(1));
        self.cursor_char = 0;
        // Tab continues from the jump target rather than wherever the selection was
        if let Some(i) = self.link_regions.iter().position(|l| l.line_index >= line) {
            self.selected_link_index = i;
        }
        true
    }

    /// Replace what the tab shows. The old layout is cleared at once, so links from the previous
    /// page can't be followed while the new one is being laid out.
    pub fn set_content(&mut self, source: impl Into<Arc<str>>, kind: PageKind) {
        self.html_source = source.into();
        self.page_kind = kind;
        self.content_changed();
    }

    /// Mark the shown content as changed (e.g. toggling source view)
    pub fn content_changed(&mut self) {
        self.content_version += 1;
        self.rendered_content.clear();
        self.link_regions.clear();
        self.anchors.clear();
        self.forms.clear();
        self.controls.clear();
        self.control_values.clear();
        self.editing_control = None;
        if self.input_mode == InputMode::FormEdit {
            self.input_mode = InputMode::Normal;
        }
    }

    /// Change the text of the field being edited
    pub fn edit_field(&mut self, edit: impl FnOnce(&mut String)) {
        let Some(index) = self.editing_control else {
            return;
        };
        let max_chars = self.controls.get(index).map_or(0, |c| c.max_chars());
        if let Some(ControlValue::Text(text)) = self.control_values.get_mut(index) {
            edit(text);
            if text.chars().count() > max_chars {
                *text = text.chars().take(max_chars).collect();
            }
        }
    }

    /// Stop typing into a field, keeping what was typed
    pub fn stop_editing(&mut self) {
        self.editing_control = None;
        if self.input_mode == InputMode::FormEdit {
            self.input_mode = InputMode::Normal;
        }
    }

    /// The current page as a history entry, at `url`
    pub fn history_entry(&self, url: String) -> HistoryEntry {
        HistoryEntry { url, scroll: self.scroll, cursor_line: self.cursor_line }
    }

    /// Jump to `fragment` once the page being laid out is ready
    pub fn jump_after_render(&mut self, fragment: Option<String>) {
        self.jump_after_render = fragment;
    }

    /// A new page has arrived: if it was loaded by going back, forward or reloading, return to
    /// the saved position once it's laid out (instead of any #fragment)
    pub fn restore_position_after_render(&mut self) {
        self.restore_after_render = self.pending_restore.take();
    }

    /// Forget a saved position, e.g. when the load failed
    pub fn cancel_restore(&mut self) {
        self.pending_restore = None;
        self.restore_after_render = None;
    }

    fn restore_position(&mut self, (scroll, cursor_line): (usize, usize)) {
        let last_line = self.rendered_content.len().saturating_sub(1);
        self.scroll = scroll.min(last_line);
        self.cursor_line = cursor_line.min(last_line);
        self.cursor_char = 0;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
        self.cursor_line = 0;
        self.cursor_char = 0;
    }

    pub fn scroll_to_bottom(&mut self, viewport_height: usize) {
        self.scroll = self.rendered_content.len().saturating_sub(viewport_height.max(1));
        self.cursor_line = self.rendered_content.len().saturating_sub(1);
        self.cursor_char = 0;
    }

    fn apply_render(&mut self, page: RenderedPage) {
        self.rendered_content = page.lines;
        self.link_regions = page.links;
        self.anchors = page.anchors;
        // A re-layout of the same page (e.g. a resize) keeps what was typed
        if self.control_values.len() != page.controls.len() {
            self.control_values = page.controls.iter().map(FormControl::initial_value).collect();
            self.editing_control = None;
        }
        self.forms = page.forms;
        self.controls = page.controls;
        if let Some(title) = page.title {
            self.page_title = title;
        }
        self.clamp_positions();
        let fragment = self.jump_after_render.take();
        if let Some(position) = self.restore_after_render.take() {
            self.restore_position(position);
        } else if let Some(fragment) = fragment {
            self.jump_to_fragment(&fragment);
        }
    }

    /// Keep the selected link and cursor inside the current content after a re-render
    fn clamp_positions(&mut self) {
        if self.selected_link_index >= self.link_regions.len() {
            self.selected_link_index = 0;
        }
        self.cursor_line = self.cursor_line.min(self.rendered_content.len().saturating_sub(1));
    }

    pub fn extract_text_from_selection(&self) -> String {
        match &self.selection {
            Some(sel) => sel.extract_text(&self.rendered_content),
            None => String::new(),
        }
    }
    pub fn new(id: usize, initial_url: String) -> Self {
        let help_html = include_str!("../assets/help.html");
        let document = Html::parse_document(help_html);
        let mut renderer = DomRenderer::new(100);
        renderer.render(&document);

        Self {
            id,
            url_input: initial_url,
            rendered_content: renderer.lines,
            link_regions: renderer.links,
            page_title: String::from("New Tab"),
            scroll: 0,
            history: Vec::new(),
            forward_history: Vec::new(),
            selected_link_index: 0,
            input_mode: InputMode::Normal,
            status_message: String::from("Ready"),
            // Kept so the help page can be laid out again at other widths
            html_source: Arc::from(help_html),
            is_source_view: false,
            cursor_line: 0,
            cursor_char: 0,
            selection: None,
            download_state: None,
            search_state: None,
            download_prompt: None,
            tool_prompt: None,
            url_before_edit: None,
            anchors: renderer.anchors,
            page_url: None,
            pending_fragment: None,
            page_kind: PageKind::Html,
            content_version: 0,
            render_in_flight: None,
            render_pending_width: None,
            record_visit_after_render: false,
            jump_after_render: None,
            pending_restore: None,
            restore_after_render: None,
            forms: Vec::new(),
            controls: Vec::new(),
            control_values: Vec::new(),
            editing_control: None,
            pending_post: None,
        }
    }

    /// Find `query` on the page. Smart case: matching ignores case unless the query has a capital
    /// letter. Matches are stored as screen columns, which is how they're highlighted.
    pub fn perform_search(&mut self, query: &str) {
        if query.is_empty() {
            self.search_state = None;
            return;
        }

        let ignore_case = !query.chars().any(char::is_uppercase);
        let fold = |c: char| if ignore_case { c.to_lowercase().next().unwrap_or(c) } else { c };
        let query_chars: Vec<char> = query.chars().map(fold).collect();
        let mut matches = Vec::new();

        for (line_idx, line) in self.rendered_content.iter().enumerate() {
            let line_chars: Vec<char> = line.to_string().chars().collect();
            // Column where each character starts (wide characters take two columns)
            let mut columns = Vec::with_capacity(line_chars.len() + 1);
            let mut column = 0;
            for ch in &line_chars {
                columns.push(column);
                column += unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
            }
            columns.push(column);

            let mut char_idx = 0;
            while char_idx + query_chars.len() <= line_chars.len() {
                let found = query_chars.iter().enumerate().all(|(i, q)| fold(line_chars[char_idx + i]) == *q);
                if found {
                    matches.push(crate::models::SearchMatch {
                        line_index: line_idx,
                        start_char: columns[char_idx],
                        end_char: columns[char_idx + query_chars.len()],
                    });
                    // Move past this match to avoid overlapping matches
                    char_idx += query_chars.len();
                } else {
                    char_idx += 1;
                }
            }
        }

        // Keep the state even with no matches so the query survives and can be edited
        self.search_state = Some(SearchState { query: query.to_string(), matches, current_match_index: 0 });
    }

    pub fn next_search_match(&mut self) {
        if let Some(search_state) = &mut self.search_state
            && !search_state.matches.is_empty()
        {
            search_state.current_match_index = (search_state.current_match_index + 1) % search_state.matches.len();
        }
    }

    pub fn previous_search_match(&mut self) {
        if let Some(search_state) = &mut self.search_state
            && !search_state.matches.is_empty()
        {
            search_state.current_match_index = if search_state.current_match_index == 0 {
                search_state.matches.len() - 1
            } else {
                search_state.current_match_index - 1
            };
        }
    }

    pub fn clear_search(&mut self) {
        self.search_state = None;
        self.input_mode = InputMode::Normal;
        self.status_message = String::from("Ready");
    }

    pub fn initiate_download_request(&mut self, url: String, download_dir: &Path) {
        // Each tab tracks one download; a second would fight over the same overlay
        if let Some(crate::models::Download { status: crate::models::DownloadStatus::Active, .. }) = self.download_state
        {
            self.status_message = String::from("A download is already in progress in this tab");
            return;
        }
        if let Some(scheme) = crate::network::unsupported_link_scheme(&url) {
            self.status_message = format!("Unsupported link type: {}", scheme);
            return;
        }
        if crate::network::is_small_web(&url) {
            self.status_message = String::from("Downloading from Gemini and Gopher isn't supported yet");
            return;
        }

        let sanitized_name = App::download_filename(&url);
        let target_path = download_dir.join(&sanitized_name);
        let file_exists = App::path_taken(&target_path);

        self.download_prompt =
            Some(crate::models::DownloadPrompt { url, filename: sanitized_name, target_path, file_exists });
    }
}

pub struct App {
    pub tabs: Vec<BrowserTab>,
    pub active_tab_index: usize,
    pub id_counter: usize,
    pub tx: mpsc::Sender<NetworkResponse>,
    pub rx: mpsc::Receiver<NetworkResponse>,
    pub i2p_mode: bool,
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
    /// Certificates seen for Gemini hosts (trust on first use)
    pub gemini_hosts: Arc<std::sync::Mutex<crate::gemini::KnownHosts>>,
    /// MCP sessions with the servers whose tools have been used
    pub mcp: Arc<crate::mcp::Client>,
    /// The tools each server listed, for turning a form into a tool call
    pub mcp_tools: HashMap<String, Vec<crate::mcp::Tool>>,
    /// Recent tool results, shown at about:tool-result?id=<n>: (n, title, page)
    pub tool_results: std::collections::VecDeque<(u64, String, String)>,
    next_result_id: u64,
    /// `x` was pressed once on the history page; a second press clears it
    pub confirm_clear_history: bool,
    /// Latest terminal width, for laying out pages the browser makes itself
    pub terminal_width: u16,
}

/// The start page with the key bindings
pub const HELP_PAGE: &str = "about:help";

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
        let initial_tab = BrowserTab::new(INITIAL_TAB_ID, String::from("https://www.rust-lang.org"));
        let network_manager = Arc::new(NetworkManager::with_settings(&settings.i2p_proxy, &settings.user_agent)?);
        let download_dir = settings.download_dir.clone().unwrap_or_else(Self::default_download_dir);
        Ok(Self {
            tabs: vec![initial_tab],
            active_tab_index: DEFAULT_TAB_INDEX,
            id_counter: INITIAL_ID_COUNTER,
            tx,
            rx,
            i2p_mode: false,
            clipboard: arboard::Clipboard::new().ok(),
            network_manager,
            download_dir,
            render_in_background: true,
            settings,
            bookmarks: Bookmarks::default(),
            history: History::default(),
            gemini_hosts: Arc::default(),
            confirm_clear_history: false,
            mcp: Arc::default(),
            mcp_tools: HashMap::new(),
            tool_results: std::collections::VecDeque::new(),
            next_result_id: 0,
            terminal_width: 100,
        })
    }

    /// Load the configured home page in the current tab, if one is set
    pub fn open_home_page(&mut self) {
        if let Some(home) = self.settings.home_page.clone() {
            self.current_tab().url_input = home;
            self.submit_request();
        }
    }

    /// Show a page the browser makes itself (about:bookmarks, about:history, about:help)
    fn show_internal_page(&mut self, address: &str) -> bool {
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
            HELP_PAGE | "about:home" => ("Start", include_str!("../assets/help.html").to_string()),
            _ => return false,
        };
        self.show_generated_page(self.active_tab_index, address, title, html);
        true
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

    /// The address of a tools link followed from the current page, after checking the page may
    /// offer that server's tools. None (with the reason in the status line) if it may not.
    fn checked_tools_link(&mut self, url: &str) -> Option<String> {
        let server = crate::mcp::server_of(url)?;
        let page = self.tabs[self.active_tab_index].page_url.clone();
        match crate::mcp::check_server(&server, page.as_deref(), self.i2p_mode) {
            Ok(server) => Some(crate::mcp::tools_address(&server)),
            Err(e) => {
                self.current_tab().status_message = e;
                None
            }
        }
    }

    /// Show the tools page for `server`, listing its tools in the background
    fn open_tools(&mut self, server: &str) {
        let index = self.active_tab_index;
        let server = match crate::mcp::check_server(server, None, self.i2p_mode) {
            Ok(server) => server,
            Err(e) => {
                let address = crate::mcp::tools_address(server);
                self.show_generated_page(index, &address, "Tools unavailable", crate::mcp::error_page_html(server, &e));
                return;
            }
        };
        let address = crate::mcp::tools_address(&server);
        let tab = self.current_tab();
        tab.url_input = address.clone();
        tab.page_title = String::from("Loading...");
        tab.status_message = String::from("Getting the site's tools...");
        let id = tab.id;
        let tx = self.tx.clone();
        let http = self.network_manager.get_client(self.i2p_mode).clone();
        let mcp = Arc::clone(&self.mcp);
        tokio::spawn(async move {
            let result = mcp.list_tools(&http, &server).await;
            let _ = tx.send(NetworkResponse::ToolsListed(id, address, server, result)).await;
        });
    }

    /// Show the tools a server listed, if the tab is still waiting for them
    pub fn finish_listing_tools(
        &mut self,
        index: usize,
        address: String,
        server: String,
        result: Result<Vec<crate::mcp::Tool>, String>,
    ) {
        if self.tabs[index].url_input != address {
            return;
        }
        match result {
            Ok(tools) => {
                let html = crate::mcp::tools_page_html(&server, &tools);
                let host = Url::parse(&server).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
                self.mcp_tools.insert(server, tools);
                self.show_generated_page(index, &address, &format!("Tools from {}", host), html);
            }
            Err(e) => {
                self.show_generated_page(
                    index,
                    &address,
                    "Tools unavailable",
                    crate::mcp::error_page_html(&server, &e),
                );
                self.tabs[index].status_message = e;
            }
        }
    }

    /// Turn a form on the tools page into a tool call waiting for confirmation
    fn prepare_tool_call(&mut self, form: usize, tool_name: &str) {
        let tab = &self.tabs[self.active_tab_index];
        let Some(server) = crate::mcp::server_of(&tab.url_input) else {
            return;
        };
        let Some(tool) =
            self.mcp_tools.get(&server).and_then(|tools| tools.iter().find(|t| t.name == tool_name)).cloned()
        else {
            self.current_tab().status_message = String::from("That tool is no longer listed; reload the page");
            return;
        };
        let mut entered = HashMap::new();
        for (control, value) in tab.controls.iter().zip(&tab.control_values) {
            if control.form != Some(form) || control.name.is_empty() {
                continue;
            }
            let field = match (&control.kind, value) {
                (ControlKind::Checkbox, ControlValue::Checked(on)) => crate::mcp::FieldValue::Checked(*on),
                (_, ControlValue::Text(text)) => crate::mcp::FieldValue::Text(text.clone()),
                (ControlKind::Select { options }, ControlValue::Selected(i)) => {
                    crate::mcp::FieldValue::Text(options.get(*i).map(|o| o.value.clone()).unwrap_or_default())
                }
                _ => continue,
            };
            entered.insert(control.name.clone(), field);
        }
        let tab = self.current_tab();
        match crate::mcp::arguments(&tool, &entered) {
            Ok(arguments) => {
                tab.tool_prompt = Some(crate::models::ToolPrompt {
                    server,
                    warning: tool.warning(),
                    tool: tool.name,
                    title: tool.title,
                    arguments,
                });
                tab.status_message = String::from("Check what will be sent: y runs the tool, n cancels");
            }
            Err(e) => tab.status_message = e,
        }
    }

    /// Run the confirmed tool call
    pub fn run_tool(&mut self) {
        let tab = self.current_tab();
        let Some(prompt) = tab.tool_prompt.take() else {
            return;
        };
        tab.status_message = format!("Running {}...", prompt.title);
        let id = tab.id;
        let tx = self.tx.clone();
        let http = self.network_manager.get_client(self.i2p_mode).clone();
        let mcp = Arc::clone(&self.mcp);
        tokio::spawn(async move {
            let result = mcp.call_tool(&http, &prompt.server, &prompt.tool, prompt.arguments).await;
            let _ = tx.send(NetworkResponse::ToolFinished(id, prompt.server, prompt.title, result)).await;
        });
    }

    /// Keep a tool's result, and show it if the tab is still on that server's tools page
    pub fn finish_tool(
        &mut self,
        index: usize,
        server: String,
        title: String,
        result: Result<serde_json::Value, String>,
    ) {
        self.next_result_id += 1;
        let id = self.next_result_id;
        let html = crate::mcp::result_page_html(&server, &title, &result);
        self.tool_results.push_back((id, title.clone(), html.clone()));
        if self.tool_results.len() > 20 {
            self.tool_results.pop_front();
        }
        let address = format!("{}{}", crate::mcp::RESULT_PAGE, id);
        let tab = &mut self.tabs[index];
        if tab.url_input != crate::mcp::tools_address(&server) {
            tab.status_message = format!("{} finished; type {} to see the result", title, address);
            return;
        }
        let entry = tab.history_entry(tab.url_input.clone());
        tab.history.push(entry);
        tab.forward_history.clear();
        self.show_generated_page(index, &address, &title, html);
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
    fn record_visit_if_pending(&mut self, tab_index: usize) {
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

    /// On the history page, `x` asks for confirmation and a second `x` clears the history
    pub fn clear_history_key(&mut self) {
        if self.current_tab().url_input != HISTORY_PAGE {
            return;
        }
        if !self.confirm_clear_history {
            self.confirm_clear_history = true;
            self.current_tab().status_message = String::from("Press x again to clear all history");
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
            width,
        };
        if !background {
            tab.apply_render(render_page(&job));
            self.record_visit_if_pending(tab_index);
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
            }
            self.record_visit_if_pending(tab_index);
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

    pub fn add_tab(&mut self, url: Option<String>) {
        let start_url = url.unwrap_or_else(|| String::from("https://www.rust-lang.org"));
        let new_tab = BrowserTab::new(self.id_counter, start_url);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.id_counter += 1;
        self.render_tab(self.active_tab_index, self.terminal_width);
        self.open_home_page();
    }

    pub fn close_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.tabs.remove(self.active_tab_index);
            if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
            }
        }
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

    /// Refuse links the browser can't open, explaining why in the status line
    fn reject_unsupported_link(&mut self, url: &str) -> bool {
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
        let internal =
            url == BOOKMARKS_PAGE || url == HISTORY_PAGE || url == HELP_PAGE || url.starts_with(crate::mcp::TOOLS_PAGE);
        if !internal && self.reject_unsupported_link(&url) {
            return;
        }
        let tab = self.current_tab();
        if !tab.url_input.is_empty() {
            let entry = tab.history_entry(tab.url_input.clone());
            tab.history.push(entry);
        }
        tab.forward_history.clear();
        tab.pending_restore = None;
        tab.url_input = url;
        self.load_url_input(false);
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
        if tab.page_url.is_none() {
            // The start page or an error page; nothing to fetch
            return;
        }
        tab.pending_restore = Some((tab.scroll, tab.cursor_line));
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
        let use_i2p = self.i2p_mode;
        let search = self.settings.clone();
        let tab = self.current_tab();
        let mut target_url = tab.url_input.trim().to_string();

        // URL Normalization
        if !target_url.starts_with("http://")
            && !target_url.starts_with("https://")
            && !crate::network::is_small_web(&target_url)
        {
            if target_url.contains('.') && !target_url.contains(' ') {
                target_url = if target_url.ends_with(".i2p") {
                    format!("http://{}", target_url)
                } else {
                    format!("https://{}", target_url)
                };
            } else {
                target_url = search.search_address(&target_url);
            }
        }

        // Enforce HTTPS for clearweb requests (security hardening)
        // Allow HTTP for local addresses (localhost, 127.0.0.1, etc.)
        let target_url = crate::network::enforce_https(&target_url, use_i2p);

        tab.url_input = target_url.clone();
        // The server never sees the #fragment, so remember it to jump to after loading
        tab.pending_fragment = fragment_of(&target_url);
        let form_body = tab.pending_post.take();
        let id = tab.id;
        let tx_clone = self.tx.clone();
        let use_i2p = self.i2p_mode;
        let solve_anubis = self.settings.solve_anubis;
        let network_manager = Arc::clone(&self.network_manager);

        let domain_for_jump =
            Url::parse(&target_url).ok().and_then(|u| u.domain().map(|s| s.to_string())).unwrap_or_default();

        if crate::network::is_small_web(&target_url) {
            let gemini_hosts = Arc::clone(&self.gemini_hosts);
            tokio::spawn(async move {
                let _ = tx_clone.send(NetworkResponse::Loading(id)).await;
                let result = if use_i2p {
                    // These connections don't go through the I2P proxy, so they'd reveal the visit
                    Err("Gemini and Gopher aren't available in I2P mode (press p to leave it)".to_string())
                } else if target_url.starts_with("gemini://") {
                    crate::gemini::load(&target_url, &gemini_hosts, MAX_PAGE_SIZE_BYTES).await
                } else {
                    crate::gopher::load(&target_url, MAX_PAGE_SIZE_BYTES).await
                };
                let response = match result {
                    Ok(page) => NetworkResponse::Success(id, page.title, page.body, page.url, page.kind),
                    Err(e) => NetworkResponse::Error(id, e),
                };
                let _ = tx_clone.send(response).await;
            });
            return;
        }

        tokio::spawn(async move {
            let _ = tx_clone.send(NetworkResponse::Loading(id)).await;

            let client = network_manager.get_client(use_i2p);
            let mut resp_result =
                crate::network::send_following_redirects(client, &target_url, use_i2p, form_body).await;

            if let Ok(ref resp) = resp_result
                && should_attempt_jump(use_i2p, &domain_for_jump, resp.status())
                && let Ok(jump_resp) = attempt_jump(client, &domain_for_jump, tx_clone.clone(), id).await
            {
                resp_result = Ok(jump_resp);
            }

            // An Anubis bot check is passed at most once per page, so a site that keeps asking
            // can't make rynx work forever
            let mut anubis_tried = false;
            loop {
                match resp_result {
                    Ok(resp) => {
                        // Where redirects actually led; relative links resolve against this
                        let final_url = resp.url().to_string();
                        let status = resp.status();
                        let content_type = resp
                            .headers()
                            .get(reqwest::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);
                        let class = classify_content_type(content_type.as_deref());

                        // Images, PDFs and the like aren't read into memory as a page; offer a download
                        if let ContentClass::Binary(mime) = &class {
                            let (title, html) =
                                crate::network::binary_notice_page(&final_url, mime, resp.content_length());
                            let _ = tx_clone
                                .send(NetworkResponse::Success(id, title, html, final_url, PageKind::Html))
                                .await;
                            return;
                        }

                        // Fast rejection when the server declares the size; read_text_limited
                        // enforces the limit when it doesn't
                        if let Some(len) = resp.content_length()
                            && len > MAX_PAGE_SIZE_BYTES
                        {
                            let _ = tx_clone.send(NetworkResponse::Error(id, "Page too large".to_string())).await;
                            return;
                        }

                        let body = match crate::network::read_text_limited(resp, MAX_PAGE_SIZE_BYTES).await {
                            Ok(body) => body,
                            Err(e) => {
                                let _ = tx_clone.send(NetworkResponse::Error(id, e)).await;
                                return;
                            }
                        };

                        // An Anubis "making sure you're not a bot" page: do its proof of work natively
                        // and load what it was guarding. If that fails, the check's page is shown.
                        let mut anubis_problem = None;
                        if anubis_tried && (status.is_client_error() || status.is_server_error()) {
                            anubis_problem = Some(String::from("the site didn't accept rynx's answer"));
                        }
                        if class == ContentClass::Html
                            && anubis_problem.is_none()
                            && let Some(challenge) = crate::anubis::detect(&body)
                        {
                            if anubis_tried {
                                anubis_problem = Some(String::from("the site didn't accept rynx's answer"));
                            } else if !solve_anubis {
                                anubis_problem = Some(String::from("passing them is turned off in settings.conf"));
                            } else {
                                anubis_tried = true;
                                let host = Url::parse(&final_url)
                                    .ok()
                                    .and_then(|u| u.host_str().map(str::to_string))
                                    .unwrap_or_default();
                                let _ = tx_clone
                                    .send(NetworkResponse::Info(
                                        id,
                                        format!("Passing {}'s bot check (Anubis), without JavaScript...", host),
                                    ))
                                    .await;
                                match crate::anubis::pass(client, &final_url, challenge, use_i2p).await {
                                    Ok(next) => {
                                        resp_result = Ok(next);
                                        continue;
                                    }
                                    Err(e) => anubis_problem = Some(e),
                                }
                            }
                        }

                        if crate::mcp::is_endpoint_answer(content_type.as_deref(), &body) {
                            let html = crate::mcp::endpoint_page_html(&final_url);
                            let title = String::from("MCP server");
                            let _ = tx_clone
                                .send(NetworkResponse::Success(id, title, html, final_url, PageKind::Html))
                                .await;
                        } else if crate::feeds::is_feed(content_type.as_deref(), &body) {
                            let (title, html) = crate::feeds::feed_to_html(&body);
                            let _ = tx_clone
                                .send(NetworkResponse::Success(id, title, html, final_url, PageKind::Html))
                                .await;
                        } else {
                            // The page isn't parsed here; an HTML page's <title> replaces this
                            // name once the page has been laid out
                            let title = crate::network::title_from_url(&final_url);
                            let kind = match class {
                                ContentClass::Html => PageKind::Html,
                                _ => PageKind::PlainText,
                            };
                            let _ = tx_clone.send(NetworkResponse::Success(id, title, body, final_url, kind)).await;
                        }
                        if let Some(problem) = anubis_problem {
                            let _ = tx_clone
                                .send(NetworkResponse::Info(
                                    id,
                                    format!("This is a bot check (Anubis), and {}", problem),
                                ))
                                .await;
                        }
                    }
                    Err(e) => {
                        let _ = tx_clone.send(NetworkResponse::Error(id, e)).await;
                    }
                }
                return;
            }
        });
    }

    /// Derive a safe local filename from the last path segment of a URL, ignoring query and fragment
    pub fn download_filename(url: &str) -> String {
        let raw_name = Url::parse(url)
            .ok()
            .and_then(|u| {
                u.path_segments().and_then(|mut segments| segments.rfind(|s| !s.is_empty()).map(str::to_string))
            })
            .unwrap_or_default();
        let decoded = percent_encoding::percent_decode_str(&raw_name).decode_utf8_lossy();
        Self::sanitize_filename(&decoded)
    }

    /// Sanitize filename to prevent path traversal attacks
    fn sanitize_filename(filename: &str) -> String {
        // Get just the filename part, stripping any path components
        let path = Path::new(filename);
        let filename_only = path.file_name().and_then(|n| n.to_str()).unwrap_or("download.dat");

        // Remove any dangerous characters and ensure it's safe
        let safe_name = filename_only
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
            .collect::<String>();

        // No hidden files (".bashrc"), and no trailing dots, which Windows silently drops
        let safe_name = safe_name.trim_start_matches('.').trim_end_matches('.');
        if safe_name.is_empty() {
            return "download.dat".to_string();
        }
        let safe_name = Self::limit_filename_length(safe_name);

        // On Windows, CON, NUL, COM1... (even with an extension) name devices, not files
        let stem = safe_name.split('.').next().unwrap_or("").to_ascii_uppercase();
        // COM and LPT take a digit, including the superscripts ¹²³ that Windows also accepts
        let is_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.chars().count() == 4
                && stem.chars().nth(3).is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)));
        if is_device { format!("_{}", safe_name) } else { safe_name }
    }

    /// Keep names well under the 255-byte limit of common filesystems, preserving the extension
    fn limit_filename_length(name: &str) -> String {
        const MAX_BYTES: usize = 200;
        if name.len() <= MAX_BYTES {
            return name.to_string();
        }
        let (stem, ext) = match name.rfind('.') {
            Some(dot) if name.len() - dot <= 20 => name.split_at(dot),
            _ => (name, ""),
        };
        let mut cut = MAX_BYTES - ext.len();
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}{}", &stem[..cut], ext)
    }

    /// Whether anything is at `path`, including a symbolic link (even one pointing nowhere)
    fn path_taken(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok()
    }

    /// The download URL as recorded on the saved file: no credentials or #fragment
    fn origin_url(url: &str) -> String {
        match Url::parse(url) {
            Ok(mut parsed) => {
                let _ = parsed.set_username("");
                let _ = parsed.set_password(None);
                parsed.set_fragment(None);
                parsed.to_string()
            }
            Err(_) => url.to_string(),
        }
    }

    /// Tag a download as coming from the internet, as other browsers do, so the OS treats it with
    /// suspicion. macOS: the quarantine attribute, so Gatekeeper checks it before it's first
    /// opened (format: flags;hex timestamp;agent;event id).
    /// The origin is also recorded where Finder's "Where from" and the Gatekeeper dialog read it.
    #[cfg(target_os = "macos")]
    fn mark_downloaded(path: &Path, url: &str) -> std::io::Result<()> {
        let timestamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let value = format!("0081;{:08x};rynx;", timestamp);
        xattr::set(path, "com.apple.quarantine", value.as_bytes())?;
        let where_from = Self::binary_plist_string_array(&Self::origin_url(url));
        xattr::set(path, "com.apple.metadata:kMDItemWhereFroms", &where_from)
    }

    /// A binary property list holding a one-element array of an ASCII string (URLs serialise as
    /// ASCII), the format of kMDItemWhereFroms
    #[cfg(target_os = "macos")]
    fn binary_plist_string_array(text: &str) -> Vec<u8> {
        let mut plist = b"bplist00".to_vec();
        let array_offset = plist.len();
        plist.extend([0xA1, 0x01]); // array of 1 element: object 1
        let string_offset = plist.len();
        let bytes = text.as_bytes();
        if bytes.len() < 15 {
            plist.push(0x50 | bytes.len() as u8);
        } else {
            // Length follows as an int object: 0x13 marks an 8-byte big-endian integer
            plist.push(0x5F);
            plist.push(0x13);
            plist.extend((bytes.len() as u64).to_be_bytes());
        }
        plist.extend(bytes);
        let offset_table = plist.len();
        for offset in [array_offset, string_offset] {
            plist.extend((offset as u64).to_be_bytes());
        }
        // Trailer: 6 unused bytes, offset size, object ref size, object count, top object,
        // offset table position
        plist.extend([0u8; 6]);
        plist.extend([8, 1]);
        plist.extend(2u64.to_be_bytes());
        plist.extend(0u64.to_be_bytes());
        plist.extend((offset_table as u64).to_be_bytes());
        plist
    }

    /// Windows: the Mark of the Web, a Zone.Identifier stream saying the file came from the
    /// Internet zone, which SmartScreen and Office Protected View check
    #[cfg(windows)]
    fn mark_downloaded(path: &Path, url: &str) -> std::io::Result<()> {
        let stream = format!("{}:Zone.Identifier", path.display());
        let content = format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={}\r\n", Self::origin_url(url));
        std::fs::write(stream, content)
    }

    /// Linux and other Unix: the freedesktop origin attribute, shown by file managers
    #[cfg(all(unix, not(target_os = "macos")))]
    fn mark_downloaded(path: &Path, url: &str) -> std::io::Result<()> {
        xattr::set(path, "user.xdg.origin.url", Self::origin_url(url).as_bytes())
    }

    #[cfg(not(any(unix, windows)))]
    fn mark_downloaded(_path: &Path, _url: &str) -> std::io::Result<()> {
        Ok(())
    }

    /// Return `path`, or `name_1.ext`, `name_2.ext`, ... if it already exists
    fn unused_path(path: &Path) -> PathBuf {
        if !Self::path_taken(path) {
            return path.to_path_buf();
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
        let ext = path.extension().and_then(|e| e.to_str());
        (1..)
            .map(|n| {
                let name = match ext {
                    Some(ext) => format!("{}_{}.{}", stem, n, ext),
                    None => format!("{}_{}", stem, n),
                };
                path.with_file_name(name)
            })
            .find(|candidate| !Self::path_taken(candidate))
            .expect("unbounded range always finds a free name")
    }

    /// Start downloading a confirmed prompt into exactly the path that was shown to the user
    pub fn trigger_download(&mut self, prompt: crate::models::DownloadPrompt) {
        let tab = self.current_tab();
        let tab_id = tab.id;
        tab.download_state = Some(crate::models::Download {
            filename: prompt.filename,
            bytes_downloaded: 0,
            total_size: None,
            status: crate::models::DownloadStatus::Active,
        });
        tab.status_message = String::from("Download starting...");

        let url = prompt.url;
        let target_path = prompt.target_path;
        let tx = self.tx.clone();
        let use_i2p = self.i2p_mode;
        let nm = Arc::clone(&self.network_manager);

        tokio::spawn(async move {
            let client = nm.get_download_client(use_i2p);

            // 1. Send request
            let res = match crate::network::get_following_redirects(client, &url, use_i2p).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Connection failed: {}", e))).await;
                    return;
                }
            };

            // Don't save a server's error page as if it were the file
            if !res.status().is_success() {
                let _ =
                    tx.send(NetworkResponse::DownloadError(tab_id, format!("Server returned {}", res.status()))).await;
                return;
            }

            let total = res.content_length();
            let mut stream = res.bytes_stream();
            let mut downloaded: u64 = 0;
            let mut file_handle: Option<(tokio::fs::File, String)> = None;
            // Throttle progress updates so the UI channel never limits download speed
            let progress_interval = Duration::from_millis(DOWNLOAD_PROGRESS_INTERVAL_MS);
            let mut last_progress: Option<Instant> = None;

            while let Some(item) = stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(e) => {
                        let _ = tx
                            .send(NetworkResponse::DownloadError(tab_id, format!("Download interrupted: {}", e)))
                            .await;
                        return;
                    }
                };
                if file_handle.is_none() {
                    // 2. Create Downloads Directory
                    if let Some(dir) = target_path.parent()
                        && let Err(e) = tokio::fs::create_dir_all(dir).await
                    {
                        let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Folder error: {}", e))).await;
                        return;
                    }

                    // 3. Sniff Extension; only the confirmed path may be overwritten
                    let mut file_path = target_path.clone();
                    if file_path.extension().is_none()
                        && let Some(ext) = crate::network::sniff_extension(&chunk)
                    {
                        file_path = Self::unused_path(&file_path.with_extension(ext));
                    }

                    // 4. Safe File Creation (No unwraps). Never write through a symbolic link: one
                    // planted in Downloads could point the write at e.g. ~/.bashrc
                    let is_symlink =
                        tokio::fs::symlink_metadata(&file_path).await.is_ok_and(|meta| meta.file_type().is_symlink());
                    if is_symlink {
                        let _ = tx
                            .send(NetworkResponse::DownloadError(
                                tab_id,
                                "Refusing to save over a symbolic link".to_string(),
                            ))
                            .await;
                        return;
                    }
                    let file = match tokio::fs::File::create(&file_path).await {
                        Ok(f) => f,
                        Err(e) => {
                            let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("File error: {}", e))).await;
                            return;
                        }
                    };
                    // Tag before writing, so even a partial download is checked on first open.
                    // Best effort: some filesystems (e.g. FAT USB drives) can't store the tag.
                    let _ = Self::mark_downloaded(&file_path, &url);
                    file_handle = Some((file, file_path.display().to_string()));
                }

                if let Some((ref mut file, _)) = file_handle {
                    if let Err(e) = file.write_all(&chunk).await {
                        let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Write failed: {}", e))).await;
                        return;
                    }
                    downloaded += chunk.len() as u64;
                    if last_progress.is_none_or(|t| t.elapsed() >= progress_interval) {
                        last_progress = Some(Instant::now());
                        let _ = tx.send(NetworkResponse::DownloadProgress(tab_id, downloaded, total)).await;
                    }
                }
            }

            if let Some((mut file, path)) = file_handle {
                // tokio completes writes in the background; flush so the file is whole before reporting done
                if let Err(e) = file.flush().await {
                    let _ = tx.send(NetworkResponse::DownloadError(tab_id, format!("Write failed: {}", e))).await;
                    return;
                }
                let _ = tx.send(NetworkResponse::DownloadFinished(tab_id, path)).await;
            } else {
                let _ = tx.send(NetworkResponse::DownloadError(tab_id, "Download stream was empty".to_string())).await;
            }
        });
    }
}
