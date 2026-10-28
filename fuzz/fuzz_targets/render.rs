//! A page: laid out as it would be shown, at a width chosen by the input, whole and in reader
//! mode, then again with its images loaded (sizes chosen by the input), which lays them out too
#![no_main]
use libfuzzer_sys::fuzz_target;
use rynx::network::PageKind;
use rynx::renderer::{RenderJob, RenderedPage, render_page};
use std::collections::HashMap;
use std::sync::Arc;

fn check(page: &RenderedPage) {
    // Every link, jump target and image block must point at lines that exist
    for link in &page.links {
        assert!(link.line_index < page.lines.len());
        assert!(link.x_start < link.x_end);
    }
    for &line in page.anchors.values() {
        assert!(line <= page.lines.len());
    }
    for block in &page.image_blocks {
        assert!(
            block.line_index + block.rows as usize <= page.lines.len(),
            "{block:?} past {} lines",
            page.lines.len()
        );
        assert!(block.image < page.images.len());
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&width, rest)) = data.split_first() else { return };
    let Some((&size_seed, rest)) = rest.split_first() else { return };
    let source = String::from_utf8_lossy(rest);
    let job = |kind, reader, image_sizes| RenderJob {
        source: source.as_ref().into(),
        kind,
        source_view: false,
        reader,
        image_sizes,
        width: u16::from(width) + 4,
    };
    let mut images = Vec::new();
    for (kind, reader) in [(PageKind::Html, false), (PageKind::Html, true), (PageKind::PlainText, false)] {
        let page = render_page(&job(kind, reader, Arc::default()));
        check(&page);
        images.extend(page.images.iter().map(|i| i.src.clone()));
    }
    // Again with every image loaded, at sizes from 1x1 to 40x12 cells
    let sizes: HashMap<String, (u16, u16)> = images
        .into_iter()
        .enumerate()
        .map(|(i, src)| {
            let n = size_seed as usize + i * 7;
            (src, ((n % 40) as u16 + 1, (n % 12) as u16 + 1))
        })
        .collect();
    let page = render_page(&job(PageKind::Html, false, Arc::new(sizes)));
    check(&page);
});
