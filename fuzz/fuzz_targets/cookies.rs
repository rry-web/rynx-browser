//! Set-Cookie headers: only Anubis's cookies are kept, and only sent back to their own site
#![no_main]
use libfuzzer_sys::fuzz_target;
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;

fuzz_target!(|data: &[u8]| {
    let jar = rynx::anubis::Cookies::default();
    let site = url::Url::parse("https://www.site.example/.within.website/x/cmd/anubis/api/pass-challenge").unwrap();
    let headers: Vec<HeaderValue> =
        data.split(|&b| b == b'\n').filter_map(|line| HeaderValue::from_bytes(line).ok()).collect();
    jar.set_cookies(&mut headers.iter(), &site);
    jar.cookies(&site);
    // Nothing set by this site is sent to an unrelated one
    let other = url::Url::parse("https://other.example/").unwrap();
    assert!(jar.cookies(&other).is_none());
});
