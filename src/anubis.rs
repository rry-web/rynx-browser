//! Anubis bot checks, passed without JavaScript.
//!
//! Anubis (https://anubis.techaro.lol) asks each visitor's browser for a small proof of work
//! before showing a site: find a number that makes a hash start with enough zeros. Its page does
//! that in JavaScript; rynx does the same work in Rust and sends the answer to the same address,
//! so the site gets exactly what it asked for. The pass is a cookie, which only this module's
//! cookie store keeps (in memory, for Anubis's cookies alone): rynx otherwise has no cookies.
//!
//! Supported methods: fast/slow (SHA-256, difficulty in hex digits), sha256 and argon2id
//! (difficulty in bits), and metarefresh (wait, then follow a link). hashx isn't supported.

use reqwest::header::HeaderValue;
use scraper::{Html, Selector};
use serde_json::Value;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use url::Url;

/// Where Anubis serves its own pages, under the site's base prefix
const ANUBIS_PATH: &str = "/.within.website/x/cmd/anubis/";

/// Longest the proof of work may take before giving up
const WORK_TIME_LIMIT: Duration = Duration::from_secs(60);

/// A challenge found on a page
#[derive(Clone, Debug, PartialEq)]
pub struct Challenge {
    /// Empty for Anubis versions before challenges had ids
    pub id: String,
    pub random_data: String,
    pub algorithm: String,
    pub difficulty: u32,
    pub base_prefix: String,
    pub public_url: String,
    /// Stylesheets the page imports from Anubis (the css-load extension checks they were fetched)
    pub stylesheets: Vec<String>,
}

/// The challenge on an Anubis "making sure you're not a bot" page, if `html` is one
pub fn detect(html: &str) -> Option<Challenge> {
    // Most pages aren't challenges; don't parse them twice
    if !html.contains("anubis_challenge") {
        return None;
    }
    let document = Html::parse_document(html);
    let json = |id: &str| -> Option<Value> {
        let selector = Selector::parse(&format!("script#{}", id)).ok()?;
        let text: String = document.select(&selector).next()?.text().collect();
        serde_json::from_str(&text).ok()
    };
    let data = json("anubis_challenge")?;
    let rules = data.get("rules")?;
    let (id, random_data) = match data.get("challenge")? {
        // Older versions sent the random data alone
        Value::String(random) => (String::new(), random.clone()),
        challenge => (
            challenge.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            challenge.get("randomData").and_then(Value::as_str)?.to_string(),
        ),
    };
    let text = |value: Option<Value>| value.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let stylesheets = Selector::parse("style")
        .map(|style| {
            document
                .select(&style)
                .flat_map(|s| imports(&s.text().collect::<String>()))
                .filter(|path| path.contains(ANUBIS_PATH))
                .collect()
        })
        .unwrap_or_default();
    Some(Challenge {
        id,
        random_data,
        algorithm: rules.get("algorithm").and_then(Value::as_str).unwrap_or("fast").to_string(),
        difficulty: rules.get("difficulty").and_then(Value::as_u64).unwrap_or(0).min(64) as u32,
        base_prefix: text(json("anubis_base_prefix")),
        public_url: text(json("anubis_public_url")),
        stylesheets,
    })
}

/// The addresses in a stylesheet's `@import url("...")` rules
fn imports(css: &str) -> Vec<String> {
    css.split("@import")
        .skip(1)
        .filter_map(|rule| {
            let rule = rule.trim_start().strip_prefix("url(")?;
            let end = rule.find(')')?;
            Some(rule[..end].trim().trim_matches(['"', '\'']).to_string())
        })
        .collect()
}

// --- The proof of work ---

/// The answer to a challenge: the hash found, in hex, and the nonce that produced it
#[derive(Debug, PartialEq)]
pub struct Answer {
    pub response: String,
    pub nonce: u64,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    for part in parts {
        context.update(part);
    }
    context.finish().as_ref().try_into().expect("SHA-256 is 32 bytes")
}

/// Whether `hash` starts with `bits` zero bits
fn leading_zero_bits(hash: &[u8], bits: u32) -> bool {
    let mut remaining = bits;
    for &byte in hash {
        match remaining {
            0 => return true,
            8.. => {
                if byte != 0 {
                    return false;
                }
                remaining -= 8;
            }
            _ => return byte & (0xFF << (8 - remaining)) == 0,
        }
    }
    remaining == 0
}

/// The hash a method gives for `nonce`, or None for an unsupported method
fn work(algorithm: &str, random_data: &str, data: &[u8], nonce: u64) -> Option<[u8; 32]> {
    // The WebAssembly methods encode the nonce in either byte order, picked by the challenge
    let little_endian = data.last().is_some_and(|&b| b >= 128);
    match algorithm {
        "fast" | "slow" => Some(sha256(&[random_data.as_bytes(), nonce.to_string().as_bytes()])),
        "sha256" => {
            let nonce = nonce as u32;
            let bytes = if little_endian { nonce.to_le_bytes() } else { nonce.to_be_bytes() };
            Some(sha256(&[data, &bytes]))
        }
        "argon2id" => {
            let bytes = if little_endian { nonce.to_le_bytes() } else { nonce.to_be_bytes() };
            let mut out = [0u8; 32];
            argon2::Argon2::default().hash_password_into(data, &bytes, &mut out).ok()?;
            Some(out)
        }
        _ => None,
    }
}

/// Whether a hash is good enough: `difficulty` leading hex zeros for fast/slow, bits otherwise
fn good_enough(algorithm: &str, hash: &[u8; 32], difficulty: u32) -> bool {
    match algorithm {
        "fast" | "slow" => leading_zero_bits(hash, difficulty * 4),
        _ => leading_zero_bits(hash, difficulty),
    }
}

/// Do the proof of work on every CPU core. Slow: run it on a blocking thread.
pub fn solve(challenge: &Challenge) -> Result<Answer, String> {
    let algorithm = challenge.algorithm.as_str();
    if !matches!(algorithm, "fast" | "slow" | "sha256" | "argon2id") {
        return Err(format!("this site's bot check uses \"{}\", which rynx can't do yet", algorithm));
    }
    // The WebAssembly methods hash the random data's bytes; fast/slow hash its text
    let data = if matches!(algorithm, "sha256" | "argon2id") {
        unhex(&challenge.random_data).ok_or("the bot check's data is malformed")?
    } else {
        Vec::new()
    };
    let workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as u64;
    let found: Mutex<Option<Answer>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    let started = Instant::now();
    std::thread::scope(|scope| {
        for worker in 0..workers {
            let (data, found, stop) = (&data, &found, &stop);
            scope.spawn(move || {
                let mut nonce = worker;
                // The sha256 method's nonce is 32 bits
                let limit = if algorithm == "sha256" { u32::MAX as u64 } else { u64::MAX };
                while nonce <= limit && !stop.load(Ordering::Relaxed) {
                    let Some(hash) = work(algorithm, &challenge.random_data, data, nonce) else { break };
                    if good_enough(algorithm, &hash, challenge.difficulty) {
                        *found.lock().unwrap_or_else(|e| e.into_inner()) = Some(Answer { response: hex(&hash), nonce });
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    if nonce % 1024 < workers && started.elapsed() > WORK_TIME_LIMIT {
                        stop.store(true, Ordering::Relaxed);
                    }
                    nonce += workers;
                }
            });
        }
    });
    found
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .ok_or_else(|| format!("this site's bot check (difficulty {}) took too long", challenge.difficulty))
}

// --- Passing the check ---

/// Where the check sends you back to once passed
fn redirect_target(page_url: &Url, challenge: &Challenge) -> String {
    if !challenge.public_url.is_empty()
        && page_url.as_str().starts_with(&challenge.public_url)
        && let Some((_, redir)) = page_url.query_pairs().find(|(k, _)| k == "redir")
    {
        return redir.into_owned();
    }
    page_url.to_string()
}

/// The address that hands in the answer
pub fn pass_address(page_url: &str, challenge: &Challenge, answer: Option<&Answer>, elapsed: Duration) -> Result<String, String> {
    let page = Url::parse(page_url).map_err(|_| "the bot check's page has no valid address".to_string())?;
    let base = format!("{}{}api/pass-challenge", challenge.base_prefix.trim_end_matches('/'), ANUBIS_PATH);
    let mut url = page.join(&base).map_err(|_| "the bot check's address is invalid".to_string())?;
    // Only on the same site: the answer goes nowhere else
    if url.origin() != page.origin() {
        return Err("the bot check points to another site".to_string());
    }
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        if !challenge.id.is_empty() {
            query.append_pair("id", &challenge.id);
        }
        match answer {
            Some(answer) => {
                query.append_pair("response", &answer.response);
                query.append_pair("nonce", &answer.nonce.to_string());
                query.append_pair("redir", &redirect_target(&page, challenge));
                query.append_pair("elapsedTime", &elapsed.as_millis().to_string());
            }
            // metarefresh hands back the random data after waiting
            None => {
                query.append_pair("redir", &redirect_target(&page, challenge));
                query.append_pair("challenge", &challenge.random_data);
            }
        }
    }
    Ok(url.to_string())
}

/// Pass the check on the page at `page_url`, returning the response that follows: normally the
/// page that was asked for
pub async fn pass(
    client: &reqwest::Client,
    page_url: &str,
    challenge: Challenge,
    i2p_mode: bool,
) -> Result<reqwest::Response, String> {
    let page = Url::parse(page_url).map_err(|_| "the bot check's page has no valid address".to_string())?;
    // The css-load extension checks the page's stylesheet was fetched, as any browser would
    for stylesheet in &challenge.stylesheets {
        if let Ok(url) = page.join(stylesheet)
            && url.origin() == page.origin()
            && let Ok(response) = client.get(url).send().await
        {
            let _ = response.bytes().await;
        }
    }
    let started = Instant::now();
    let answer = if challenge.algorithm == "metarefresh" {
        // The browser is asked to wait before coming back; difficulty is the number of seconds
        tokio::time::sleep(Duration::from_secs(u64::from(challenge.difficulty.min(30)) + 1)).await;
        None
    } else {
        let to_solve = challenge.clone();
        let answer = tokio::task::spawn_blocking(move || solve(&to_solve))
            .await
            .map_err(|_| "the bot check's work stopped unexpectedly".to_string())??;
        Some(answer)
    };
    let address = pass_address(page_url, &challenge, answer.as_ref(), started.elapsed())?;
    crate::network::get_following_redirects(client, &address, i2p_mode).await
}

// --- Cookies ---

#[derive(Clone, Debug)]
struct StoredCookie {
    /// The host it came from, or the domain it was set for
    domain: String,
    /// Sent only to `domain` itself, not its subdomains
    host_only: bool,
    secure: bool,
    name: String,
    value: String,
}

/// A cookie store that keeps only Anubis's cookies, in memory. Every other cookie a site sets is
/// dropped, so browsing stays cookie-free.
#[derive(Debug, Default)]
pub struct Cookies {
    cookies: Mutex<Vec<StoredCookie>>,
}

/// Most cookies kept, and the longest value
const MAX_COOKIES: usize = 64;
const MAX_VALUE: usize = 4096;

/// Whether a cookie belongs to Anubis: set by its own addresses, or named like its cookies (the
/// name prefix is configurable, but the endings aren't)
fn is_anubis_cookie(name: &str, url: &Url) -> bool {
    url.path().contains(ANUBIS_PATH)
        || name.to_ascii_lowercase().contains("anubis")
        || name.ends_with("-cookie-verification")
        || name.ends_with("-original-referer")
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

impl reqwest::cookie::CookieStore for Cookies {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else { return };
        let mut cookies = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        for header in headers {
            let Ok(header) = header.to_str() else { continue };
            let mut parts = header.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else { continue };
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() || value.len() > MAX_VALUE || !is_anubis_cookie(name, url) {
                continue;
            }
            let mut domain = None;
            let mut secure = false;
            let mut expired = value.is_empty();
            for attribute in parts {
                let (key, val) = attribute.split_once('=').unwrap_or((attribute, ""));
                match key.trim().to_ascii_lowercase().as_str() {
                    "domain" => domain = Some(val.trim().trim_start_matches('.').to_ascii_lowercase()),
                    "secure" => secure = true,
                    "max-age" => expired |= val.trim().parse::<i64>().is_ok_and(|age| age <= 0),
                    _ => {}
                }
            }
            // A site may only set cookies for itself or a parent domain of its own
            let (domain, host_only) = match domain.filter(|d| !d.is_empty()) {
                Some(d) if domain_matches(&host, &d) && d.contains('.') => (d, false),
                Some(_) => continue,
                None => (host.clone(), true),
            };
            cookies.retain(|c| !(c.name == name && c.domain == domain));
            if !expired {
                cookies.push(StoredCookie { domain, host_only, secure, name: name.to_string(), value: value.to_string() });
                if cookies.len() > MAX_COOKIES {
                    cookies.remove(0);
                }
            }
        }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let host = url.host_str()?.to_ascii_lowercase();
        let https = url.scheme() == "https";
        let cookies = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        let header: Vec<String> = cookies
            .iter()
            .filter(|c| if c.host_only { host == c.domain } else { domain_matches(&host, &c.domain) })
            .filter(|c| https || !c.secure)
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if header.is_empty() {
            return None;
        }
        HeaderValue::from_str(&header.join("; ")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::cookie::CookieStore;

    #[test]
    fn zero_bits_are_counted() {
        assert!(leading_zero_bits(&[0x00, 0x0f, 0xff], 12));
        assert!(!leading_zero_bits(&[0x00, 0x1f, 0xff], 12));
        assert!(leading_zero_bits(&[0xff], 0));
        assert!(!leading_zero_bits(&[0x00], 9), "more bits than the hash has");
    }

    #[test]
    fn imports_are_found() {
        assert_eq!(imports(r#"a{} @import url("/x/a.css"); @import url('/b');"#), ["/x/a.css", "/b"]);
    }

    #[test]
    fn only_anubis_cookies_are_kept() {
        let jar = Cookies::default();
        let page = Url::parse("https://www.site.example/fiction/1").unwrap();
        let pass = Url::parse("https://www.site.example/.within.website/x/cmd/anubis/api/pass-challenge").unwrap();
        let set = |url: &Url, headers: &[&str]| {
            let values: Vec<HeaderValue> = headers.iter().map(|h| HeaderValue::from_str(h).unwrap()).collect();
            jar.set_cookies(&mut values.iter(), url);
        };
        set(&page, &["tracker=abc; Path=/", "session-auth=x", "techaro.lol-anubis-cookie-verification=id1; Secure"]);
        set(&pass, &["custom-auth=jwt; Domain=site.example; Secure", "evil=1; Domain=other.example"]);
        let sent = jar.cookies(&page).unwrap();
        assert_eq!(sent.to_str().unwrap(), "techaro.lol-anubis-cookie-verification=id1; custom-auth=jwt");
        // The domain cookie reaches the parent domain; the host-only one doesn't
        let parent = jar.cookies(&Url::parse("https://site.example/").unwrap()).unwrap();
        assert_eq!(parent.to_str().unwrap(), "custom-auth=jwt");
        assert!(jar.cookies(&Url::parse("https://other.example/").unwrap()).is_none());
        assert!(jar.cookies(&Url::parse("http://www.site.example/").unwrap()).is_none(), "secure cookies need https");
        // Clearing works
        set(&page, &["techaro.lol-anubis-cookie-verification=; Max-Age=-1"]);
        assert_eq!(jar.cookies(&page).unwrap().to_str().unwrap(), "custom-auth=jwt");
    }
}
