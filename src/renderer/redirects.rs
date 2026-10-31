//! Pages that send you on elsewhere. A meta refresh, often inside <noscript>, is the site's own
//! way on for browsers without scripts, and rynx follows a quick one as it would an HTTP redirect.
//! A page that is nothing but a script setting its location is offered as a link to where the
//! script would go: the address is read from the script's text, never run.

use super::*;

/// A refresh slower than this is a timer reloading the page (news sites), not a way on
const MAX_REFRESH_DELAY: u32 = 60;

/// A page with fewer words than this, and a script that sets its location, is only a redirect
const SPARSE_PAGE_WORDS: usize = 30;

/// A meta refresh to another address: how many seconds it waits, and the address as written
#[derive(Clone, Debug, PartialEq)]
pub struct Refresh {
    pub delay: u32,
    pub url: String,
}

/// The page's meta refresh to an address, if it has one: in its <head>, or in a <noscript>
/// (whose content the parser keeps as text, as a scripting browser would)
pub(super) fn meta_refresh(document: &Html) -> Option<Refresh> {
    static META: OnceLock<Selector> = OnceLock::new();
    static NOSCRIPT: OnceLock<Selector> = OnceLock::new();
    let meta = META.get_or_init(|| Selector::parse("meta[http-equiv][content]").expect("valid selector"));
    let noscript = NOSCRIPT.get_or_init(|| Selector::parse("noscript").expect("valid selector"));
    let refresh_in = |elements: scraper::html::Select| {
        elements
            .filter(|m| m.value().attr("http-equiv").is_some_and(|v| v.trim().eq_ignore_ascii_case("refresh")))
            .find_map(|m| parse_refresh(m.value().attr("content")?))
    };
    refresh_in(document.select(meta)).or_else(|| {
        document.select(noscript).find_map(|element| {
            let fragment = Html::parse_fragment(&element.text().collect::<String>());
            refresh_in(fragment.select(meta))
        })
    })
}

/// A refresh's content, "5; url=/next" (also "5;URL='/next'", "0, /next"), if it leads to an
/// address soon enough; one without an address only reloads the page
fn parse_refresh(content: &str) -> Option<Refresh> {
    let content = content.trim();
    let digits = content.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(content.len());
    let delay = content[..digits].split('.').next()?.parse::<u32>().ok()?;
    let mut rest = content[digits..].trim_start().trim_start_matches([';', ',']).trim_start();
    if rest.get(..3).is_some_and(|u| u.eq_ignore_ascii_case("url")) {
        let after = rest[3..].trim_start();
        if let Some(value) = after.strip_prefix('=') {
            rest = value.trim_start();
        }
    }
    let url = match rest.chars().next() {
        Some(quote @ ('\'' | '"')) => rest[1..].split(quote).next().unwrap_or(""),
        _ => rest,
    }
    .trim();
    (delay <= MAX_REFRESH_DELAY && !url.is_empty()).then(|| Refresh { delay, url: url.to_string() })
}

/// Where the page's scripts would send you, if they set its location to one fixed address
/// (`location = "…"`, `location.href = "…"`, `location.replace("…")` and the like); several
/// different ones, or an address built up from pieces, aren't a simple redirect
pub(super) fn script_redirect(document: &Html) -> Option<String> {
    static SCRIPT: OnceLock<Selector> = OnceLock::new();
    let selector = SCRIPT.get_or_init(|| Selector::parse("script:not([src])").expect("valid selector"));
    let mut targets: Vec<String> = Vec::new();
    for script in document.select(selector) {
        let is_javascript = script.value().attr("type").is_none_or(|kind| {
            let kind = kind.trim().to_ascii_lowercase();
            kind.is_empty() || kind == "module" || kind.contains("javascript") || kind.contains("ecmascript")
        });
        if !is_javascript {
            continue;
        }
        for target in location_assignments(&script.text().collect::<String>()) {
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
    }
    match targets.as_slice() {
        [only] => Some(only.clone()),
        _ => None,
    }
}

/// The fixed addresses a script sets its location to
fn location_assignments(script: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = script[from..].find("location").map(|i| from + i) {
        from = at + "location".len();
        // A word of its own ("location", "window.location"), not part of another
        let before = script[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            continue;
        }
        let mut rest = script[from..].trim_start();
        if let Some(after) = rest.strip_prefix(".href") {
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix(".replace(").or_else(|| rest.strip_prefix(".assign(")) {
            if let Some((literal, after)) = string_literal(after.trim_start())
                && after.trim_start().starts_with(')')
            {
                found.push(literal);
            }
            continue;
        }
        // An assignment, not a comparison
        let Some(after) = rest.strip_prefix('=').filter(|after| !after.starts_with('=')) else {
            continue;
        };
        if let Some((literal, after)) = string_literal(after.trim_start()) {
            let next = after.trim_start_matches([' ', '\t']).chars().next();
            if next.is_none_or(|c| matches!(c, ';' | '}' | ')' | ',' | '\n')) {
                found.push(literal);
            }
        }
    }
    found
}

/// A quoted string at the start of `text`, unescaped, with what follows; None if it isn't one,
/// or it isn't plainly an address
fn string_literal(text: &str) -> Option<(String, &str)> {
    let quote = text.chars().next().filter(|c| matches!(c, '"' | '\'' | '`'))?;
    let mut value = String::new();
    let mut chars = text[1..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => match chars.next()?.1 {
                // Only the escapes addresses use ("https:\/\/…")
                escaped @ ('/' | '\\' | '\'' | '"') => value.push(escaped),
                _ => return None,
            },
            c if c == quote => {
                let plain = !value.is_empty()
                    && !value.contains(|c: char| c.is_whitespace() || c == '<' || c == '>')
                    && !(quote == '`' && value.contains("${"));
                return plain.then(|| (value, &text[1 + i + 1..]));
            }
            c => value.push(c),
        }
    }
    None
}

impl DomRenderer {
    /// A line above the page saying where it sends you on to, as a link
    pub(super) fn render_redirect_notice(&mut self, label: &str, url: &str) {
        self.push_text(label);
        self.push_link_text(url, url);
        self.flush_line();
        self.push_blank_line();
    }

    /// Whether the page drawn so far has hardly any words
    pub(super) fn is_sparse(&self) -> bool {
        self.words_drawn < SPARSE_PAGE_WORDS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_contents_are_read_as_browsers_read_them() {
        let refresh = |delay, url: &str| Some(Refresh { delay, url: url.to_string() });
        assert_eq!(parse_refresh("0; url=/next?nojs=1"), refresh(0, "/next?nojs=1"));
        assert_eq!(parse_refresh("5;URL='https://example.org/'"), refresh(5, "https://example.org/"));
        assert_eq!(parse_refresh(" 3 , url = \"/a b\" "), refresh(3, "/a b"));
        assert_eq!(parse_refresh("0; /plain"), refresh(0, "/plain"));
        assert_eq!(parse_refresh("1.5; url=/x"), refresh(1, "/x"));
        // A reload, and a slow timer, aren't ways on
        assert_eq!(parse_refresh("0"), None);
        assert_eq!(parse_refresh("300; url=/same"), None);
        assert_eq!(parse_refresh("soon; url=/x"), None);
    }

    #[test]
    fn fixed_script_redirects_are_found() {
        let found = |script: &str| location_assignments(script);
        assert_eq!(found(r#"window.location = "https://example.org/a";"#), ["https://example.org/a"]);
        assert_eq!(found("location.href='/b'"), ["/b"]);
        assert_eq!(found(r#"top.location.replace("https:\/\/example.org\/c")"#), ["https://example.org/c"]);
        assert_eq!(found("document.location.assign(`/d`);"), ["/d"]);
        // Built from pieces, compared, escaped oddly, or not the location: not a fixed address
        assert!(found(r#"location.href = "/e?q=" + query;"#).is_empty());
        assert!(found(r#"if (location == "/f") {}"#).is_empty());
        assert!(found(r#"location.href = "\x2fg";"#).is_empty());
        assert!(found("location = `/h/${id}`;").is_empty());
        assert!(found(r#"mylocation = "/i";"#).is_empty());
        assert!(found(r#"const here = location.pathname;"#).is_empty());
    }
}
