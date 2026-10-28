//! Showing site names so they can't pass for others. International names travel encoded
//! ("xn--bcher-kva.de" for bücher.de); rynx shows them readably when that's safe, and encoded,
//! with a warning, when a name could pass for another: a part mixing Latin, Cyrillic or Greek
//! letters ("аpple.com" with a Cyrillic а), one written entirely in Cyrillic or Greek letters
//! that look Latin ("аррlе"), or one with characters that look like a slash or a dot.

use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Script {
    /// Digits and hyphens, which go with any script
    Common,
    Cyrillic,
    Greek,
    /// Latin, and every other script
    Other,
}

fn script(ch: char) -> Script {
    match ch as u32 {
        0x30..=0x39 | 0x2d => Script::Common,
        0x370..=0x3ff | 0x1f00..=0x1fff => Script::Greek,
        0x400..=0x52f | 0x2de0..=0x2dff | 0xa640..=0xa69f | 0x1c80..=0x1c8f => Script::Cyrillic,
        _ => Script::Other,
    }
}

/// Cyrillic and Greek letters that look like Latin ones
const LATIN_LOOKALIKES: &str = "аеорсухіјѕԁԛԝһӏьвкмнтгпАВЕКМНОРСТХѵʏαοτυνικρεχγωϲϳ";

/// Characters that look like the parts of an address
const PUNCTUATION_LOOKALIKES: &[char] = &['⁄', '∕', '／', '．', '。', '｡', '？', '＃', '＠', '։', '꞉', '﹒', '․'];

/// The script of a name's ending (com, рф, ελ), if it's one letters can pass for
fn ending_script(name: &str) -> Option<Script> {
    let ending = name.rsplit('.').next()?;
    let mut scripts = ending.chars().map(script).filter(|s| *s != Script::Common);
    let first = scripts.next()?;
    scripts.all(|s| s == first).then_some(first)
}

/// Whether one part of a name, as shown, can't pass for another name. `ending` is the script of
/// the name's ending: an all-Cyrillic name under a Cyrillic ending (москва.рф) can't pass for a
/// Latin one, which would need a Latin ending.
fn safe_label(label: &str, ending: Option<Script>) -> bool {
    if label.chars().any(|c| PUNCTUATION_LOOKALIKES.contains(&c) || c.is_control() || c.is_whitespace()) {
        return false;
    }
    let scripts: Vec<Script> = label.chars().map(script).filter(|s| *s != Script::Common).collect();
    let has = |wanted: Script| scripts.contains(&wanted);
    // Cyrillic or Greek mixed with each other, or with any other script (Latin included)
    if (has(Script::Cyrillic) && has(Script::Greek))
        || ((has(Script::Cyrillic) || has(Script::Greek)) && has(Script::Other))
    {
        return false;
    }
    // Entirely Cyrillic or Greek letters that look Latin ("аррlе"), outside their own endings
    let own_ending = (has(Script::Cyrillic) && ending == Some(Script::Cyrillic))
        || (has(Script::Greek) && ending == Some(Script::Greek));
    if !own_ending
        && (has(Script::Cyrillic) || has(Script::Greek))
        && label.chars().filter(|c| script(*c) != Script::Common).all(|c| LATIN_LOOKALIKES.contains(c))
    {
        return false;
    }
    true
}

/// How a host is shown: readably if it's safe, and the readable form a risky one would pass for
pub struct ShownHost {
    pub shown: String,
    /// Set when the name is kept encoded because it could pass for another: its readable form
    pub lookalike: Option<String>,
}

/// How to show a host name (already encoded, as addresses carry it)
pub fn show_host(host: &str) -> ShownHost {
    let plain = ShownHost { shown: host.to_string(), lookalike: None };
    if !host.split('.').any(|label| label.get(..4).is_some_and(|start| start.eq_ignore_ascii_case("xn--"))) {
        return plain;
    }
    let (readable, result) = idna::domain_to_unicode(host);
    if result.is_err() {
        return ShownHost { shown: host.to_string(), lookalike: Some(readable) };
    }
    let ending = ending_script(&readable);
    if readable.split('.').all(|label| safe_label(label, ending)) {
        ShownHost { shown: readable, lookalike: None }
    } else {
        ShownHost { shown: host.to_string(), lookalike: Some(readable) }
    }
}

/// An address as shown: its host readable when that's safe. The second part is set when the host
/// could pass for another name: what it looks like.
pub fn show_address(address: &str) -> (Cow<'_, str>, Option<String>) {
    let Ok(url) = url::Url::parse(address) else { return (Cow::Borrowed(address), None) };
    let Some(host) = url.host_str() else { return (Cow::Borrowed(address), None) };
    let shown = show_host(host);
    if shown.lookalike.is_some() || shown.shown == host {
        return (Cow::Borrowed(address), shown.lookalike);
    }
    // The host appears once, after the scheme and any user name
    match address.find(host) {
        Some(at) => {
            let mut readable = String::with_capacity(address.len());
            readable.push_str(&address[..at]);
            readable.push_str(&shown.shown);
            readable.push_str(&address[at + host.len()..]);
            (Cow::Owned(readable), None)
        }
        None => (Cow::Borrowed(address), None),
    }
}

/// A host for looking up: international names encoded, as DNS needs them. Web addresses come
/// encoded already; Gemini and Gopher ones may carry the name percent-encoded or as typed.
pub fn host_for_lookup(host: &str) -> Cow<'_, str> {
    if host.is_ascii() && !host.contains('%') {
        return Cow::Borrowed(host);
    }
    let decoded = percent_encoding::percent_decode_str(host).decode_utf8_lossy();
    match idna::domain_to_ascii(&decoded) {
        Ok(ascii) => Cow::Owned(ascii),
        Err(_) => Cow::Borrowed(host),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_with_hidden_or_address_like_characters_are_unsafe() {
        // International names can't carry these once decoded, but the check doesn't rely on that
        for label in ["ex\u{7}ample", "ex ample", "ex\tample", "pay⁄pal", "pay．pal"] {
            assert!(!safe_label(label, None), "{label:?}");
        }
        assert!(safe_label("bücher", None));
    }

    #[test]
    fn cyrillic_or_greek_mixed_with_any_other_script_is_unsafe() {
        assert!(!safe_label("мир例え", Some(Script::Cyrillic)), "Cyrillic and Han");
        assert!(!safe_label("λόγος例", Some(Script::Greek)), "Greek and Han");
        assert!(!safe_label("мирλ", Some(Script::Cyrillic)), "Cyrillic and Greek");
        assert!(safe_label("例え", None));
        assert!(safe_label("мир", Some(Script::Cyrillic)));
    }

    #[test]
    fn plain_addresses_are_shown_as_they_are_without_a_copy() {
        assert!(matches!(show_address("https://example.com/a").0, Cow::Borrowed("https://example.com/a")));
    }
}
