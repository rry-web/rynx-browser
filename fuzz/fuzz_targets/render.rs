//! A page: laid out as it would be shown, at a width chosen by the input, whole and in reader mode
#![no_main]
use libfuzzer_sys::fuzz_target;
use rynx::network::PageKind;
use rynx::renderer::{RenderJob, render_page};

fuzz_target!(|data: &[u8]| {
    let Some((&width, rest)) = data.split_first() else { return };
    let source = String::from_utf8_lossy(rest);
    for (kind, reader) in [(PageKind::Html, false), (PageKind::Html, true), (PageKind::PlainText, false)] {
        let job = RenderJob {
            source: source.as_ref().into(),
            kind,
            source_view: false,
            reader,
            image_sizes: Default::default(),
            width: u16::from(width) + 4,
        };
        let page = render_page(&job);
        // Every link and jump target must point at a line that exists
        for link in &page.links {
            assert!(link.line_index < page.lines.len());
            assert!(link.x_start < link.x_end);
        }
        for &line in page.anchors.values() {
            assert!(line <= page.lines.len());
        }
    }
});
