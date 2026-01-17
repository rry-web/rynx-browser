use crate::models::{LinkRegion, InputMode, Selection};
use crate::network::{NetworkResponse, parse_html_metadata, strict_redirect_policy, attempt_jump};
use crate::renderer::DomRenderer;

use ratatui::text::Line;
use scraper::Html;
use url::Url;
use tokio::sync::mpsc;
use std::time::Duration;
use reqwest::StatusCode;

use tokio::io::AsyncWriteExt; // Required for streaming to file
use futures_util::StreamExt;

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
        let sel = match &self.selection {
            Some(s) => s,
            None => return String::new(),
        };

        // Normalize selection (handle backwards selection)
        let (s_line, s_char, e_line, e_char) = if (sel.start_line, sel.start_char) <= (sel.end_line, sel.end_char) {
            (sel.start_line, sel.start_char, sel.end_line, sel.end_char)
        } else {
            (sel.end_line, sel.end_char, sel.start_line, sel.start_char)
        };

        let mut result = String::new();
        for i in s_line..=e_line {
            if let Some(line) = self.rendered_content.get(i) {
                let line_str = line.to_string();
                let start = if i == s_line { s_char } else { 0 };
                let end = if i == e_line { e_char } else { line_str.chars().count() };

                // Map char index to byte index
                let byte_start = line_str.char_indices().nth(start).map(|(idx, _)| idx).unwrap_or(0);
                let byte_end = line_str.char_indices().nth(end).map(|(idx, _)| idx).unwrap_or(line_str.len());

                //result.push_str(&line_str[start..end]);
                result.push_str(&line_str[byte_start..byte_end]);
                if i < e_line {
                    result.push('\n');
                }
            }
        }
        result
    }
    pub fn new(id: usize, initial_url: String) -> Self {
        let help_html = r#"
            <h1>NAVIGATION/NORMAL</h1>
            <p><b>h / j / k / l:</b> Move cursor (Vim-style). View scrolls to follow.</p>
            <p><b>Left Click:</b> Position cursor and follow links.</p>
            <p><b>Ctrl + Left Click:</b> Open link in new tab.</p>
            <p><b>Up / Down Arrow:</b> Scroll page without moving cursor.</p>
            <p><b>Scroll:</b> Scroll page up/down by 3 lines.</p>
            <p><b>Tab / Shift + Tab:</b> Cycle through links (Forward / Backward).</p>
            <p><b>Enter:</b> Open the currently selected link.</p>
            <p><b>Backspace / Left Arrow:</b> Go back to previous page.</p>
            <p><b>d:</b> Download from the currently selected link.</p>
            <p><b>Esc:</b> Clear finished or failed download overlays, or return to Normal Mode if in Edit/Visual.</p>
            <hr>
            <h1>CLIPBOARD & VISUAL MODES</h1>
            <p><b>v:</b> Enter <b>Visual Mode</b> (Character selection).</p>
            <p><b>y (in Visual):</b> Yank (Copy) selected text to system clipboard.</p>
            <hr>
            <h1>EDIT MODE (Press 'e')</h1>
            <p><b>Ctrl + u:</b> Clear address bar.</p>
            <p><b>Ctrl + y:</b> Copy address to clipboard.</p>
            <p><b>Ctrl + v:</b> Paste from clipboard.</p>
            <p><b>Ctrl + k:</b> Clear address bar AND paste.</p>
            <p>Non valid URLs will automatically search in Marginalia, but this currently doesn't work due to lack of JS.</p>
            <hr>
            <h1>BROWSER CONTROL</h1>
            <p><b>n / w:</b> New Tab / Close Tab.</p>
            <p><b>t:</b> Open highlighted address in new tab.</p>
            <p><b>p:</b> Toggle I2P mode.</p>
            <p><b>q:</b> Quit the browser.</p>
            <p><b>[ / ]:</b> Switch between tabs.</p>
            <p><b>Shift + V:</b> Toggle Page Source View.</p>
        "#;
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
        }
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
}

impl App {
    pub fn new(tx: mpsc::Sender<NetworkResponse>, rx: mpsc::Receiver<NetworkResponse>) -> Self {
        let initial_tab = BrowserTab::new(0, String::from("https://www.rust-lang.org"));
        Self {
            tabs: vec![initial_tab],
            active_tab_index: 0,
            id_counter: 1,
            tx,
            rx,
            i2p_mode: false,
            clipboard: arboard::Clipboard::new().expect("Failed to initialize clipboard"),
        }
    }

    pub fn current_tab(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn render_tab(&mut self, tab_index: usize, width: u16) {
        if let Some(tab) = self.tabs.get_mut(tab_index) {
            let content_width = (width as usize).saturating_sub(2);
            if tab.is_source_view {
                tab.rendered_content = tab.html_source
                    .lines()
                    .map(|l| Line::from(l.to_string()))
                    .collect();
                tab.link_regions.clear();
            }
            else {
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
                target_url = format!("https://search.marginalia.nu/search?{}", safe_query);
            }
        }

        tab.url_input = target_url.clone();
        let id = tab.id;
        let tx_clone = self.tx.clone();
        let use_i2p = self.i2p_mode;

        let domain_for_jump = Url::parse(&target_url)
            .ok()
            .and_then(|u| u.domain().map(|s| s.to_string()))
            .unwrap_or_default();

        tokio::spawn(async move {
            let _ = tx_clone.send(NetworkResponse::Loading(id)).await;

            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("Referer", reqwest::header::HeaderValue::from_static(""));

            let mut builder = reqwest::Client::builder()
                .user_agent("RustBrowser/0.1.0 reqwest/0.12")
                .timeout(Duration::from_secs(100))
                .default_headers(headers)
                .redirect(strict_redirect_policy());

            if use_i2p {
                if let Ok(proxy) = reqwest::Proxy::http("http://127.0.0.1:4444") {
                    builder = builder.proxy(proxy);
                }
            }

            match builder.build() {
                Ok(client) => {
                    let mut resp_result = client.get(&target_url).send().await;

                    if let Ok(ref resp) = resp_result {
                        if resp.status() == StatusCode::INTERNAL_SERVER_ERROR || resp.status() == StatusCode::SERVICE_UNAVAILABLE {
                             if let Ok(jump_resp) = attempt_jump(&client, &domain_for_jump, tx_clone.clone(), id).await {
                                 resp_result = Ok(jump_resp);
                             }
                        }
                    }

                    match resp_result {
                        Ok(resp) => {
                            if let Some(len) = resp.content_length() {
                                if len > 10 * 1024 * 1024 {
                                    let _ = tx_clone.send(NetworkResponse::Error(id, "Page too large".to_string())).await;
                                    return;
                                }
                            }

                            match resp.text().await {
                                Ok(html_text) => {
                                    let metadata = parse_html_metadata(&html_text);
                                    let _ = tx_clone.send(NetworkResponse::Success(id, metadata.title, html_text)).await;
                                }
                                Err(e) => {
                                    let _ = tx_clone.send(NetworkResponse::Error(id, e.to_string())).await;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx_clone.send(NetworkResponse::Error(id, e.to_string())).await;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx_clone.send(NetworkResponse::Error(id, e.to_string())).await;
                }
            }
        });
    }
    pub fn trigger_download(&mut self, url: String) {
        let tab_id = self.current_tab().id;
        let tx = self.tx.clone();
        let use_i2p = self.i2p_mode;

        tokio::spawn(async move {
            let mut builder = reqwest::Client::builder()
                .user_agent("RynxBrowser/0.1.0")
                .timeout(std::time::Duration::from_secs(3000));

            if use_i2p {
                if let Ok(proxy) = reqwest::Proxy::http("http://127.0.0.1:4444") {
                    builder = builder.proxy(proxy);
                }
            }
            //let client = reqwest::Client::new();
            let client = builder.build().unwrap_or_else(|_| reqwest::Client::new());

            let res = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(NetworkResponse::Error(tab_id, format!("Download failed!: {}", e))).await;
                    return;
                }
            };

            let total_size = res.content_length();
            let fname = url.split('/').last().unwrap_or("download.dat").to_string();
            let mut file = match tokio::fs::File::create(&fname).await {
                Ok(f) => f,
                Err(e) => {
                    let _ = tx.send(NetworkResponse::Error(tab_id, format!("I/O error: {}", e))).await;
                    return;
                }
            };
            let mut stream = res.bytes_stream();
            let mut downloaded: u64 = 0;

            while let Some(item) = stream.next().await {
                if let Ok(chunk) = item {
                    if file.write_all(&chunk).await.is_err() { break; }
                    downloaded += chunk.len() as u64;
                    let _ = tx.send(NetworkResponse::DownloadProgress(tab_id, downloaded, total_size)).await;
                }
            }
            let _ = tx.send(NetworkResponse::DownloadFinished(tab_id, fname)).await;
        });
    }
}
