//! Render an HTML file as rynx would and print it: cargo run --example render_file -- page.html [width]
use rynx_browser::network::PageKind;
use rynx_browser::renderer::{RenderJob, render_page};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: render_file <page.html> [width]");
    let width = args.next().and_then(|w| w.parse().ok()).unwrap_or(100);
    let html = std::fs::read_to_string(path).expect("readable file");
    let page = render_page(&RenderJob { source: html.into(), kind: PageKind::Html, source_view: false, width });
    for line in page.lines {
        println!("{}", line);
    }
}
