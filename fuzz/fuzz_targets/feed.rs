//! An RSS or Atom feed (or anything served as one)
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    rynx::feeds::is_feed(Some("application/xml"), &text);
    let (_, html) = rynx::feeds::feed_to_html(&text);
    // Entities are never expanded, so the page can't be much bigger than the feed
    assert!(html.len() <= text.len() * 8 + 4096, "{} from {}", html.len(), text.len());
});
