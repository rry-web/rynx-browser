//! ALTCHA proof-of-work checks, passed without JavaScript.
//!
//! ALTCHA (https://altcha.org) asks a visitor's browser for a small proof of work: given a salt
//! and a hash, find the number (up to a limit) that the salt followed by the number hashes to.
//! The page does it in JavaScript and posts the number, with the challenge's signed fields, to
//! the site; the site answers with a pass (a cookie) and where to go. rynx does the same work in
//! Rust and sends the same answer, so the site gets exactly what it asked for. Nothing about the
//! browser is pretended: it's only the work.
//!
//! Pages put the challenge in a script (`{"algorithm":"SHA-256","challenge":...}`) and post the
//! answer with fetch(): both are read from the page. The answer goes only to the page's own site.

use serde_json::{Value, json};
use std::time::{Duration, Instant};
use url::Url;

/// Largest number searched: ALTCHA's default is 100,000 and sites raise it; beyond this the
/// work would take too long anyway
const MAX_NUMBER: u64 = 50_000_000;

/// Longest the work may take before giving up
const WORK_TIME_LIMIT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct Challenge {
    pub algorithm: String,
    pub challenge: String,
    pub max_number: u64,
    pub salt: String,
    pub signature: String,
    /// Where the page posts the answer (as written in the page)
    pub answer_path: String,
}

/// The ALTCHA challenge on a page, if `html` is one rynx can answer: the challenge in a script,
/// and the address the script posts the answer to
pub fn detect(html: &str) -> Option<Challenge> {
    // Most pages aren't challenges
    if !html.contains("\"maxnumber\"") || !html.contains("\"signature\"") {
        return None;
    }
    let start = html.find("{\"algorithm\"")?;
    let data: Value = serde_json::Deserializer::from_str(&html[start..]).into_iter::<Value>().next()?.ok()?;
    let text = |key: &str| data.get(key).and_then(Value::as_str).map(str::to_string);
    Some(Challenge {
        algorithm: text("algorithm")?,
        challenge: text("challenge")?.to_ascii_lowercase(),
        max_number: data.get("maxnumber").and_then(Value::as_u64)?,
        salt: text("salt")?,
        signature: text("signature")?,
        answer_path: posted_to(html)?,
    })
}

/// The address a page's script posts to: `fetch("/api/challenge",{method:"POST"`
fn posted_to(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(at) = rest.find("fetch(") {
        rest = &rest[at + "fetch(".len()..];
        let quote = rest.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let address = rest[1..].split(quote).next()?;
        let after = &rest[1 + address.len()..];
        let nearby: String = after.chars().take(80).filter(|c| !c.is_whitespace()).collect();
        if nearby.replace('\'', "\"").contains("method:\"POST\"") {
            return Some(address.to_string());
        }
    }
    None
}

fn hash(algorithm: &str, data: &[u8]) -> Option<Vec<u8>> {
    let algorithm = match algorithm.to_ascii_uppercase().as_str() {
        "SHA-256" => &ring::digest::SHA256,
        "SHA-384" => &ring::digest::SHA384,
        "SHA-512" => &ring::digest::SHA512,
        "SHA-1" => &ring::digest::SHA1_FOR_LEGACY_USE_ONLY,
        _ => return None,
    };
    Some(ring::digest::digest(algorithm, data).as_ref().to_vec())
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

/// The number the challenge's hash is of. Slow: run it on a blocking thread.
pub fn solve(challenge: &Challenge) -> Result<u64, String> {
    let wanted = unhex(&challenge.challenge).ok_or("the bot check's data is malformed")?;
    hash(&challenge.algorithm, b"")
        .ok_or_else(|| format!("this site's bot check uses \"{}\", which rynx can't do yet", challenge.algorithm))?;
    let started = Instant::now();
    for number in 0..=challenge.max_number.min(MAX_NUMBER) {
        let data = format!("{}{}", challenge.salt, number);
        if hash(&challenge.algorithm, data.as_bytes()).is_some_and(|h| h == wanted) {
            return Ok(number);
        }
        if number % 4096 == 0 && started.elapsed() > WORK_TIME_LIMIT {
            return Err("this site's bot check took too long".into());
        }
    }
    Err("this site's bot check has no answer rynx could find".into())
}

/// Base64, as JavaScript's btoa writes it
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What's posted: the answer with the challenge's signed fields, in base64, as the page sends it
pub fn answer_body(challenge: &Challenge, number: u64, elapsed: Duration) -> String {
    let solution = json!({
        "algorithm": challenge.algorithm,
        "challenge": challenge.challenge,
        "number": number,
        "salt": challenge.salt,
        "signature": challenge.signature,
    });
    json!({ "payload": base64(solution.to_string().as_bytes()), "st": elapsed.as_millis() as u64 }).to_string()
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
    let answer_url = page.join(&challenge.answer_path).map_err(|_| "the bot check's address is invalid".to_string())?;
    // Only on the same site: the answer goes nowhere else
    if answer_url.origin() != page.origin() {
        return Err("the bot check points to another site".into());
    }
    let started = Instant::now();
    let to_solve = challenge.clone();
    let number = tokio::task::spawn_blocking(move || solve(&to_solve))
        .await
        .map_err(|_| "the bot check's work stopped unexpectedly".to_string())??;
    // The pass the answer brings is kept (rynx keeps no other cookies for the site)
    crate::anubis::expect_pass_from(&answer_url);
    let response = client
        .post(answer_url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "*/*")
        .body(answer_body(&challenge, number, started.elapsed()))
        .send()
        .await
        .map_err(|e| format!("the answer couldn't be sent ({})", e))?;
    if !response.status().is_success() {
        return Err(format!("the site didn't accept rynx's answer (HTTP {})", response.status().as_u16()));
    }
    let text = crate::network::read_text_limited(response, 64 * 1024).await?;
    let answer: Value = serde_json::from_str(&text).map_err(|_| "the site's answer wasn't understood".to_string())?;
    let destination = match (answer.get("ok").and_then(Value::as_bool), answer.get("dest").and_then(Value::as_str)) {
        (Some(true), Some(dest)) => dest.to_string(),
        _ => {
            let why = answer.get("error").and_then(Value::as_str).unwrap_or("it was rejected");
            return Err(format!("the site didn't accept rynx's answer ({})", why));
        }
    };
    let next = page.join(&destination).map_err(|_| "the bot check sent rynx to an invalid address".to_string())?;
    if next.origin() != page.origin() {
        return Err("the bot check sent rynx to another site".into());
    }
    crate::network::get_following_redirects(client, next.as_str(), i2p_mode).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_btoas() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"{\"a\":1}"), "eyJhIjoxfQ==");
    }

    #[test]
    fn the_post_address_is_found() {
        assert_eq!(posted_to(r#"fetch("/api/challenge",{method:"POST",headers:{}})"#), Some("/api/challenge".into()));
        assert_eq!(posted_to(r#"fetch('/x', { method: 'POST' })"#), Some("/x".into()));
        assert_eq!(posted_to(r#"fetch("/data.json").then(f)"#), None);
    }
}
