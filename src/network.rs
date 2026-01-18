use crate::constants::{
    BROWSING_TIMEOUT_SECS, DOWNLOAD_TIMEOUT_SECS, I2P_PROXY_URL, JUMP_SERVICES, MAX_REDIRECTS,
    USER_AGENT_BROWSING, USER_AGENT_DOWNLOAD,
};
use crate::models::PageMetadata;
use reqwest::{Client, StatusCode};
use scraper::{Html, Selector};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::mpsc;
use url::Url;

pub enum NetworkResponse {
    Success(usize, String, String),
    Error(usize, String),
    Loading(usize),
    Info(usize, String),
    // Variant for downloads
    DownloadProgress(usize, u64, Option<u64>),
    DownloadFinished(usize, String), // tab_id, filename
}

/// Resolve relative URLs against a base URL
pub fn resolve_url(base: &str, target: &str) -> String {
    // If target is already a full URL (e.g. https://google.com), return it immediately
    if let Ok(url) = Url::parse(target) {
        return url.to_string();
    }

    // Handle internal pages or empty bases
    if base.is_empty() || base.starts_with("about:") || base == "New Tab" {
        // If we are on a help page, relative links can't be resolved,
        // so we treat the target as a potential new absolute URL or search query.
        return target.to_string();
    }

    // Try standard joining
    match Url::parse(base) {
        Ok(base_url) => {
            match base_url.join(target) {
                Ok(joined) => joined.to_string(),
                Err(_) => target.to_string(), // Fallback to target string if join fails
            }
        }
        Err(_) => target.to_string(), // Fallback if base is unparseable
    }
}

pub struct NetworkManager {
    client: Client,
    i2p_client: Client,
    download_client: Client,
    i2p_download_client: Client,
}

impl NetworkManager {
    /// Private helper method to build a reqwest client with consistent configuration
    fn build_client(
        user_agent: &str,
        timeout: Duration,
        use_proxy: bool,
        include_headers: bool,
    ) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
        let mut builder = Client::builder().user_agent(user_agent).timeout(timeout);

        if include_headers {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("Referer", reqwest::header::HeaderValue::from_static(""));
            builder = builder.default_headers(headers);
        }

        if use_proxy {
            let proxy = reqwest::Proxy::http(I2P_PROXY_URL)?;
            builder = builder.proxy(proxy);
        }

        // Always apply redirect policy for browsing clients
        if include_headers {
            builder = builder.redirect(strict_redirect_policy());
        }

        Ok(builder.build()?)
    }

    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create all four clients using the build_client helper method
        let client = Self::build_client(
            USER_AGENT_BROWSING,
            Duration::from_secs(BROWSING_TIMEOUT_SECS),
            false,
            true,
        )?;
        let i2p_client = Self::build_client(
            USER_AGENT_BROWSING,
            Duration::from_secs(BROWSING_TIMEOUT_SECS),
            true,
            true,
        )?;
        let download_client = Self::build_client(
            USER_AGENT_DOWNLOAD,
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            false,
            false,
        )?;
        let i2p_download_client = Self::build_client(
            USER_AGENT_DOWNLOAD,
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            true,
            false,
        )?;

        Ok(Self {
            client,
            i2p_client,
            download_client,
            i2p_download_client,
        })
    }

    pub fn get_client(&self, i2p_mode: bool) -> &Client {
        if i2p_mode {
            &self.i2p_client
        } else {
            &self.client
        }
    }

    pub fn get_download_client(&self, i2p_mode: bool) -> &Client {
        if i2p_mode {
            &self.i2p_download_client
        } else {
            &self.download_client
        }
    }
}

pub fn parse_html_metadata(html: &str) -> PageMetadata {
    let document = Html::parse_document(html);
    static TITLE_SELECTOR: OnceLock<Selector> = OnceLock::new();
    let title_selector = TITLE_SELECTOR.get_or_init(|| Selector::parse("title").unwrap());

    let title = document
        .select(title_selector)
        .next()
        .map(|element| {
            element
                .text()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "No Title".to_string());

    PageMetadata { title }
}

pub fn strict_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("Too many redirects");
        }
        if let Some(host) = attempt.url().host_str() {
            if host == "localhost" || host == "127.0.0.1" || host == "::1" {
                return attempt.error("Blocked redirect to local network (SSRF Protection)");
            }
        }
        attempt.follow()
    })
}

pub async fn attempt_jump(
    client: &Client,
    target_domain: &str,
    tx: mpsc::Sender<NetworkResponse>,
    id: usize,
) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
    let _ = tx
        .send(NetworkResponse::Info(
            id,
            "Address not found. Requesting Jump Helper...".to_string(),
        ))
        .await;
    for service_base in JUMP_SERVICES {
        let jump_url = format!("{}{}", service_base, target_domain);
        let _ = tx
            .send(NetworkResponse::Info(
                id,
                format!("Contacting jump service: {}", service_base),
            ))
            .await;
        let response = client.get(&jump_url).send().await?;
        if response.status() == StatusCode::OK {
            return Ok(response);
        }
    }
    Err("All jump services failed.".into())
}
