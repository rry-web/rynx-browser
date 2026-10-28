//! Images: loading a page's images (all of them on I, or by themselves as the settings allow),
//! drawing them in the page or in a full-screen viewer (i), in whatever the terminal supports:
//! Kitty, iTerm2 or Sixel graphics, or coloured text blocks where it supports none (macOS
//! Terminal).
//!
//! Images are fetched like pages (HTTPS, redirect checks, I2P through the proxy, the cache), but
//! a public page can never load images from the local network, where a request can make a router
//! or printer act. They're decoded off the UI thread by memory-safe decoders with limits on size,
//! and big ones are scaled down.

use super::*;
use crate::renderer::PageImage;
use crate::settings::{ImageDisplay, ImageLoading, ImageProtocol};
use image::DynamicImage;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use std::collections::HashSet;
use std::sync::Mutex;

/// Largest image fetched
const MAX_IMAGE_BYTES: u64 = crate::cache::MAX_FILE_BYTES;
/// Largest picture decoded, in pixels per side; and how far big ones are scaled down
const MAX_DECODE_SIDE: u32 = 12_000;
const MAX_KEPT_SIDE: u32 = 2048;
/// Images fetched at once
const PARALLEL_FETCHES: usize = 6;
/// Tallest an image drawn in the page gets, in rows
const MAX_INLINE_ROWS: u16 = 24;
/// Most memory decoded images may use (4 bytes a pixel); the least recently used are dropped
/// beyond it, and decoded again from the cache if they're needed on screen
const DECODED_BUDGET: usize = 192 * 1024 * 1024;

/// The browser's images: decoded ones, those loading or failed, and their drawing state
pub struct Images {
    /// How images are drawn on this terminal
    pub picker: Picker,
    /// What the terminal said it supports, for going back to it from a chosen protocol
    detected: ratatui_image::picker::ProtocolType,
    decoded: HashMap<String, Arc<DynamicImage>>,
    /// Least recently used first, and the bytes they take
    decoded_order: std::collections::VecDeque<String>,
    decoded_bytes: usize,
    /// Every image's size in pixels, kept after it's dropped so the page's layout doesn't change
    dimensions: HashMap<String, (u32, u32)>,
    /// Images the page needed on screen that had been dropped; reloaded after the frame
    pub missing: Mutex<HashSet<String>>,
    loading: HashSet<String>,
    failed: HashMap<String, String>,
    /// Images still loading, per tab
    pending: HashMap<usize, usize>,
    fetches: Arc<tokio::sync::Semaphore>,
    /// Drawing state per image address; the UI draws through a shared reference
    pub drawn: Mutex<HashMap<String, StatefulProtocol>>,
    /// The image open in the full-screen viewer: its address and description
    pub viewer: Option<(String, String)>,
    pub viewer_state: Mutex<Option<StatefulProtocol>>,
    /// An image asked for in the viewer that's still loading
    viewer_wanted: Option<(String, String)>,
}

impl Default for Images {
    fn default() -> Self {
        Images {
            picker: Picker::halfblocks(),
            detected: ratatui_image::picker::ProtocolType::Halfblocks,
            decoded: HashMap::new(),
            decoded_order: std::collections::VecDeque::new(),
            decoded_bytes: 0,
            dimensions: HashMap::new(),
            missing: Mutex::default(),
            loading: HashSet::new(),
            failed: HashMap::new(),
            pending: HashMap::new(),
            fetches: Arc::new(tokio::sync::Semaphore::new(PARALLEL_FETCHES)),
            drawn: Mutex::default(),
            viewer: None,
            viewer_state: Mutex::default(),
            viewer_wanted: None,
        }
    }
}

impl std::fmt::Debug for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Images").field("decoded", &self.decoded.len()).finish()
    }
}

/// The memory a decoded image takes
fn image_bytes(image: &DynamicImage) -> usize {
    image.width() as usize * image.height() as usize * 4
}

impl Images {
    pub fn is_decoded(&self, url: &str) -> bool {
        self.decoded.contains_key(url)
    }

    /// Images decoded and held in memory, and the bytes they take
    pub fn decoded_memory(&self) -> (usize, usize) {
        (self.decoded.len(), self.decoded_bytes)
    }

    /// Keep a decoded image, dropping the least recently used beyond the budget (with their
    /// drawing state, which holds another copy)
    pub fn keep(&mut self, url: String, image: Arc<DynamicImage>) {
        if let Some(old) = self.decoded.remove(&url) {
            self.decoded_bytes -= image_bytes(&old);
            self.decoded_order.retain(|u| *u != url);
        }
        self.dimensions.insert(url.clone(), (image.width(), image.height()));
        self.decoded_bytes += image_bytes(&image);
        self.decoded.insert(url.clone(), image);
        self.decoded_order.push_back(url);
        while self.decoded_bytes > DECODED_BUDGET && self.decoded_order.len() > 1 {
            let Some(oldest) = self.decoded_order.pop_front() else { break };
            if let Some(old) = self.decoded.remove(&oldest) {
                self.decoded_bytes -= image_bytes(&old);
            }
            self.drawn.lock().unwrap_or_else(|e| e.into_inner()).remove(&oldest);
        }
    }

    /// Mark an image as just used, so it's the last to be dropped
    fn touch(&mut self, url: &str) {
        if let Some(i) = self.decoded_order.iter().position(|u| u == url) {
            let url = self.decoded_order.remove(i).expect("position is in range");
            self.decoded_order.push_back(url);
        }
    }

    /// Ask the terminal how it draws images (once, at startup, before input is read), then use
    /// the protocol the settings choose. Only terminals known to draw images are asked: the
    /// question is answered on standard input, and a terminal that never answers would leave the
    /// asking thread waiting there, taking the user's first keys. Others get text blocks.
    pub fn detect(&mut self, setting: ImageProtocol) {
        // Only a terminal known to draw images is asked, chosen method or not: the question is
        // answered on standard input, and one a terminal doesn't answer in full would keep reading
        // it, taking the first keys typed. A method chosen for another terminal is used with
        // the usual cell size instead.
        let ask = setting != ImageProtocol::Blocks && draws_images(&|name| std::env::var(name).ok());
        if ask && let Ok(picker) = Picker::from_query_stdio() {
            self.detected = picker.protocol_type();
            self.picker = picker;
        }
        self.use_protocol(setting);
    }

    /// Draw with the protocol the settings choose (Auto: what the terminal said)
    pub fn use_protocol(&mut self, setting: ImageProtocol) {
        use ratatui_image::picker::ProtocolType;
        let protocol = match setting {
            ImageProtocol::Auto => self.detected,
            ImageProtocol::Kitty => ProtocolType::Kitty,
            ImageProtocol::Iterm2 => ProtocolType::Iterm2,
            ImageProtocol::Sixel => ProtocolType::Sixel,
            ImageProtocol::Blocks => ProtocolType::Halfblocks,
        };
        if self.picker.protocol_type() != protocol {
            self.picker.set_protocol_type(protocol);
            // Images already drawn are drawn again the new way
            self.drawn.lock().unwrap_or_else(|e| e.into_inner()).clear();
        }
    }

    /// The protocol in use, for the settings page
    pub fn protocol_name(&self) -> &'static str {
        use ratatui_image::picker::ProtocolType;
        match self.picker.protocol_type() {
            ProtocolType::Kitty => "Kitty graphics",
            ProtocolType::Iterm2 => "iTerm2 images",
            ProtocolType::Sixel => "Sixel graphics",
            ProtocolType::Halfblocks => "coloured text blocks",
        }
    }
}

/// Whether the terminal is one known to draw images (and so to answer questions about it), going
/// by what it puts in the environment
pub fn draws_images(env: &dyn Fn(&str) -> Option<String>) -> bool {
    let is = |name: &str, values: &[&str]| env(name).is_some_and(|v| values.iter().any(|x| v.eq_ignore_ascii_case(x)));
    let set = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    let term = env("TERM").unwrap_or_default();
    is("TERM_PROGRAM", &["iTerm.app", "WezTerm", "ghostty", "vscode"])
        || is("LC_TERMINAL", &["iTerm2"])
        || set("KITTY_WINDOW_ID")
        || set("WEZTERM_EXECUTABLE")
        || set("KONSOLE_VERSION")
        || ["xterm-kitty", "xterm-ghostty", "foot", "foot-extra", "mlterm", "contour"].contains(&term.as_str())
}

/// The site part of a host for "same site": without www.
fn site(url: &Url) -> String {
    url.host_str().unwrap_or("").trim_start_matches("www.").to_ascii_lowercase()
}

/// Whether an image belongs to the page's site (the same host, or one of its subdomains, as
/// cdn.example.com is for example.com)
fn same_site(image: &Url, page: &Url) -> bool {
    let (image, page) = (site(image), site(page));
    image == page || image.ends_with(&format!(".{}", page))
}

/// Decode an image within limits, scaling big ones down
fn decode(data: &[u8]) -> Result<DynamicImage, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| format!("unreadable image: {}", e))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| format!("couldn't decode the image: {}", e))?;
    Ok(if image.width() > MAX_KEPT_SIDE || image.height() > MAX_KEPT_SIDE {
        image.thumbnail(MAX_KEPT_SIDE, MAX_KEPT_SIDE)
    } else {
        image
    })
}

/// Fetch an image's bytes, from the cache if it has them
async fn fetch(
    client: &reqwest::Client,
    url: &str,
    i2p_mode: bool,
    cache: &crate::cache::Cache,
) -> Result<Arc<Vec<u8>>, String> {
    let private = crate::cache::is_i2p(url, i2p_mode);
    if let Some(data) = cache.get_bytes(url, private) {
        return Ok(data);
    }
    let response = crate::network::get_following_redirects(client, url, i2p_mode).await?;
    if !response.status().is_success() {
        return Err(format!("the site answered {}", response.status()));
    }
    let kind = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    if kind.starts_with("image/svg") {
        return Err("SVG images can't be shown yet".to_string());
    }
    let keep = crate::cache::freshness(response.headers(), crate::cache::now()).is_some();
    let data = Arc::new(crate::network::read_bytes_limited(response, MAX_IMAGE_BYTES).await?);
    if keep {
        cache.put_bytes(url, Arc::clone(&data), private);
    }
    Ok(data)
}

/// The columns and rows to draw an image of `size` pixels in, keeping its shape on a terminal
/// whose character cells are `font` pixels, within `available` columns
fn cells_for(size: (u32, u32), font: (u16, u16), available: u16) -> (u16, u16) {
    let (fw, fh) = (font.0.max(1) as f64, font.1.max(1) as f64);
    let (iw, ih) = (size.0.max(1) as f64, size.1.max(1) as f64);
    let mut columns = (iw / fw).ceil().min(available.max(1) as f64).max(1.0);
    let mut rows = (columns * fw * ih / iw / fh).ceil().max(1.0);
    if rows > MAX_INLINE_ROWS as f64 {
        rows = MAX_INLINE_ROWS as f64;
        columns = (rows * fh * iw / ih / fw).ceil().clamp(1.0, available.max(1) as f64);
    }
    (columns as u16, rows as u16)
}

impl App {
    /// The images of a tab's page, with their addresses resolved
    fn page_images(&self, index: usize) -> Vec<(String, PageImage)> {
        let tab = &self.tabs[index];
        tab.page_images
            .iter()
            .map(|image| (crate::network::resolve_url(tab.link_base(), &image.src), image.clone()))
            .filter(|(url, _)| url.starts_with("https://") || url.starts_with("http://"))
            .collect()
    }

    /// Load a tab's images: all of them when asked (I), or only those the settings allow to
    /// load by themselves
    pub fn load_images(&mut self, index: usize, asked: bool) {
        let page = self.tabs[index].page_url.as_deref().and_then(|p| Url::parse(p).ok());
        let mut wanted: Vec<String> = Vec::new();
        let mut blocked = 0;
        for (url, _) in self.page_images(index) {
            if self.images.decoded.contains_key(&url) || self.images.loading.contains(&url) || wanted.contains(&url) {
                continue;
            }
            let Ok(parsed) = Url::parse(&url) else { continue };
            // An image request can make a router or printer act: never from a public page
            if crate::network::is_local_network_url(&url)
                && !page.as_ref().is_some_and(|p| crate::network::is_local_network_url(p.as_str()))
            {
                blocked += 1;
                continue;
            }
            let allowed = asked
                || match self.settings.images {
                    ImageLoading::Ask => false,
                    ImageLoading::Site => page.as_ref().is_some_and(|p| same_site(&parsed, p)),
                    ImageLoading::All => true,
                };
            if allowed && (asked || !self.images.failed.contains_key(&url)) {
                wanted.push(url);
            }
        }
        if asked {
            let tab = &mut self.tabs[index];
            tab.status_message = match (wanted.len(), blocked) {
                (0, 0) if tab.page_images.is_empty() => String::from("This page has no images"),
                (0, 0) => String::from("This page's images are loaded"),
                (0, _) => format!("{} images on your local network weren't loaded", blocked),
                (n, _) => format!("Loading {} {}...", n, if n == 1 { "image" } else { "images" }),
            };
        }
        for url in wanted {
            self.fetch_image(index, url);
        }
    }

    fn fetch_image(&mut self, index: usize, url: String) {
        self.images.loading.insert(url.clone());
        self.images.failed.remove(&url);
        let tab_id = self.tabs[index].id;
        *self.images.pending.entry(tab_id).or_default() += 1;
        let client = self.network_manager.get_client(self.i2p_mode).clone();
        let i2p_mode = self.i2p_mode;
        let cache = Arc::clone(&self.cache);
        let fetches = Arc::clone(&self.images.fetches);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _permit = fetches.acquire_owned().await;
            let result = match fetch(&client, &url, i2p_mode, &cache).await {
                Ok(data) => tokio::task::spawn_blocking(move || decode(&data))
                    .await
                    .unwrap_or_else(|_| Err("the image couldn't be decoded".to_string()))
                    .map(Arc::new),
                Err(e) => Err(e),
            };
            let _ = tx.send(NetworkResponse::ImageLoaded(tab_id, url, result)).await;
        });
    }

    /// An image arrived (or failed): keep it, and lay the page out again to show it
    pub fn finish_image(&mut self, index: usize, url: String, result: Result<Arc<DynamicImage>, String>) {
        self.images.loading.remove(&url);
        let tab_id = self.tabs[index].id;
        let pending = self.images.pending.entry(tab_id).or_default();
        *pending = pending.saturating_sub(1);
        let done = *pending == 0;
        match result {
            Ok(image) => {
                self.images.keep(url.clone(), image);
            }
            Err(e) => {
                self.images.failed.insert(url.clone(), e);
            }
        }
        if self.images.viewer_wanted.as_ref().is_some_and(|(u, _)| *u == url) {
            let (url, alt) = self.images.viewer_wanted.take().expect("checked");
            self.open_viewer(url, alt);
        }
        // Lay the page out once the batch is in (and every few images on long pages)
        let decoded_here = self.page_images(index).iter().filter(|(u, _)| self.images.decoded.contains_key(u)).count();
        if self.settings.image_display == ImageDisplay::Inline && (done || decoded_here % 8 == 0) {
            self.update_image_sizes(index);
        }
        if done {
            // Why the page's images failed (the image finished last may not be one of them)
            let reasons: Vec<String> =
                self.page_images(index).iter().filter_map(|(u, _)| self.images.failed.get(u).cloned()).collect();
            let tab = &mut self.tabs[index];
            if tab.status_message.starts_with("Loading") || !reasons.is_empty() {
                tab.status_message = match reasons.len() {
                    0 => String::from("Images loaded (i views one full screen)"),
                    n => {
                        let first = &reasons[0];
                        let others = if reasons.iter().all(|r| r == first) { "" } else { ", among other reasons" };
                        format!("Images loaded; {} couldn't be: {}{}", n, first, others)
                    }
                };
            }
        }
    }

    /// Reserve space in the page for its decoded images, and lay it out again
    fn update_image_sizes(&mut self, index: usize) {
        let available = self.terminal_width.saturating_sub(4);
        let font = self.images.picker.font_size();
        let font = (font.width, font.height);
        let mut sizes = HashMap::new();
        for (url, image) in self.page_images(index) {
            // Dimensions are kept after an image is dropped from memory, so the layout stays put
            if let Some(size) = self.images.dimensions.get(&url) {
                sizes.insert(image.src.clone(), cells_for(*size, font, available));
            }
        }
        let tab = &mut self.tabs[index];
        if *tab.image_sizes == sizes {
            return;
        }
        tab.image_sizes = Arc::new(sizes);
        // Keep the place on the page while the images make room for themselves
        tab.pending_restore = Some((tab.scroll, tab.cursor_line));
        tab.restore_position_after_render();
        tab.content_changed();
        let width = self.terminal_width;
        self.render_tab(index, width);
    }

    /// Load the images a freshly laid-out page may load by itself
    pub(super) fn load_images_if_pending(&mut self, index: usize) {
        if std::mem::take(&mut self.tabs[index].load_images_after_render) {
            if self.settings.image_display == ImageDisplay::Inline {
                self.update_image_sizes(index);
            }
            self.load_images(index, false);
        }
    }

    /// i: show the selected image (or the first one on screen) full screen, loading it if needed
    pub fn view_image(&mut self, viewport_height: usize) {
        let index = self.active_tab_index;
        let tab = &self.tabs[index];
        let chosen = tab
            .link_regions
            .get(tab.selected_link_index)
            .and_then(|r| r.image)
            .or_else(|| {
                // The first image on screen
                tab.link_regions
                    .iter()
                    .find(|r| {
                        r.image.is_some() && r.line_index >= tab.scroll && r.line_index < tab.scroll + viewport_height
                    })
                    .and_then(|r| r.image)
            })
            .or(if tab.page_images.is_empty() { None } else { Some(0) });
        let Some(image) = chosen.and_then(|i| tab.page_images.get(i)).cloned() else {
            self.current_tab().status_message = String::from("There's no image here");
            return;
        };
        let url = crate::network::resolve_url(tab.link_base(), &image.src);
        if self.images.decoded.contains_key(&url) {
            self.images.touch(&url);
            self.open_viewer(url, image.alt);
            return;
        }
        if !url.starts_with("http") {
            self.current_tab().status_message = String::from("This image can't be loaded");
            return;
        }
        let page = tab.page_url.clone().unwrap_or_default();
        if crate::network::is_local_network_url(&url) && !crate::network::is_local_network_url(&page) {
            self.current_tab().status_message = String::from("Blocked: a public page's image on your local network");
            return;
        }
        self.images.viewer_wanted = Some((url.clone(), image.alt));
        if !self.images.loading.contains(&url) {
            self.fetch_image(index, url);
        }
        self.current_tab().status_message = String::from("Loading the image...");
    }

    fn open_viewer(&mut self, url: String, alt: String) {
        let Some(image) = self.images.decoded.get(&url) else { return };
        let state = self.images.picker.new_resize_protocol((**image).clone());
        *self.images.viewer_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
        self.images.viewer = Some((url, alt));
    }

    pub fn close_viewer(&mut self) {
        self.images.viewer = None;
        *self.images.viewer_state.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Decode again the images the last frame needed but had been dropped from memory (from the
    /// cache, so without the network unless the cache let them go too)
    pub fn restore_missing_images(&mut self) {
        let missing: Vec<String> = self.images.missing.lock().unwrap_or_else(|e| e.into_inner()).drain().collect();
        for url in missing {
            if !self.images.loading.contains(&url) && !self.images.decoded.contains_key(&url) {
                self.fetch_image(self.active_tab_index, url);
            }
        }
    }

    /// The drawing state for an image in the page, made on first use. An image that was dropped
    /// from memory is noted, to be decoded again after this frame.
    pub fn with_drawn_image<R>(&self, url: &str, draw: impl FnOnce(&mut StatefulProtocol) -> R) -> Option<R> {
        let Some(image) = self.images.decoded.get(url) else {
            if self.images.dimensions.contains_key(url) {
                self.images.missing.lock().unwrap_or_else(|e| e.into_inner()).insert(url.to_string());
            }
            return None;
        };
        let mut drawn = self.images.drawn.lock().unwrap_or_else(|e| e.into_inner());
        let state =
            drawn.entry(url.to_string()).or_insert_with(|| self.images.picker.new_resize_protocol((**image).clone()));
        Some(draw(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_images_stay_within_their_budget() {
        let mut images = Images::default();
        // 2048 x 2048 pixels: 16 MB each, so the budget holds 12
        for i in 0..20 {
            images.keep(format!("https://x.example/{i}.png"), Arc::new(DynamicImage::new_rgba8(2048, 2048)));
        }
        let (count, bytes) = images.decoded_memory();
        assert!(bytes <= DECODED_BUDGET, "{bytes}");
        assert_eq!(count, 12);
        assert!(!images.is_decoded("https://x.example/0.png"), "the oldest went first");
        assert!(images.is_decoded("https://x.example/19.png"));
        // Its size is remembered, so the page keeps its layout
        assert_eq!(images.dimensions.get("https://x.example/0.png"), Some(&(2048, 2048)));

        // Using one keeps it longest
        images.touch("https://x.example/8.png");
        images.keep("https://x.example/new.png".into(), Arc::new(DynamicImage::new_rgba8(2048, 2048)));
        assert!(images.is_decoded("https://x.example/8.png"));
        assert!(!images.is_decoded("https://x.example/9.png"));
    }
}
