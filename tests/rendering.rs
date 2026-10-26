//! How HTML is laid out as terminal text.

use rynx_browser::renderer::DomRenderer;
use scraper::Html;

/// Render at a content width of `width` columns
fn render_full(html: &str, width: usize) -> DomRenderer {
    // DomRenderer::new leaves room for the two border columns
    let mut renderer = DomRenderer::new(width + 2);
    renderer.render(&Html::parse_document(html));
    renderer
}

fn render(html: &str, width: usize) -> Vec<String> {
    render_full(html, width).lines.iter().map(|l| l.to_string()).collect()
}

fn text_lines(html: &str, width: usize) -> Vec<String> {
    render(html, width).into_iter().filter(|l| !l.trim().is_empty()).collect()
}

#[test]
fn inline_code_stays_in_its_sentence() {
    assert_eq!(
        text_lines("<p>Call <code>foo()</code> to start, then <code>bar()</code>.</p>", 80),
        ["Call foo() to start, then bar()."]
    );
}

#[test]
fn spaces_come_only_from_the_source() {
    assert_eq!(
        text_lines("<p>Hello<b>!</b> Read <a href='/x'>this</a>, then <i>stop</i> .</p>", 80),
        ["Hello! Read this, then stop ."]
    );
    // &nbsp; is not collapsed
    assert_eq!(text_lines("<p>a&nbsp;&nbsp;b</p>", 80), ["a\u{a0}\u{a0}b"]);
}

#[test]
fn wrapped_lines_have_no_trailing_spaces() {
    let lines = render("<p>one two three four five six seven eight nine ten</p>", 20);
    assert!(lines.iter().all(|l| !l.ends_with(' ')), "{lines:?}");
    assert!(lines.iter().all(|l| l.chars().count() <= 20), "{lines:?}");
}

#[test]
fn every_heading_level_is_its_own_block() {
    assert_eq!(
        render("<h1>One</h1><h4>Four</h4><h5>Five</h5><h6>Six</h6><p>text</p>", 80),
        ["One", "", "Four", "", "Five", "", "Six", "", "text", ""]
    );
}

#[test]
fn blockquotes_are_marked_on_every_line() {
    let lines = render(
        "<p>He said:</p><blockquote><p>quoted text that is long enough to wrap</p><p>again</p></blockquote><p>after</p>",
        24,
    );
    assert_eq!(
        lines,
        ["He said:", "", "│ quoted text that is", "│ long enough to wrap", "│", "│ again", "", "after", ""]
    );
}

#[test]
fn ordered_lists_are_numbered() {
    assert_eq!(
        text_lines("<ol><li>first</li><li>second</li></ol><ol start='9'><li>a</li><li>b</li></ol>", 80),
        ["  1. first", "  2. second", "   9. a", "  10. b"]
    );
    assert_eq!(
        text_lines("<ol type='a'><li>x</li><li value='26'>z</li><li>aa</li></ol>", 80),
        ["   a. x", "   z. z", "  aa. aa"]
    );
    assert_eq!(
        text_lines("<ol type='I' reversed><li>c</li><li>b</li><li>a</li></ol>", 80),
        ["  III. c", "   II. b", "    I. a"]
    );
}

#[test]
fn list_items_wrap_under_their_text() {
    assert_eq!(
        text_lines("<ol><li>an item with enough words to wrap</li></ol>", 20),
        ["  1. an item with", "     enough words to", "     wrap"]
    );
}

#[test]
fn nested_lists_indent_and_change_bullets() {
    assert_eq!(
        text_lines("<ul><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li></ul>", 80),
        ["  • a", "      ◦ b", "          ▪ c"]
    );
}

#[test]
fn block_content_in_list_items_starts_beside_the_marker() {
    assert_eq!(
        text_lines("<ul><li><p>first para</p></li><li><p>second</p></li></ul>", 80),
        ["  • first para", "  • second"]
    );
}

#[test]
fn definition_lists_indent_definitions() {
    assert_eq!(
        text_lines("<dl><dt>Term</dt><dd>Meaning of it</dd></dl>", 80),
        ["Term", "    Meaning of it"]
    );
}

#[test]
fn preformatted_text_keeps_lines_and_spacing() {
    let html = "<pre><span>fn</span> <span>main</span>() {\n\n    <span>println!</span>(\"hi\");\n}</pre>";
    assert_eq!(render(html, 80), ["fn main() {", "", "    println!(\"hi\");", "}", ""]);
}

#[test]
fn runs_of_line_breaks_collapse_to_one_blank_line() {
    assert_eq!(
        render("<p>one<br>two<br><br>stanza</p><div><br><br><br><br></div><h2>Next</h2>", 80),
        ["one", "two", "", "stanza", "", "Next", ""]
    );
}

#[test]
fn noscript_content_is_shown() {
    // At the very start of a document the parser puts <noscript> in <head>, where it holds style
    // fallbacks and is skipped; visible <noscript> content comes after body content
    assert_eq!(
        text_lines("<p>Welcome</p><noscript><p>JavaScript is <b>off</b></p></noscript>", 80),
        ["Welcome", "JavaScript is off"]
    );
}

#[test]
fn hidden_and_non_text_content_is_skipped() {
    let html = "<p>a<svg><title>icon</title></svg>b</p>\
                <p hidden>x</p><p aria-hidden='true'>y</p><p style='display: none'>z</p>\
                <template><p>t</p></template><p>end</p>";
    assert_eq!(text_lines(html, 80), ["ab", "end"]);
}

#[test]
fn images_show_alt_text_unless_decorative() {
    assert_eq!(text_lines("<p>x <img alt='A chart'> <img alt=''> <img src='a.png'></p>", 80), ["x [A chart]"]);
    // An image link without alt text stays visible and clickable
    let r = render_full("<p><a href='/home'><img src='logo.png'></a></p>", 80);
    assert_eq!(r.lines[0].to_string(), "[image]");
    assert_eq!(r.links[0].url, "/home");
}

#[test]
fn embedded_frames_and_media_become_links() {
    let r = render_full(
        "<iframe src='https://video.example/embed/1' title='Talk'></iframe>\
         <video><source src='/clip.mp4'></video><audio src='/song.ogg'></audio>",
        80,
    );
    let text: Vec<String> = r.lines.iter().map(|l| l.to_string()).collect();
    assert_eq!(text, ["[Embedded: Talk]", "[Video]", "[Audio]"]);
    let urls: Vec<&str> = r.links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(urls, ["https://video.example/embed/1", "/clip.mp4", "/song.ogg"]);
}

#[test]
fn anchors_without_href_are_not_links() {
    let r = render_full("<p><a id='top'>Title</a> and <a href='/x'>link</a></p>", 80);
    assert_eq!(r.links.len(), 1);
    assert_eq!(r.links[0].url, "/x");
    assert!(r.anchors.contains_key("top"));
}

#[test]
fn inline_markers_for_quotes_and_scripts() {
    assert_eq!(
        text_lines("<p>x<sup>2</sup>, H<sub>2</sub>O, <q>quoted</q>, <del>gone</del></p>", 80),
        ["x^2, H_2O, “quoted”, gone"]
    );
}

#[test]
fn details_show_their_summary_and_content() {
    assert_eq!(
        text_lines("<details><summary>More info</summary><p>Hidden by default</p></details>", 80),
        ["▸ More info", "Hidden by default"]
    );
}

#[test]
fn extremely_deep_nesting_does_not_overflow_the_stack() {
    // Rendering recursed once per element; ~10,000 levels overflowed the stack and crashed.
    // Test threads have smaller stacks than the main thread, so this is a stricter check.
    let html = format!("{}deep text{}", "<span>".repeat(20_000), "</span>".repeat(20_000));
    assert_eq!(text_lines(&html, 80), ["deep text"]);
}

const FILES_TABLE: &str = "<table><caption>Downloads</caption>\
    <thead><tr><th>Name</th><th>Size</th><th>Notes</th></tr></thead>\
    <tbody><tr><td><a href='/a.txt'>alpha.txt</a></td><td>12 KB</td><td>plain text file that wraps</td></tr>\
    <tr><td>b</td><td>3 MB</td><td>short</td></tr>\
    <tr><td colspan='2'>spans two</td><td>x</td></tr></tbody></table>";

#[test]
fn tables_line_up_in_columns_with_a_header_rule() {
    assert_eq!(
        text_lines(FILES_TABLE, 40),
        [
            "Downloads",
            "Name      │ Size  │ Notes",
            "──────────┼───────┼─────────────────────",
            "alpha.txt │ 12 KB │ plain text file that",
            "          │       │ wraps",
            "b         │ 3 MB  │ short",
            "spans two         │ x",
        ]
    );
}

#[test]
fn links_inside_table_cells_stay_clickable() {
    let r = render_full(FILES_TABLE, 40);
    let link = &r.links[0];
    assert_eq!(link.url, "/a.txt");
    let line = r.lines[link.line_index].to_string();
    let text: String = line.chars().skip(link.x_start).take(link.x_end - link.x_start).collect();
    assert_eq!(text, "alpha.txt");
}

#[test]
fn narrow_tables_are_stacked_with_header_labels() {
    assert_eq!(
        text_lines(FILES_TABLE, 20),
        [
            "Downloads",
            "Name: alpha.txt",
            "Size: 12 KB",
            "Notes: plain text",
            "file that wraps",
            "Name: b",
            "Size: 3 MB",
            "Notes: short",
            "Name: spans two",
            "Size: x",
        ]
    );
}

#[test]
fn table_cells_can_hold_lists_and_tables() {
    assert_eq!(
        text_lines("<table><tr><td><p>left</p></td><td><ul><li>item</li></ul></td></tr></table>", 40),
        ["left │   • item"]
    );
    assert_eq!(
        text_lines("<table><tr><td>outer</td><td><table><tr><td>in1</td><td>in2</td></tr></table></td></tr></table>", 40),
        ["outer │ in1 │ in2"]
    );
}

#[test]
fn deeply_nested_tables_render_quickly() {
    // Measuring cells by rendering them would cost 2^depth renders for nested tables
    let html = format!("{}core{}", "<table><tr><td>x</td><td>".repeat(30), "</td></tr></table>".repeat(30));
    let started = std::time::Instant::now();
    let lines = text_lines(&html, 120);
    assert!(lines.concat().contains("core"));
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "took {:?}", started.elapsed());
}

#[test]
fn adjacent_links_are_kept_apart() {
    // Sites often separate links with CSS margins only
    assert_eq!(
        text_lines("<p><a href='/a'>Home</a><a href='/b'>new</a>, <a href='/c'>past</a></p>", 80),
        ["Home new, past"]
    );
}
