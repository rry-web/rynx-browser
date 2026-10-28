use rynx::names::{host_for_lookup, show_address, show_host};

fn encoded(name: &str) -> String {
    idna::domain_to_ascii(name).unwrap()
}

#[test]
fn international_names_are_shown_readably_when_safe() {
    for name in ["bücher.de", "пример.рф", "ελληνικά.gr", "例え.jp", "münchen-straße.de", "москва2024.рф"]
    {
        let host = encoded(name);
        assert!(host.contains("xn--"), "{host}");
        let shown = show_host(&host);
        assert_eq!(shown.shown, name, "{host}");
        assert_eq!(shown.lookalike, None, "{name} was taken for a look-alike");
    }
    // Plain names are left alone
    assert_eq!(show_host("example.com").shown, "example.com");
}

#[test]
fn names_that_could_pass_for_others_stay_encoded_with_a_warning() {
    for name in [
        "аpple.com", // Cyrillic а among Latin letters
        "pаypal.com",
        "аррlе.com", // entirely Cyrillic, every letter looks Latin
        "ѕсоре.com",
        "gοοgle.com", // Greek ο
        "οο.com",
        "москва.com", // all Cyrillic look-alikes under a Latin ending
    ] {
        let host = encoded(name);
        let shown = show_host(&host);
        assert_eq!(shown.shown, host, "{name} was shown readably");
        assert_eq!(shown.lookalike.as_deref(), Some(name));
    }
}

#[test]
fn addresses_show_their_site_readably() {
    let (shown, warning) = show_address("https://xn--bcher-kva.de/b%C3%BCcher?q=1");
    assert_eq!(shown, "https://bücher.de/b%C3%BCcher?q=1");
    assert_eq!(warning, None);
    let (shown, warning) = show_address("https://xn--pple-43d.com/login");
    assert_eq!(shown, "https://xn--pple-43d.com/login");
    assert_eq!(warning.as_deref(), Some("аpple.com"));
    // The host is replaced, not text elsewhere that happens to match
    let (shown, _) = show_address("https://user@xn--bcher-kva.de/xn--bcher-kva.de");
    assert_eq!(shown, "https://user@bücher.de/xn--bcher-kva.de");
    assert_eq!(show_address("not an address").0, "not an address");
}

#[test]
fn gemini_and_gopher_names_are_looked_up_encoded() {
    // The url crate keeps gemini:// hosts percent-encoded
    let url = url::Url::parse("gemini://bücher.de/").unwrap();
    assert_eq!(host_for_lookup(url.host_str().unwrap()), "xn--bcher-kva.de");
    assert_eq!(host_for_lookup("example.org"), "example.org");
    let target = rynx::gopher::parse("gopher://bücher.de/1/").unwrap();
    assert_eq!(target.host, "xn--bcher-kva.de");
}

#[test]
fn direction_controls_cant_disguise_text() {
    // "invoice\u{202e}fdp.exe" would show as "invoiceexe.pdf"
    let clean =
        rynx::renderer::sanitize_for_terminal("invoice\u{202e}fdp.exe and \u{2066}isolated\u{2069} \u{200f}mark");
    assert_eq!(clean, "invoicefdp.exe and isolated mark");
    // Ordinary right-to-left text is kept
    assert_eq!(rynx::renderer::sanitize_for_terminal("שלום עולם"), "שלום עולם");
}

#[test]
fn a_page_cant_hide_reversed_text_in_its_links() {
    let html = "<p><a href=\"/report\u{202e}fdp.exe\">report\u{202e}fdp.exe</a></p>";
    let job = rynx::renderer::RenderJob {
        source: html.into(),
        kind: rynx::network::PageKind::Html,
        source_view: false,
        reader: false,
        image_sizes: Default::default(),
        width: 80,
    };
    let page = rynx::renderer::render_page(&job);
    let text: String = page.lines.iter().map(|l| l.to_string()).collect();
    assert!(!text.contains('\u{202e}'), "{text:?}");
    assert!(text.contains("reportfdp.exe"), "{text:?}");
}
