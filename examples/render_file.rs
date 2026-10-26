//! Render an HTML file as rynx would and print it:
//! cargo run --example render_file -- page.html [width] [--reader]
use rynx::network::PageKind;
use rynx::renderer::{RenderJob, render_page};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reader = args.iter().any(|a| a == "--reader");
    let mut args = args.into_iter().filter(|a| a != "--reader");
    let path = args.next().expect("usage: render_file <page.html> [width] [--reader]");
    let width = args.next().and_then(|w| w.parse().ok()).unwrap_or(100);
    let html = std::fs::read_to_string(path).expect("readable file");
    let page = render_page(&RenderJob { source: html.into(), kind: PageKind::Html, source_view: false, reader, width });
    if page.reader_found == Some(false) {
        eprintln!("(no article found: the whole page is shown)");
    }
    for line in page.lines {
        println!("{}", line);
    }
}
