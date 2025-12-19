use crate::models::{LinkRegion, InputMode};
use crate::network::{NetworkResponse, parse_html_metadata, strict_redirect_policy, attempt_jump};
use crate::renderer::DomRenderer;

use ratatui::text::Line;
use scraper::Html;
use url::Url;
use tokio::sync::mpsc;
use std::time::Duration;
use reqwest::StatusCode;

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
}

impl BrowserTab {
    pub fn new(id: usize, initial_url: String) -> Self {
        let help_html = r#"
            <h1>NAVIGATION</h1>
            <p><b>Up / Down Arrow:</b> Scroll page by 1 line.</p>
            <p><b>Scroll Wheel:</b> Scroll page by 3 lines.</p>
            <p><b>Tab:</b> Cycle selection through links on the screen.</p>
            <p><b>Enter:</b> Open the currently selected link.</p>
            <p><b>Left Click:</b> Open the clicked link.</p>
            <p><b>Ctrl + Click:</b> Open the clicked link in a New Tab.</p>
            <p><b>Backspace / Left Arrow:</b> Go back in history.</p>
            <hr>
            <h1>BROWSER CONTROL</h1>
            <p><b>t:</b> Open the currently selected link in a New Tab.</p>
            <p><b>n:</b> Open a blank New Tab.</p>
            <p><b>w:</b> Close the current tab.</p>
            <p><b>[ and ]:</b> Switch between Previous / Next tab.</p>
            <p><b>e:</b> Enter 'Edit Mode' to type a new URL.</p>
            <p><b>p:</b> Toggle i2p proxy mode.</p>
            <p><b>q:</b> Quit the browser.</p>
            <p><b>v:</b> Toggle Page Source View.</p>
            <hr>
            <h1>EDIT MODE (Press 'e')</h1>
            <p><b>Typing:</b> Type a URL or a search query.</p>
            <p><b>Enter:</b> Submit the request.</p>
            <p><b>Esc:</b> Cancel and return to Normal Mode.</p>
            <hr>
            <h1>SUPPLEMENTARY NOTES</h1>
            <p>Weird things may happen on Javascript-heavy sites with anti-AI scraping measures.</p>
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
}
