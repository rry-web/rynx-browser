//! A Gopher menu, and a gopher:// address taken apart
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    rynx::gopher::menu_to_html(&text);
    if let Ok(target) = rynx::gopher::parse(&format!("gopher://host.example/{}", text)) {
        // What's sent can never carry a line break: that's what keeps commands out of selectors
        assert!(!target.selector.contains(['\r', '\n', '\0']));
        assert!(!target.query.unwrap_or_default().contains(['\r', '\n', '\0']));
    }
});
