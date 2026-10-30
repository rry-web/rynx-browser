//! A Micron page (NomadNet's): converted to HTML, then laid out narrow and wide (for alignment and indents)
#![no_main]
use libfuzzer_sys::fuzz_target;
use rynx::network::PageKind;
use rynx::renderer::{RenderJob, render_page};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (_, html) = rynx::micron::micron_to_html(&text);
    let source: std::sync::Arc<str> = html.into();
    for width in [12, 80] {
        let job = RenderJob {
            source: source.clone(),
            kind: PageKind::Html,
            source_view: false,
            reader: false,
            image_sizes: Default::default(),
            width,
        };
        render_page(&job);
    }
});
