//! Micron (NomadNet's page markup) drawn as NomadNet draws it, through rynx's renderer.

use ratatui::style::{Color, Modifier};
use rynx::micron::{is_micron, micron_to_html, slug};
use rynx::renderer::DomRenderer;
use scraper::Html;

fn render(source: &str, width: usize) -> DomRenderer {
    let (_, html) = micron_to_html(source);
    let mut renderer = DomRenderer::new(width + 2);
    renderer.render(&Html::parse_document(&html));
    renderer
}

fn lines(source: &str, width: usize) -> Vec<String> {
    render(source, width).lines.iter().map(|l| l.to_string()).collect()
}

/// The style of the first character of `text` where it's drawn
fn style_of(renderer: &DomRenderer, text: &str) -> ratatui::style::Style {
    for line in &renderer.lines {
        let mut drawn = String::new();
        for span in &line.spans {
            if (drawn.clone() + &span.content).contains(text) && span.content.contains(text.chars().next().unwrap()) {
                return span.style;
            }
            drawn.push_str(&span.content);
        }
    }
    panic!("no {text:?} drawn")
}

#[test]
fn each_line_is_a_line_and_blank_lines_stay() {
    assert_eq!(lines("First line\nSecond line\n\nAfter a gap", 40), ["First line", "Second line", "", "After a gap"]);
}

#[test]
fn spaces_are_kept_for_layout() {
    let drawn = lines(" indented\na    b\n  `!x`!  y", 40);
    let shown: Vec<String> = drawn.iter().map(|l| l.replace('\u{a0}', " ")).collect();
    assert_eq!(shown, [" indented", "a    b", "  x  y"]);
}

#[test]
fn formatting_carries_over_lines_until_turned_off_or_reset() {
    let r = render("plain `!bold\nstill bold`! plain `*italic `_both``\nall reset", 40);
    assert!(style_of(&r, "bold").add_modifier.contains(Modifier::BOLD));
    assert!(style_of(&r, "still").add_modifier.contains(Modifier::BOLD));
    assert!(!style_of(&r, "plain").add_modifier.contains(Modifier::BOLD));
    let both = style_of(&r, "both").add_modifier;
    assert!(both.contains(Modifier::ITALIC) && both.contains(Modifier::UNDERLINED));
    assert!(style_of(&r, "all reset").add_modifier.is_empty());
}

#[test]
fn colours_come_from_the_256_colour_palette() {
    let r = render("`Ff00red`f `B444`F222dark on grey`` `FT00ff00green", 60);
    assert_eq!(style_of(&r, "red").fg, Some(Color::Indexed(196)));
    let dark = style_of(&r, "dark on grey");
    assert_eq!((dark.fg, dark.bg), (Some(Color::Indexed(235)), Some(Color::Indexed(238))));
    assert_eq!(style_of(&r, "green").fg, Some(Color::Indexed(46)));
}

#[test]
fn sections_indent_and_less_than_returns_to_the_top() {
    let source = ">Top\nunder the top\n>>Deeper\nunder deeper\n>>>\nheadless section\n<\nback at the edge";
    let drawn = lines(source, 40);
    assert_eq!(
        drawn,
        ["Top", "under the top", "  Deeper", "  under deeper", "    headless section", "back at the edge"]
    );
    let r = render(source, 40);
    assert!(style_of(&r, "Top").add_modifier.contains(Modifier::BOLD | Modifier::UNDERLINED));
    assert!(style_of(&r, "Deeper").add_modifier.contains(Modifier::BOLD));
}

#[test]
fn the_first_heading_is_the_title_and_headings_are_anchors() {
    let (title, _) = micron_to_html("#!c=0\n\n>`!Hello`! World\ntext\n>Introduction & Setup");
    assert_eq!(title.as_deref(), Some("Hello World"));
    assert_eq!(slug("Introduction & Setup"), "introduction-setup");
    assert_eq!(slug("`Ff00Hello`f World"), "hello-world");
    let r = render(">Hello World\n`:install-notes\nNotes\n`[Jump`#install-notes]", 40);
    assert!(r.anchors.contains_key("hello-world"));
    assert_eq!(r.anchors.get("install-notes"), Some(&1));
    assert!(r.links.iter().any(|l| l.url == "#install-notes"));
}

#[test]
fn links_go_where_nomadnet_sends_them() {
    let node = "72914442a3689add83a09a767963f57c";
    let source = format!(
        "`[Home`:/page/index.mu] `[{node}:/page/about.mu] `[Query`:/page/q.mu`user|action=view|n=64] `[Web`https://example.org/]"
    );
    let r = render(&source, 100);
    let urls: Vec<&str> = r.links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "/page/index.mu",
            &format!("nomadnet://{node}/page/about.mu"),
            "/page/q.mu?action=view&n=64",
            "https://example.org/"
        ]
    );
    // A link without a label shows its address
    assert!(r.lines[0].to_string().contains(&format!("{node}:/page/about.mu")));
}

#[test]
fn centred_and_right_aligned_lines_keep_their_links_clickable() {
    let r = render("`cA `[link`:/page/x.mu] here\n`rright\n`aleft again", 30);
    let drawn: Vec<String> = r.lines.iter().map(|l| l.to_string()).collect();
    assert_eq!(drawn[0], format!("{}A link here", " ".repeat((30 - 11) / 2)));
    assert_eq!(drawn[1], format!("{}right", " ".repeat(25)));
    assert_eq!(drawn[2], "left again");
    let link = &r.links[0];
    let text: String = drawn[link.line_index].chars().skip(link.x_start).take(link.x_end - link.x_start).collect();
    assert_eq!(text, "link");
}

#[test]
fn dividers_take_their_own_character_without_space_around() {
    assert_eq!(lines("above\n-\nbelow", 10), ["above", "──────────", "below"]);
    assert_eq!(lines("-∿", 6), ["∿∿∿∿∿∿"]);
    // In a section, a divider keeps the section's indent
    assert_eq!(lines(">>Sub\n-=", 8), ["  Sub", "  ======"]);
}

#[test]
fn literal_blocks_show_markup_as_written() {
    let drawn = lines("`=\n`!not bold`! and `[not a link`x]\n\\`=\n`=\nafter", 60);
    assert!(drawn.iter().any(|l| l.contains("`!not bold`! and `[not a link`x]")), "{drawn:?}");
    assert!(drawn.iter().any(|l| l.trim() == "`="), "{drawn:?}");
    assert!(drawn.iter().any(|l| l == "after"), "{drawn:?}");
}

#[test]
fn tables_have_a_header_row() {
    let source = "`t\n| Name | Price | Qty |\n| ---- | :---: | --: |\n| `F3a3Apple`f | Free | `!5`! |\n| Orange | Ask, nicely | 3 |\n`t";
    let drawn = lines(source, 60);
    let text = drawn.join("\n");
    assert!(text.contains("Name") && text.contains("Ask, nicely") && text.contains('┼'), "{text}");
    assert!(!text.contains("----"), "{text}");
}

/// Drawn, not yet usable: there's nowhere to send them until rynx can reach Reticulum nodes
#[test]
fn fields_checkboxes_and_radio_buttons_are_drawn() {
    let source = "Name: `<user_input`Pre-defined data>\nSecret: `<!16|password`>\nNotes: `<40x5|notes`>\n\
                  `<?|sign_up|1|*`>`b Sign me up\n`<^|color|Red`>`b Red `<^|color|Blue`>`b Blue";
    let text = lines(source, 80).join("\n");
    for drawn in ["Name: [Pre-defined data", "Secret: [", "Notes: [", "Sign me up", "Red", "Blue"] {
        assert!(text.contains(drawn), "{drawn} missing from:\n{text}");
    }
}

#[test]
fn comments_headers_and_escapes() {
    let drawn = lines("#!bg=444\n# a comment\n\\>not a heading\nA \\`!backtick\\` and a \\\\ backslash", 60);
    assert_eq!(drawn, [">not a heading", "A `!backtick` and a \\ backslash"]);
}

#[test]
fn images_and_partials_are_links_for_now() {
    let r = render(
        "`(The RNS logo`w=n`a=c`:/media/demo.webp)\n`{f64a846313b874ee4a357040807f8c77:/page/partial_1.mu`10}",
        80,
    );
    let text: String = r.lines.iter().map(|l| l.to_string() + "\n").collect();
    assert!(text.contains("[Image: The RNS logo]") && text.contains("[Part of the page:"), "{text}");
    let urls: Vec<&str> = r.links.iter().map(|l| l.url.as_str()).collect();
    assert_eq!(urls, ["/media/demo.webp", "nomadnet://f64a846313b874ee4a357040807f8c77/page/partial_1.mu"]);
}

#[test]
fn collapsible_headings_show_open() {
    assert_eq!(lines("`->Hidden until expanded\ninside", 40), ["Hidden until expanded", "inside"]);
}

#[test]
fn micron_is_known_by_its_address_or_type() {
    assert!(is_micron("http://127.0.0.1:8000/page/index.mu", None));
    assert!(is_micron("http://node.example/INDEX.MU?x=1", Some("application/octet-stream")));
    assert!(is_micron("http://node.example/page", Some("text/micron; charset=utf-8")));
    assert!(!is_micron("http://node.example/page.html", Some("text/html")));
}

#[test]
fn any_text_is_drawn_without_panicking() {
    // Unfinished tags, odd colours and wide characters: drawn as best it can, never a crash
    let samples = [
        "`",
        "``",
        "`F",
        "`F12",
        "`FT12",
        "`B日本語",
        "`[",
        "`[label",
        "`[a`b`c`d]",
        "`<",
        "`<x",
        "`<|`>",
        "`<?|`",
        "`{",
        "`{}",
        "`(",
        "`()",
        "`(a)",
        ">",
        ">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>",
        "`+",
        "`->",
        "-",
        "-日",
        "<",
        "<<>",
        "\\",
        "`t",
        "`t\n|",
        "`=",
        "`:",
        "`:日本",
        "`c`r`l`a",
        "\u{7}bell\u{1b}[2J",
    ];
    for sample in samples {
        for width in [1, 10, 80] {
            let _ = render(sample, width);
        }
    }
}

#[tokio::test]
async fn a_mu_page_from_any_web_server_is_shown_as_micron() {
    use rynx::event_handler::handle_network_event;
    use rynx::network::NetworkResponse;
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let server = MockServer::builder().start().await;
    // Web servers that don't know .mu send it as a download
    Mock::given(path("/page/index.mu"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(">Node index\n`!Welcome`! to the node", "application/octet-stream"),
        )
        .mount(&server)
        .await;
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = rynx::app::App::new(tx, rx).unwrap();
    app.clipboard = None;
    app.render_in_background = false;
    app.current_tab().url_input = format!("{}/page/index.mu", server.uri());
    app.submit_request();
    loop {
        let response = tokio::time::timeout(std::time::Duration::from_secs(10), app.rx.recv()).await.unwrap().unwrap();
        let done = matches!(response, NetworkResponse::Success(..) | NetworkResponse::Error(..));
        handle_network_event(&mut app, response, 80).unwrap();
        if done {
            break;
        }
    }
    let tab = app.current_tab();
    let text: Vec<String> = tab.rendered_content.iter().map(|l| l.to_string()).collect();
    assert_eq!(tab.page_title, "Node index");
    assert!(text.iter().any(|l| l == "Welcome to the node"), "{text:?}");
}
