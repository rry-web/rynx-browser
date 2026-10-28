//! Set-Cookie headers: kept only for Anubis and the sites allowed, and never sent to another site
#![no_main]
use libfuzzer_sys::fuzz_target;
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;

fuzz_target!(|data: &[u8]| {
    let jar = rynx::cookies::Jar::default();
    jar.allow("www.site.example").unwrap();
    let pass = url::Url::parse("https://www.site.example/.within.website/x/cmd/anubis/api/pass-challenge").unwrap();
    let page = url::Url::parse("https://accounts.site.example/login").unwrap();
    let stranger = url::Url::parse("https://www.stranger.example/").unwrap();
    let headers: Vec<HeaderValue> =
        data.split(|&b| b == b'\n').filter_map(|line| HeaderValue::from_bytes(line).ok()).collect();
    jar.set_cookies(&mut headers.iter(), &pass);
    jar.set_cookies(&mut headers.iter(), &page);
    jar.cookies(&page);
    jar.sites();
    // Nothing set by these sites is sent to another one
    assert!(jar.cookies(&stranger).is_none());
    assert!(jar.cookies(&url::Url::parse("https://example.com/").unwrap()).is_none());
});
