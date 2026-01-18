use crate::constants::{
    DEFAULT_TAB_INDEX, INITIAL_ID_COUNTER, INITIAL_TAB_ID, MARGINALIA_SEARCH_URL,
    MAX_PAGE_SIZE_BYTES,
};
use crate::models::{InputMode, LinkRegion, SearchState, Selection};
use crate::network::{NetworkManager, NetworkResponse, attempt_jump, parse_html_metadata};
use crate::renderer::DomRenderer;

use ratatui::text::Line;
use reqwest::StatusCode;
use scraper::Html;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc;
use url::Url;

use directories::UserDirs;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt; // Required for streaming to file

pub struct BrowserTab {
    pub id: usize,
    pub url_input: String,
    pub rendered_content: Vec<Line<'static>>,
    pub link_regions: Vec<LinkRegion>,
    pub page_title: String,
    pub scroll: usize,
    pub history: Vec<String>,
    pub selected_link_index: usize,
    pub input_mode: InputMode,
    pub status_message: String,
    pub html_source: String,
    pub is_source_view: bool,
    pub cursor_line: usize,
    pub cursor_char: usize,
    pub selection: Option<Selection>,
    pub download_state: Option<crate::models::Download>,
    pub search_state: Option<SearchState>,
}

impl BrowserTab {
    pub fn enter_visual_mode(&mut self) {
        self.input_mode = InputMode::Visual;
        self.status_message = String::from("VISUAL MODE - Move cursor to select, 'y' to copy");
        // Anchor the selection to current cursor position
        self.selection = Some(Selection {
            start_line: self.cursor_line,
            start_char: self.cursor_char,
            end_line: self.cursor_line,
            end_char: self.cursor_char,
        });
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
            selected_link_index: 0,
            input_mode: InputMode::Normal,
            status_message: String::from("Ready"),
            html_source: String::new(),
            is_source_view: false,
            cursor_line: 0,
            cursor_char: 0,
            selection: None,
            download_state: None,
            search_state: None,
        }
    }

    pub fn perform_search(&mut self, query: &str) {
        if query.is_empty() {
            self.search_state = None;
            return;
        }

        let mut matches = Vec::new();
        let query_chars: Vec<char> = query.chars().collect();

        for (line_idx, line) in self.rendered_content.iter().enumerate() {
            let line_str = line.to_string();
            let line_chars: Vec<char> = line_str.chars().collect();

            let mut char_idx = 0;
            while char_idx <= line_chars.len().saturating_sub(query_chars.len()) {
                let mut found = true;
                for (i, &query_char) in query_chars.iter().enumerate() {
                    if char_idx + i >= line_chars.len() || line_chars[char_idx + i] != query_char {
                        found = false;
                        break;
                    }
                }

                if found {
                    matches.push(crate::models::SearchMatch {
                        line_index: line_idx,
                        start_char: char_idx,
                        end_char: char_idx + query_chars.len(),
                    });
                    // Move past this match to avoid overlapping matches
                    char_idx += query_chars.len();
                } else {
                    char_idx += 1;
                }
            }
        }

        self.search_state = if matches.is_empty() {
            None
        } else {
            Some(SearchState {
                query: query.to_string(),
                matches,
                current_match_index: 0,
            })
        };
    }

    pub fn next_search_match(&mut self) {
        if let Some(search_state) = &mut self.search_state {
            if !search_state.matches.is_empty() {
                search_state.current_match_index =
                    (search_state.current_match_index + 1) % search_state.matches.len();
            }
        }
    }

    pub fn previous_search_match(&mut self) {
        if let Some(search_state) = &mut self.search_state {
            if !search_state.matches.is_empty() {
                search_state.current_match_index = if search_state.current_match_index == 0 {
                    search_state.matches.len() - 1
                } else {
                    search_state.current_match_index - 1
                };
            }
        }
    }

    pub fn clear_search(&mut self) {
        self.search_state = None;
        self.input_mode = InputMode::Normal;
        self.status_message = String::from("Ready");
    }
}

pub struct App {
    pub tabs: Vec<BrowserTab>,
    pub active_tab_index: usize,
    pub id_counter: usize,
    pub tx: mpsc::Sender<NetworkResponse>,
    pub rx: mpsc::Receiver<NetworkResponse>,
    pub i2p_mode: bool,
    pub clipboard: arboard::Clipboard,
    pub network_manager: Arc<NetworkManager>,
}

impl App {
    pub fn new(
        tx: mpsc::Sender<NetworkResponse>,
        rx: mpsc::Receiver<NetworkResponse>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let initial_tab =
            BrowserTab::new(INITIAL_TAB_ID, String::from("https://www.rust-lang.org"));
        let network_manager = Arc::new(NetworkManager::new()?);
        Ok(Self {
            tabs: vec![initial_tab],
            active_tab_index: DEFAULT_TAB_INDEX,
            id_counter: INITIAL_ID_COUNTER,
            tx,
            rx,
            i2p_mode: false,
            clipboard: arboard::Clipboard::new().expect("Failed to initialize clipboard"),
            network_manager,
        })
    }

    pub fn current_tab(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn render_tab(&mut self, tab_index: usize, width: u16) {
        if let Some(tab) = self.tabs.get_mut(tab_index) {
            let content_width = (width as usize).saturating_sub(2);
            if tab.is_source_view {
                tab.rendered_content = tab
                    .html_source
                    .lines()
                    .map(|l| Line::from(l.to_string()))
                    .collect();
                tab.link_regions.clear();
            } else {
                let document = Html::parse_document(&tab.html_source);
                let mut renderer = DomRenderer::new(content_width);
                renderer.render(&document);
                tab.rendered_content = renderer.lines;
                tab.link_regions = renderer.links;
            }
        }
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
        let new_tab = BrowserTab::new(self.id_counter, url);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.id_counter += 1;
        self.submit_request();
    }

    pub fn submit_request(&mut self) {
        let use_i2p = self.i2p_mode;
        let tab = self.current_tab();
        let mut target_url = tab.url_input.clone();

        // URL Normalization
        if !target_url.starts_with("http://") && !target_url.starts_with("https://") {
            if target_url.contains('.') && !target_url.contains(' ') {
                target_url = if target_url.ends_with(".i2p") {
                    format!("http://{}", target_url)
                } else {
                    format!("https://{}", target_url)
                };
            } else {
                let safe_query = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("query", &target_url)
                    .finish();
                target_url = format!("{}{}", MARGINALIA_SEARCH_URL, safe_query);
            }
        }

        // Enforce HTTPS for clearweb requests (security hardening)
        // Allow HTTP for local addresses (localhost, 127.0.0.1, etc.)
        if !use_i2p && target_url.starts_with("http://") && !target_url.contains(".i2p") {
            if let Ok(url) = Url::parse(&target_url) {
                if let Some(host) = url.host_str() {
                    let is_local = match host {
                        "localhost" => true,
                        "127.0.0.1" => true,
                        "::1" => true,
                        host if host.starts_with("127.") => true, // 127.x.x.x range
                        _ => false,
                    };

                    if !is_local {
                        target_url = target_url.replace("http://", "https://");
                    }
                } else {
                    // If we can't parse the host, enforce HTTPS for security
                    target_url = target_url.replace("http://", "https://");
                }
            }
        }

        tab.url_input = target_url.clone();
        let id = tab.id;
        let tx_clone = self.tx.clone();
        let use_i2p = self.i2p_mode;
        let network_manager = Arc::clone(&self.network_manager);

        let domain_for_jump = Url::parse(&target_url)
            .ok()
            .and_then(|u| u.domain().map(|s| s.to_string()))
            .unwrap_or_default();

        tokio::spawn(async move {
            let _ = tx_clone.send(NetworkResponse::Loading(id)).await;

            let client = network_manager.get_client(use_i2p);
            let mut resp_result = client.get(&target_url).send().await;

            if let Ok(ref resp) = resp_result {
                if resp.status() == StatusCode::INTERNAL_SERVER_ERROR
                    || resp.status() == StatusCode::SERVICE_UNAVAILABLE
                {
                    if let Ok(jump_resp) =
                        attempt_jump(&client, &domain_for_jump, tx_clone.clone(), id).await
                    {
                        resp_result = Ok(jump_resp);
                    }
                }
            }

            match resp_result {
                Ok(resp) => {
                    if let Some(len) = resp.content_length() {
                        if len > MAX_PAGE_SIZE_BYTES {
                            let _ = tx_clone
                                .send(NetworkResponse::Error(id, "Page too large".to_string()))
                                .await;
                            return;
                        }
                    }

                    match resp.text().await {
                        Ok(html_text) => {
                            let metadata = parse_html_metadata(&html_text);
                            let _ = tx_clone
                                .send(NetworkResponse::Success(id, metadata.title, html_text))
                                .await;
                        }
                        Err(e) => {
                            let _ = tx_clone
                                .send(NetworkResponse::Error(id, e.to_string()))
                                .await;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx_clone
                        .send(NetworkResponse::Error(id, e.to_string()))
                        .await;
                }
            }
        });
    }

    /// Sanitize filename to prevent path traversal attacks
    fn sanitize_filename(filename: &str) -> String {
        // Get just the filename part, stripping any path components
        let path = Path::new(filename);
        let filename_only = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download.dat");

        // Remove any dangerous characters and ensure it's safe
        let safe_name = filename_only
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
            .collect::<String>();

        // Ensure we have a valid filename
        if safe_name.is_empty() || safe_name == "." || safe_name == ".." {
            "download.dat".to_string()
        } else {
            safe_name
        }
    }

    pub fn trigger_download(&mut self, url: String) {
        let tab_id = self.current_tab().id;
        let tx = self.tx.clone();
        let use_i2p = self.i2p_mode;
        let network_manager = Arc::clone(&self.network_manager);

        tokio::spawn(async move {
            let client = network_manager.get_download_client(use_i2p);

            let res = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx
                        .send(NetworkResponse::Error(
                            tab_id,
                            format!("Download failed!: {}", e),
                        ))
                        .await;
                    return;
                }
            };

            let total_size = res.content_length();

            // Get the system Downloads directory
            let downloads_dir = match UserDirs::new() {
                Some(user_dirs) => {
                    if let Some(downloads) = user_dirs.download_dir() {
                        downloads.to_path_buf()
                    } else {
                        let _ = tx
                            .send(NetworkResponse::Error(
                                tab_id,
                                "Could not determine Downloads directory".to_string(),
                            ))
                            .await;
                        return;
                    }
                }
                None => {
                    let _ = tx
                        .send(NetworkResponse::Error(
                            tab_id,
                            "Could not access user directories".to_string(),
                        ))
                        .await;
                    return;
                }
            };

            // Create Downloads directory if it doesn't exist
            if let Err(e) = tokio::fs::create_dir_all(&downloads_dir).await {
                let _ = tx
                    .send(NetworkResponse::Error(
                        tab_id,
                        format!("Failed to create Downloads directory: {}", e),
                    ))
                    .await;
                return;
            }

            // Extract and sanitize filename to prevent path traversal attacks
            let raw_filename = url.split('/').last().unwrap_or("download.dat");
            let fname = Self::sanitize_filename(raw_filename);
            let file_path = downloads_dir.join(&fname);

            let mut file = match tokio::fs::File::create(&file_path).await {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx
                        .send(NetworkResponse::Error(tab_id, format!("I/O error: {}", e)))
                        .await;
                    return;
                }
            };
            let mut stream = res.bytes_stream();
            let mut downloaded: u64 = 0;

            while let Some(item) = stream.next().await {
                if let Ok(chunk) = item {
                    if file.write_all(&chunk).await.is_err() {
                        break;
                    }
                    downloaded += chunk.len() as u64;
                    let _ = tx
                        .send(NetworkResponse::DownloadProgress(
                            tab_id, downloaded, total_size,
                        ))
                        .await;
                }
            }
            let _ = tx
                .send(NetworkResponse::DownloadFinished(
                    tab_id,
                    file_path.display().to_string(),
                ))
                .await;
        });
    }
}
