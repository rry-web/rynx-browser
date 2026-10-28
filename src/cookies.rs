//! Cookies: none, except for the sites you allow (C on one of their pages), so you can stay
//! signed in to them. Anubis's bot-check passes are always kept, as before.
//!
//! A site is what's registered under a public suffix ("example.com" for accounts.example.com
//! and www.example.com, but "alice.github.io" and "bob.github.io" apart), so a site's sign-in
//! pages and its other pages share them. Cookies are only taken from, and only sent to, allowed
//! sites, and a site can't set a cookie for a whole public suffix.
//!
//! Clearweb cookies meant to outlast the session (with an expiry) are saved in the settings
//! folder, readable only by you; others end when rynx quits, as in other browsers. I2P sites are
//! allowed for the session only and their cookies are never written to disk, so there's no
//! record of them there.

use reqwest::header::HeaderValue;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use url::Url;

pub const COOKIES_FILE: &str = "cookies.json";
pub const SITES_FILE: &str = "cookie_sites.txt";
/// The page listing the sites allowed cookies
pub const COOKIES_PAGE: &str = "about:cookies";

/// Most cookies kept in all, and per site
const MAX_COOKIES: usize = 3000;
const MAX_PER_SITE: usize = 300;

/// The site a host belongs to: its registrable domain, or the host itself (an address, or a name
/// with no known public suffix, like a .i2p or local name)
pub fn site_of(host: &str) -> String {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') || host.ends_with(".i2p") {
        return host;
    }
    match psl::domain_str(&host) {
        Some(domain) => domain.to_string(),
        None => host,
    }
}

/// The cookies of one network (clearweb or I2P)
#[derive(Debug)]
pub struct Jar {
    anubis: crate::anubis::Cookies,
    store: Mutex<cookie_store::CookieStore>,
    sites: RwLock<BTreeSet<String>>,
    /// Where they're saved; None keeps them in memory only
    folder: RwLock<Option<PathBuf>>,
}

impl Default for Jar {
    fn default() -> Self {
        Jar {
            anubis: crate::anubis::Cookies::default(),
            store: Mutex::new(cookie_store::CookieStore::new()),
            sites: RwLock::new(BTreeSet::new()),
            folder: RwLock::new(None),
        }
    }
}

impl Jar {
    /// Read the allowed sites and their saved cookies from `folder`, and save there from now on
    pub fn load(&self, folder: &Path) {
        let sites: BTreeSet<String> = std::fs::read_to_string(folder.join(SITES_FILE))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        if let Ok(file) = std::fs::File::open(folder.join(COOKIES_FILE))
            && let Ok(mut store) = cookie_store::serde::json::load(std::io::BufReader::new(file))
        {
            // Only the allowed sites' (the file may be older than a change to them)
            let stale: Vec<(String, String, String)> = store
                .iter_any()
                .filter(|c| !c.domain.as_cow().is_some_and(|d| sites.contains(&site_of(&d))))
                .map(|c| (c.domain.as_cow().unwrap_or_default().into_owned(), c.path.to_string(), c.name().to_string()))
                .collect();
            for (domain, path, name) in stale {
                store.remove(&domain, &path, &name);
            }
            *self.store.lock().unwrap_or_else(|e| e.into_inner()) = store;
        }
        *self.sites.write().unwrap_or_else(|e| e.into_inner()) = sites;
        *self.folder.write().unwrap_or_else(|e| e.into_inner()) = Some(folder.to_path_buf());
    }

    /// Whether `host`'s site may keep cookies
    pub fn allowed(&self, host: &str) -> bool {
        self.sites.read().unwrap_or_else(|e| e.into_inner()).contains(&site_of(host))
    }

    /// Let `host`'s site keep cookies. Returns the site.
    pub fn allow(&self, host: &str) -> Result<String, String> {
        let site = site_of(host);
        self.sites.write().unwrap_or_else(|e| e.into_inner()).insert(site.clone());
        self.save()?;
        Ok(site)
    }

    /// Stop `site` keeping cookies, and forget the ones it has
    pub fn forget(&self, site: &str) -> Result<(), String> {
        let site = site_of(site);
        self.sites.write().unwrap_or_else(|e| e.into_inner()).remove(&site);
        {
            let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let theirs: Vec<(String, String, String)> = store
                .iter_any()
                .filter(|c| c.domain.as_cow().is_some_and(|d| site_of(&d) == site))
                .map(|c| (c.domain.as_cow().unwrap_or_default().into_owned(), c.path.to_string(), c.name().to_string()))
                .collect();
            for (domain, path, name) in theirs {
                store.remove(&domain, &path, &name);
            }
        }
        self.save()
    }

    /// The allowed sites, each with how many cookies it has now
    pub fn sites(&self) -> Vec<(String, usize)> {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let sites = self.sites.read().unwrap_or_else(|e| e.into_inner());
        sites
            .iter()
            .map(|site| {
                let count =
                    store.iter_unexpired().filter(|c| c.domain.as_cow().is_some_and(|d| site_of(&d) == *site)).count();
                (site.clone(), count)
            })
            .collect()
    }

    /// Whether these cookies are saved to disk (the clearweb's) or kept in memory (I2P's)
    pub fn is_saved(&self) -> bool {
        self.folder.read().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// Write the allowed sites and their lasting cookies, readable only by you
    fn save(&self) -> Result<(), String> {
        let Some(folder) = self.folder.read().unwrap_or_else(|e| e.into_inner()).clone() else { return Ok(()) };
        let mut sites = String::from("# Sites allowed to keep cookies (C on one of their pages, or about:cookies)\n");
        for site in self.sites.read().unwrap_or_else(|e| e.into_inner()).iter() {
            sites.push_str(site);
            sites.push('\n');
        }
        let mut cookies = Vec::new();
        cookie_store::serde::json::save(&self.store.lock().unwrap_or_else(|e| e.into_inner()), &mut cookies)
            .map_err(|e| format!("Couldn't save cookies: {}", e))?;
        write_private(&folder.join(SITES_FILE), sites.as_bytes())?;
        write_private(&folder.join(COOKIES_FILE), &cookies)
    }
}

/// Write a file only its owner can read, atomically
fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options
        .open(&temporary)
        .and_then(|mut file| file.write_all(data))
        .and_then(|_| std::fs::rename(&temporary, path))
        .map_err(|e| format!("Couldn't save {}: {}", path.display(), e))
}

impl reqwest::cookie::CookieStore for Jar {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let headers: Vec<&HeaderValue> = headers.collect();
        // Anubis's passes, whatever the site
        self.anubis.set_cookies(&mut headers.iter().copied(), url);
        let Some(host) = url.host_str() else { return };
        if !self.allowed(host) {
            return;
        }
        let site = site_of(host);
        let mut lasting = false;
        {
            let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            for header in headers {
                let Ok(text) = header.to_str() else { continue };
                let Ok(cookie) = cookie_store::RawCookie::parse(text.to_string()) else { continue };
                // Not for a whole public suffix (co.uk, github.io), only for the site itself
                if cookie.domain().is_some_and(|d| site_of(d.trim_start_matches('.')) != site) {
                    continue;
                }
                let here = store.iter_any().filter(|c| c.domain.as_cow().is_some_and(|d| site_of(&d) == site)).count();
                let total = store.iter_any().count();
                let replacing = store.contains_any(
                    cookie.domain().map(|d| d.trim_start_matches('.')).unwrap_or(host),
                    cookie.path().unwrap_or("/"),
                    cookie.name(),
                );
                if !replacing && (here >= MAX_PER_SITE || total >= MAX_COOKIES) {
                    continue;
                }
                lasting |= cookie.max_age().is_some() || cookie.expires().is_some();
                let _ = store.insert_raw(&cookie, url);
            }
        }
        if lasting {
            let _ = self.save();
        }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let mut values: Vec<String> = Vec::new();
        if let Some(anubis) = self.anubis.cookies(url).and_then(|v| v.to_str().ok().map(str::to_string)) {
            values.push(anubis);
        }
        if url.host_str().is_some_and(|host| self.allowed(host)) {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            values.extend(store.get_request_values(url).map(|(name, value)| format!("{}={}", name, value)));
        }
        if values.is_empty() {
            return None;
        }
        HeaderValue::from_str(&values.join("; ")).ok()
    }
}
