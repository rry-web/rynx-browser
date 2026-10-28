//! Addresses and site names as shown: readable only when safe, and never with direction controls
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    for address in [text.to_string(), format!("https://{}/", text), format!("gemini://{}/", text)] {
        let (shown, lookalike) = rynx::names::show_address(&address);
        if lookalike.is_some() {
            // A risky name is shown as it travels, encoded
            assert_eq!(shown, address);
        }
        if let Ok(url) = url::Url::parse(&address)
            && let Some(host) = url.host_str()
        {
            rynx::names::host_for_lookup(host);
        }
    }
    let host = rynx::names::show_host(&text);
    let _ = host.lookalike;
    let clean = rynx::renderer::sanitize_for_terminal(&text);
    assert!(!clean.contains(['\u{202e}', '\u{202d}', '\u{2066}', '\u{2067}', '\u{2068}']));
});
