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
//!
//! Saved cookies, and the list of sites (which says where you're signed in), are encrypted
//! (ChaCha20-Poly1305) with a random key kept in the system keychain (macOS Keychain, the
//! Secret Service on Linux, Windows' Credential Manager), so a copy of the settings folder (a
//! backup, a synced folder) doesn't carry your sign-ins. Without a keychain they're saved as
//! plain files, readable only by you, and the cookies page says so.

use reqwest::header::HeaderValue;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};
use url::Url;

pub const COOKIES_FILE: &str = "cookies.json";
pub const SITES_FILE: &str = "cookie_sites.txt";
/// The sites and cookies, encrypted
pub const ENCRYPTED_FILE: &str = "cookies.enc";
const MAGIC: &[u8] = b"RYNXCK1\0";
/// Bound to the ciphertext, so a file encrypted for something else can't stand in
const ASSOCIATED: &[u8] = b"rynx saved cookies v1";
/// The keychain entry the key is kept in
pub const KEYCHAIN_SERVICE: &str = "rynx-browser";
const KEYCHAIN_ACCOUNT: &str = "cookie encryption key";
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
    /// The key they're encrypted with, if the keychain gave one
    key: RwLock<Option<[u8; 32]>>,
    /// How they're kept, for the cookies page
    note: RwLock<String>,
}

impl Default for Jar {
    fn default() -> Self {
        Jar {
            anubis: crate::anubis::Cookies::default(),
            store: Mutex::new(cookie_store::CookieStore::new()),
            sites: RwLock::new(BTreeSet::new()),
            folder: RwLock::new(None),
            key: RwLock::new(None),
            note: RwLock::new(String::new()),
        }
    }
}

impl Jar {
    /// Read the allowed sites and their saved cookies from `folder`, and save there from now on,
    /// encrypted with `key` (Ok) or, if the keychain couldn't give one (Err: why), as plain files
    pub fn load(&self, folder: &Path, key: Result<[u8; 32], String>) {
        let encrypted = folder.join(ENCRYPTED_FILE);
        let plain_sites = folder.join(SITES_FILE);
        let (sites, store, note, migrate) = match &key {
            Ok(key) if encrypted.exists() => {
                match std::fs::read(&encrypted).map_err(|e| e.to_string()).and_then(|data| decrypt(key, &data)) {
                    Ok((sites, store)) => {
                        (sites, store, String::from("encrypted, with a key in the system keychain"), false)
                    }
                    Err(why) => (
                        BTreeSet::new(),
                        None,
                        format!(
                            "saved cookies couldn't be read ({}), so rynx started without them; signing in again replaces them",
                            why
                        ),
                        false,
                    ),
                }
            }
            // Plain files from before the key existed are encrypted now, and removed
            Ok(_) => {
                let (sites, store) = read_plain(folder);
                let migrate = plain_sites.exists();
                (sites, store, String::from("encrypted, with a key in the system keychain"), migrate)
            }
            Err(why) if encrypted.exists() => (
                BTreeSet::new(),
                None,
                format!(
                    "saved cookies are encrypted with a key in the system keychain, which isn't available now ({}); rynx started without them",
                    why
                ),
                false,
            ),
            Err(why) => {
                let (sites, store) = read_plain(folder);
                (
                    sites,
                    store,
                    format!(
                        "not encrypted, since the system keychain isn't available ({}); saved readable only by you",
                        why
                    ),
                    false,
                )
            }
        };
        let mut store = store.unwrap_or_default();
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
        *self.sites.write().unwrap_or_else(|e| e.into_inner()) = sites;
        *self.key.write().unwrap_or_else(|e| e.into_inner()) = key.ok();
        *self.note.write().unwrap_or_else(|e| e.into_inner()) = note;
        // An encrypted file that couldn't be read is left alone until there's something to save
        *self.folder.write().unwrap_or_else(|e| e.into_inner()) = Some(folder.to_path_buf());
        if migrate {
            let _ = self.save();
        }
    }

    /// How saved cookies are kept (encrypted or not, and why), for the cookies page
    pub fn storage_note(&self) -> String {
        if self.is_saved() {
            self.note.read().unwrap_or_else(|e| e.into_inner()).clone()
        } else {
            String::from("kept in memory only, never saved")
        }
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

    /// Write the allowed sites and their lasting cookies, encrypted if there's a key, and
    /// readable only by you either way
    fn save(&self) -> Result<(), String> {
        let Some(folder) = self.folder.read().unwrap_or_else(|e| e.into_inner()).clone() else { return Ok(()) };
        let sites: Vec<String> = self.sites.read().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
        let mut cookies = Vec::new();
        cookie_store::serde::json::save(&self.store.lock().unwrap_or_else(|e| e.into_inner()), &mut cookies)
            .map_err(|e| format!("Couldn't save cookies: {}", e))?;
        if let Some(key) = *self.key.read().unwrap_or_else(|e| e.into_inner()) {
            let contents = serde_json::json!({ "sites": sites, "cookies": String::from_utf8_lossy(&cookies) });
            write_private(&folder.join(ENCRYPTED_FILE), &encrypt(&key, contents.to_string().as_bytes())?)?;
            for plain in [SITES_FILE, COOKIES_FILE] {
                let _ = std::fs::remove_file(folder.join(plain));
            }
            return Ok(());
        }
        let mut list = String::from("# Sites allowed to keep cookies (C on one of their pages, or about:cookies)\n");
        for site in sites {
            list.push_str(&site);
            list.push('\n');
        }
        write_private(&folder.join(SITES_FILE), list.as_bytes())?;
        write_private(&folder.join(COOKIES_FILE), &cookies)
    }
}

/// The sites and cookies from the plain files
fn read_plain(folder: &Path) -> (BTreeSet<String>, Option<cookie_store::CookieStore>) {
    let sites = std::fs::read_to_string(folder.join(SITES_FILE))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let store = std::fs::File::open(folder.join(COOKIES_FILE))
        .ok()
        .and_then(|file| cookie_store::serde::json::load(std::io::BufReader::new(file)).ok());
    (sites, store)
}

fn cipher(key: &[u8; 32]) -> ring::aead::LessSafeKey {
    ring::aead::LessSafeKey::new(
        ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, key).expect("a 32-byte key"),
    )
}

/// The magic, a random nonce, then the sealed contents
pub fn encrypt(key: &[u8; 32], contents: &[u8]) -> Result<Vec<u8>, String> {
    use ring::rand::SecureRandom;
    let mut nonce = [0u8; 12];
    ring::rand::SystemRandom::new().fill(&mut nonce).map_err(|_| "no randomness for encryption".to_string())?;
    let mut sealed = contents.to_vec();
    cipher(key)
        .seal_in_place_append_tag(
            ring::aead::Nonce::assume_unique_for_key(nonce),
            ring::aead::Aad::from(ASSOCIATED),
            &mut sealed,
        )
        .map_err(|_| "couldn't encrypt cookies".to_string())?;
    Ok([MAGIC, &nonce, &sealed].concat())
}

/// The sites and cookies from an encrypted file
fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<(BTreeSet<String>, Option<cookie_store::CookieStore>), String> {
    let rest = data.strip_prefix(MAGIC).ok_or("not a rynx cookies file")?;
    if rest.len() < 12 {
        return Err("the file is cut short".to_string());
    }
    let (nonce, sealed) = rest.split_at(12);
    let mut sealed = sealed.to_vec();
    let nonce = ring::aead::Nonce::try_assume_unique_for_key(nonce).map_err(|_| "a bad nonce")?;
    let plain = cipher(key)
        .open_in_place(nonce, ring::aead::Aad::from(ASSOCIATED), &mut sealed)
        .map_err(|_| "the keychain's key doesn't open it, or it was changed")?;
    let contents: serde_json::Value = serde_json::from_slice(plain).map_err(|e| e.to_string())?;
    let sites =
        contents["sites"].as_array().into_iter().flatten().filter_map(|s| s.as_str()).map(str::to_string).collect();
    let store = contents["cookies"].as_str().and_then(|json| cookie_store::serde::json::load(json.as_bytes()).ok());
    Ok((sites, store))
}

/// The key saved cookies are encrypted with, from the system keychain under `service`, made the
/// first time. Asked for once at startup.
pub fn keychain_key(service: &str) -> Result<[u8; 32], String> {
    let entry = keyring::Entry::new(service, KEYCHAIN_ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_secret() {
        Ok(secret) => secret.try_into().map_err(|_| "the keychain's cookie key isn't 32 bytes".to_string()),
        Err(keyring::Error::NoEntry) => {
            use ring::rand::SecureRandom;
            let mut key = [0u8; 32];
            ring::rand::SystemRandom::new().fill(&mut key).map_err(|_| "no randomness for a key".to_string())?;
            entry.set_secret(&key).map_err(|e| e.to_string())?;
            Ok(key)
        }
        Err(e) => Err(e.to_string()),
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
