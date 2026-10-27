//! A Gemini page: converted to HTML, then laid out
#![no_main]
use libfuzzer_sys::fuzz_target;
use rynx::network::PageKind;
use rynx::renderer::{RenderJob, render_page};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (_, html) = rynx::gemini::gemtext_to_html(&text);
    let job = RenderJob {
        source: html.into(),
        kind: PageKind::Html,
        source_view: false,
        reader: false,
        image_sizes: Default::default(),
        width: 80,
    };
    render_page(&job);
});
