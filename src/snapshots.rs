//! Snapshots: dated copies of the pages of sites the user chooses to track, so a site can be seen
//! as it was on an earlier visit.
//!
//! Only tracked sites get snapshots (T tracks the current page's site), and a visit stores one only
//! when the page changed since the last. Snapshots are kept in the data folder, one folder per site
//! and page, as gzip files named by time and a hash of the page, so an unchanged page is spotted
//! without reading anything back. The tracked sites are listed in tracked_sites.txt.

use crate::cache::{CachedPage, read_entry, write_entry};
use crate::network::Page;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const TRACKED_FILE: &str = "tracked_sites.txt";

/// The list of tracked sites and their pages
pub const SNAPSHOTS_PAGE: &str = "about:snapshots";
/// One page's snapshots: about:snapshots?url=<address>
pub const PAGE_SNAPSHOTS: &str = "about:snapshots?url=";
/// One snapshot: about:snapshot?at=<time>&url=<address>
pub const SNAPSHOT: &str = "about:snapshot?";

/// Most snapshots kept of one page; the oldest go first
const MAX_PER_PAGE: usize = 200;

#[derive(Debug, Default)]
pub struct Snapshots {
    /// Where snapshots are stored; None keeps none (no data folder, or tests)
    dir: Option<PathBuf>,
    /// Where the tracked sites are listed; None keeps the list in memory
    list: Option<PathBuf>,
    tracked: Mutex<BTreeSet<String>>,
}

/// The site a page belongs to: its host, lowercase
pub fn site_of(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https" | "gemini" | "gopher") {
        return None;
    }
    url.host_str().map(|h| h.to_ascii_lowercase())
}

/// A folder name for `text` that can't climb out of the snapshots folder or clash with others
fn folder_name(text: &str) -> String {
    let safe: String =
        text.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    let safe = safe.trim_start_matches('.').to_string();
    if safe.is_empty() { "_".to_string() } else { safe }
}

fn hash(text: &str, length: usize) -> String {
    ring::digest::digest(&ring::digest::SHA256, text.as_bytes())
        .as_ref()
        .iter()
        .take(length)
        .map(|b| format!("{:02x}", b))
        .collect()
}

impl Snapshots {
    /// Snapshots stored in `dir`, for the sites listed in `list`
    pub fn load(dir: Option<PathBuf>, list: Option<PathBuf>) -> Snapshots {
        let tracked = list
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        Snapshots { dir, list, tracked: Mutex::new(tracked) }
    }

    pub fn is_tracked(&self, url: &str) -> bool {
        site_of(url).is_some_and(|site| self.tracked.lock().unwrap_or_else(|e| e.into_inner()).contains(&site))
    }

    pub fn tracked_sites(&self) -> Vec<String> {
        self.tracked.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }

    /// Start or stop tracking the site of `url`. Returns whether it's now tracked. Stopping keeps
    /// the snapshots already taken.
    pub fn toggle(&self, url: &str) -> Result<bool, String> {
        let site = site_of(url).ok_or("Only web, Gemini and Gopher pages can be tracked")?;
        let mut tracked = self.tracked.lock().unwrap_or_else(|e| e.into_inner());
        let now_tracked = if tracked.remove(&site) {
            false
        } else {
            tracked.insert(site);
            true
        };
        if let Some(path) = &self.list {
            let text: String = tracked.iter().map(|s| format!("{}\n", s)).collect();
            let temporary = path.with_extension("txt.tmp");
            if let Some(folder) = path.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            std::fs::write(&temporary, text)
                .and_then(|_| std::fs::rename(&temporary, path))
                .map_err(|e| format!("Couldn't save the tracked sites: {}", e))?;
        }
        Ok(now_tracked)
    }

    fn page_folder(&self, url: &str) -> Option<PathBuf> {
        let key = crate::cache::key(url)?;
        let site = site_of(&key)?;
        Some(self.dir.as_ref()?.join(folder_name(&site)).join(hash(&key, 12)))
    }

    /// The snapshot files of a page: (time, content hash, path), newest first
    fn files(folder: &Path) -> Vec<(u64, String, PathBuf)> {
        let Ok(read) = std::fs::read_dir(folder) else { return Vec::new() };
        let mut files: Vec<(u64, String, PathBuf)> = read
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let (time, content) = name.strip_suffix(".gz")?.split_once('-')?;
                Some((time.parse().ok()?, content.to_string(), e.path()))
            })
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        files
    }

    /// Keep a snapshot of `page`, loaded from `url`, if its site is tracked and the page changed
    /// since the last snapshot. Returns the snapshot's time when one was taken.
    pub fn record(&self, url: &str, page: &Page, now: u64) -> Option<u64> {
        if !self.is_tracked(url) {
            return None;
        }
        let folder = self.page_folder(url)?;
        let content = hash(&page.body, 8);
        let files = Self::files(&folder);
        if files.first().is_some_and(|(_, latest, _)| *latest == content) {
            return None;
        }
        std::fs::create_dir_all(&folder).ok()?;
        // The page's address, for listing a site's pages
        std::fs::write(folder.join("address.txt"), crate::cache::key(url)?).ok()?;
        let entry = CachedPage { page: page.clone(), stored: now, fresh_until: None, etag: None, last_modified: None };
        write_entry(&folder.join(format!("{}-{}.gz", now, content)), &entry).ok()?;
        for (_, _, old) in files.iter().skip(MAX_PER_PAGE - 1) {
            let _ = std::fs::remove_file(old);
        }
        Some(now)
    }

    /// The times of a page's snapshots, newest first
    pub fn times(&self, url: &str) -> Vec<u64> {
        self.page_folder(url).map(|f| Self::files(&f).into_iter().map(|(t, ..)| t).collect()).unwrap_or_default()
    }

    /// The snapshot of `url` taken at `time`
    pub fn open(&self, url: &str, time: u64) -> Option<CachedPage> {
        let folder = self.page_folder(url)?;
        let (_, _, path) = Self::files(&folder).into_iter().find(|(t, ..)| *t == time)?;
        read_entry(&path)
    }

    /// A site's pages that have snapshots: (address, how many, newest time), newest first
    pub fn site_pages(&self, site: &str) -> Vec<(String, usize, u64)> {
        let Some(dir) = &self.dir else { return Vec::new() };
        let Ok(read) = std::fs::read_dir(dir.join(folder_name(site))) else { return Vec::new() };
        let mut pages: Vec<(String, usize, u64)> = read
            .flatten()
            .filter_map(|e| {
                let address = std::fs::read_to_string(e.path().join("address.txt")).ok()?;
                let files = Self::files(&e.path());
                let newest = files.first()?.0;
                Some((address.trim().to_string(), files.len(), newest))
            })
            .collect();
        pages.sort_by_key(|p| std::cmp::Reverse(p.2));
        pages
    }

    /// Delete every snapshot of a site
    pub fn delete_site(&self, site: &str) -> Result<(), String> {
        let Some(dir) = &self.dir else { return Ok(()) };
        match std::fs::remove_dir_all(dir.join(folder_name(site))) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Couldn't delete the snapshots: {}", e)),
            _ => Ok(()),
        }
    }
}

/// The address of the snapshot of `url` taken at `time`
pub fn snapshot_address(url: &str, time: u64) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
    format!("{}at={}&url={}", SNAPSHOT, time, encoded)
}

/// The address listing `url`'s snapshots
pub fn page_snapshots_address(url: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
    format!("{}{}", PAGE_SNAPSHOTS, encoded)
}

/// The page address and time in a snapshot address
pub fn parse_snapshot_address(address: &str) -> Option<(String, u64)> {
    let query = address.strip_prefix(SNAPSHOT)?;
    let mut url = None;
    let mut at = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "url" => url = Some(value.into_owned()),
            "at" => at = value.parse().ok(),
            _ => {}
        }
    }
    Some((url?, at?))
}

/// The page address in a page-snapshots address
pub fn parse_page_snapshots_address(address: &str) -> Option<String> {
    let query = address.strip_prefix("about:snapshots?")?;
    url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "url").map(|(_, v)| v.into_owned())
}
