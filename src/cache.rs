//! The page cache: pages kept so Back and Forward are instant, unchanged pages aren't downloaded
//! again, and a site that can't be reached can still be read.
//!
//! Pages are always kept in memory for the session (up to MEMORY_LIMIT). On disk they're kept only
//! when the user turns the disk cache on, and pages from I2P only with a separate switch, since a
//! file per site visited is a record of the visit. Disk entries are gzip files in the cache folder,
//! named by a hash of the address, oldest removed first beyond the size limit.
//!
//! What's cached is the page as rynx shows it (after decoding, feed conversion and so on), with the
//! validators (ETag, Last-Modified) for asking the site whether it changed, and how long the site
//! said it stays fresh. Responses marked no-store are never kept.

use crate::network::{Page, PageKind};
use reqwest::header::HeaderMap;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Most page text kept in memory, in bytes
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// Largest file (an image) kept
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// The disk cache's default size limit, in megabytes
pub const DEFAULT_DISK_MB: u64 = 200;

/// A page as kept in the cache
#[derive(Clone, Debug, PartialEq)]
pub struct CachedPage {
    pub page: Page,
    /// When it was fetched (seconds since 1970)
    pub stored: u64,
    /// Until when the site said it stays fresh, if it said
    pub fresh_until: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl CachedPage {
    pub fn is_fresh(&self, now: u64) -> bool {
        self.fresh_until.is_some_and(|until| now < until)
    }

    /// Whether the site can be asked if this changed, instead of sending it again
    pub fn can_revalidate(&self) -> bool {
        self.etag.is_some() || self.last_modified.is_some()
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How a response may be cached: None if it mustn't be (no-store), otherwise until when it's
/// fresh, if the site said (Cache-Control max-age, or Expires)
pub fn freshness(headers: &HeaderMap, now: u64) -> Option<Option<u64>> {
    let control = headers
        .get_all(reqwest::header::CACHE_CONTROL)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join(",")
        .to_ascii_lowercase();
    let directives: Vec<&str> = control.split(',').map(str::trim).collect();
    if directives.contains(&"no-store") {
        return None;
    }
    if directives.contains(&"no-cache") {
        return Some(None);
    }
    if let Some(age) = directives.iter().find_map(|d| d.strip_prefix("max-age=")) {
        return Some(age.trim_matches('"').parse::<u64>().ok().map(|age| now.saturating_add(age)));
    }
    let expires = headers
        .get(reqwest::header::EXPIRES)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| httpdate::parse_http_date(v).ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    Some(expires)
}

/// The key a page is cached under: its address without the #fragment. Addresses carrying a user
/// name or password aren't cached.
pub fn key(url: &str) -> Option<String> {
    let mut url = url::Url::parse(url).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}

/// Pages from I2P are sensitive: they go to disk only with their own switch
pub fn is_i2p(url: &str, i2p_mode: bool) -> bool {
    i2p_mode || url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.ends_with(".i2p"))).unwrap_or(false)
}

#[derive(Default)]
struct Memory {
    pages: HashMap<String, Arc<CachedPage>>,
    /// Least recently used first
    order: VecDeque<String>,
    bytes: usize,
}

impl Memory {
    fn touch(&mut self, key: &str) {
        if let Some(i) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(i).expect("position is in range");
            self.order.push_back(key);
        }
    }

    fn insert(&mut self, key: String, entry: Arc<CachedPage>) {
        if let Some(old) = self.pages.insert(key.clone(), Arc::clone(&entry)) {
            self.bytes -= old.page.body.len();
            self.order.retain(|k| *k != key);
        }
        self.bytes += entry.page.body.len();
        self.order.push_back(key);
        while self.bytes > MEMORY_LIMIT {
            let Some(oldest) = self.order.pop_front() else { break };
            if let Some(old) = self.pages.remove(&oldest) {
                self.bytes -= old.page.body.len();
            }
        }
    }
}

/// Images' bytes kept in memory, least recently used first out
#[derive(Default)]
struct Bytes {
    files: HashMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    total: usize,
}

/// The cache, shared by every tab
pub struct Cache {
    memory: Mutex<Memory>,
    bytes: Mutex<Bytes>,
    /// The cache folder; None if the system has none
    dir: Option<PathBuf>,
    disk_clearweb: AtomicBool,
    disk_i2p: AtomicBool,
    disk_limit: AtomicU64,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(None)
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache").field("dir", &self.dir).finish()
    }
}

impl Cache {
    /// A cache whose disk entries (once turned on) go in `dir`
    pub fn new(dir: Option<PathBuf>) -> Cache {
        Cache {
            memory: Mutex::default(),
            bytes: Mutex::default(),
            dir,
            disk_clearweb: AtomicBool::new(false),
            disk_i2p: AtomicBool::new(false),
            disk_limit: AtomicU64::new(DEFAULT_DISK_MB * 1024 * 1024),
        }
    }

    /// Turn the disk cache on or off, for clearweb and I2P pages, with a size limit in megabytes
    pub fn configure(&self, clearweb: bool, i2p: bool, limit_mb: u64) {
        self.disk_clearweb.store(clearweb, Ordering::Relaxed);
        self.disk_i2p.store(i2p, Ordering::Relaxed);
        self.disk_limit.store(limit_mb.max(1) * 1024 * 1024, Ordering::Relaxed);
    }

    fn disk_dir(&self, i2p: bool) -> Option<&Path> {
        let on = if i2p { &self.disk_i2p } else { &self.disk_clearweb };
        self.dir.as_deref().filter(|_| on.load(Ordering::Relaxed))
    }

    fn file_for(dir: &Path, key: &str) -> PathBuf {
        let hash: String = ring::digest::digest(&ring::digest::SHA256, key.as_bytes())
            .as_ref()
            .iter()
            .take(16)
            .map(|b| format!("{:02x}", b))
            .collect();
        dir.join(format!("{}.gz", hash))
    }

    /// The cached copy of `url`, from memory or (if allowed for it) disk
    pub fn get(&self, url: &str, i2p: bool) -> Option<Arc<CachedPage>> {
        let key = key(url)?;
        {
            let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = memory.pages.get(&key).cloned() {
                memory.touch(&key);
                return Some(entry);
            }
        }
        let entry = Arc::new(read_entry(&Self::file_for(self.disk_dir(i2p)?, &key))?);
        self.memory.lock().unwrap_or_else(|e| e.into_inner()).insert(key, Arc::clone(&entry));
        Some(entry)
    }

    /// Keep `entry` for `url` (the address asked for; it's also kept under the page's own address
    /// if a redirect led elsewhere)
    pub fn put(&self, url: &str, entry: CachedPage, i2p: bool) {
        let mut keys: Vec<String> = [key(url), key(&entry.page.url)].into_iter().flatten().collect();
        keys.dedup();
        if keys.is_empty() {
            return;
        }
        let entry = Arc::new(entry);
        {
            let mut memory = self.memory.lock().unwrap_or_else(|e| e.into_inner());
            for key in &keys {
                memory.insert(key.clone(), Arc::clone(&entry));
            }
        }
        if let Some(dir) = self.disk_dir(i2p) {
            let _ = std::fs::create_dir_all(dir);
            for key in keys {
                let _ = write_entry(&Self::file_for(dir, &key), &entry);
            }
            self.trim_disk(dir);
        }
    }

    /// A cached file (an image) for `url`, from memory or (if allowed for it) disk
    pub fn get_bytes(&self, url: &str, i2p: bool) -> Option<Arc<Vec<u8>>> {
        let key = key(url)?;
        if let Some(found) = self.bytes.lock().unwrap_or_else(|e| e.into_inner()).files.get(&key).cloned() {
            return Some(found);
        }
        let path = Self::file_for(self.disk_dir(i2p)?, &key).with_extension("img");
        let mut data = Vec::new();
        std::fs::File::open(path).ok()?.take(MAX_FILE_BYTES).read_to_end(&mut data).ok()?;
        let data = Arc::new(data);
        self.keep_bytes(key, Arc::clone(&data));
        Some(data)
    }

    /// Keep a file (an image) for `url`
    pub fn put_bytes(&self, url: &str, data: Arc<Vec<u8>>, i2p: bool) {
        let Some(key) = key(url) else { return };
        if let Some(dir) = self.disk_dir(i2p) {
            let _ = std::fs::create_dir_all(dir);
            let path = Self::file_for(dir, &key).with_extension("img");
            let temporary = path.with_extension("img.tmp");
            if std::fs::write(&temporary, data.as_slice()).and_then(|_| std::fs::rename(&temporary, &path)).is_ok() {
                self.trim_disk(dir);
            }
        }
        self.keep_bytes(key, data);
    }

    fn keep_bytes(&self, key: String, data: Arc<Vec<u8>>) {
        let mut bytes = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = bytes.files.insert(key.clone(), Arc::clone(&data)) {
            bytes.total -= old.len();
            bytes.order.retain(|k| *k != key);
        }
        bytes.total += data.len();
        bytes.order.push_back(key);
        while bytes.total > MEMORY_LIMIT {
            let Some(oldest) = bytes.order.pop_front() else { break };
            if let Some(old) = bytes.files.remove(&oldest) {
                bytes.total -= old.len();
            }
        }
    }

    /// Forget every cached page and file, in memory and on disk
    pub fn clear(&self) -> Result<(), String> {
        *self.memory.lock().unwrap_or_else(|e| e.into_inner()) = Memory::default();
        *self.bytes.lock().unwrap_or_else(|e| e.into_inner()) = Bytes::default();
        let Some(dir) = &self.dir else { return Ok(()) };
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            if entry.path().extension().is_some_and(|e| e == "gz" || e == "img") {
                std::fs::remove_file(entry.path()).map_err(|e| format!("Couldn't clear the cache: {}", e))?;
            }
        }
        Ok(())
    }

    /// Bytes used on disk
    pub fn disk_usage(&self) -> u64 {
        self.dir.as_deref().map(|dir| entries(dir).iter().map(|(_, size, _)| size).sum()).unwrap_or(0)
    }

    /// Remove the oldest entries until the cache is within its limit
    fn trim_disk(&self, dir: &Path) {
        let limit = self.disk_limit.load(Ordering::Relaxed);
        let mut files = entries(dir);
        let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
        if total <= limit {
            return;
        }
        files.sort_by_key(|(_, _, modified)| *modified);
        for (path, size, _) in files {
            // Down to 90%, so the next few pages don't each trigger a scan
            if total <= limit / 10 * 9 {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
            }
        }
    }
}

/// The cache folder's entries: path, size and when written
fn entries(dir: &Path) -> Vec<(PathBuf, u64, SystemTime)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "gz" || x == "img"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.path(), meta.len(), meta.modified().unwrap_or(UNIX_EPOCH)))
        })
        .collect()
}

/// Write an entry atomically: the header line, then the page, gzip-compressed
pub(crate) fn write_entry(path: &Path, entry: &CachedPage) -> std::io::Result<()> {
    // The metadata line: one JSON object
    let header = serde_json::json!({
        "url": entry.page.url,
        "title": entry.page.title,
        "plain_text": entry.page.kind == PageKind::PlainText,
        "stored": entry.stored,
        "fresh_until": entry.fresh_until,
        "etag": entry.etag,
        "last_modified": entry.last_modified,
    });
    let temporary = path.with_extension("gz.tmp");
    let file = std::fs::File::create(&temporary)?;
    let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    gz.write_all(header.to_string().as_bytes())?;
    gz.write_all(b"\n")?;
    gz.write_all(entry.page.body.as_bytes())?;
    gz.finish()?;
    std::fs::rename(&temporary, path)
}

/// Read an entry back; None if it's missing or damaged. Decompression is limited, so a damaged or
/// planted file can't expand without bound.
pub(crate) fn read_entry(path: &Path) -> Option<CachedPage> {
    let file = std::fs::File::open(path).ok()?;
    let limit = crate::constants::MAX_PAGE_SIZE_BYTES + 64 * 1024;
    let mut text = String::new();
    flate2::read::GzDecoder::new(file).take(limit).read_to_string(&mut text).ok()?;
    let (header, body) = text.split_once('\n')?;
    let header: serde_json::Value = serde_json::from_str(header).ok()?;
    let text = |name: &str| header.get(name).and_then(|v| v.as_str()).map(str::to_string);
    Some(CachedPage {
        page: Page {
            title: text("title").unwrap_or_default(),
            body: body.to_string(),
            url: text("url")?,
            kind: if header.get("plain_text").and_then(|v| v.as_bool()) == Some(true) {
                PageKind::PlainText
            } else {
                PageKind::Html
            },
        },
        stored: header.get("stored").and_then(|v| v.as_u64()).unwrap_or(0),
        fresh_until: header.get("fresh_until").and_then(|v| v.as_u64()),
        etag: text("etag"),
        last_modified: text("last_modified"),
    })
}
