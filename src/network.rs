use crate::models::PageMetadata;
use reqwest::{Client, StatusCode};
use scraper::{Html, Selector};
use std::sync::OnceLock;
use tokio::sync::mpsc;

pub enum NetworkResponse {
    Success(usize, String, String),
    Error(usize, String),
    Loading(usize),
    Info(usize, String),
    // Variant for downloads
    DownloadProgress(usize, u64, Option<u64>),
    DownloadFinished(usize, String), // tab_id, filename
}

pub const JUMP_SERVICES: &[&str] = &[
    "http://i2p-projekt.i2p/jump/",
    "http://stats.i2p/jump/",
    "http://reg.i2p/jump/",
];

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
        if attempt.previous().len() > 10 {
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
