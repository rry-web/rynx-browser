//! HTML forms: the controls a page declares, what the user has entered into them, and how a form
//! is submitted.
//!
//! The renderer records each form and control and reserves space for it on screen; the values the
//! user enters live in the tab and are drawn over that space, so typing never re-lays out the page.

use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FormMethod {
    Get,
    Post,
}

#[derive(Clone, Debug)]
pub struct Form {
    /// The action attribute as written; resolved against the page when submitting
    pub action: Option<String>,
    pub method: FormMethod,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlKind {
    /// A single-line field (text, search, email, number...), masked if it's a password
    Text { password: bool },
    TextArea,
    Checkbox,
    Radio,
    Select { options: Vec<SelectOption> },
    /// Submits its form: submit and image inputs, and <button>
    Submit,
    /// Sent with the form but never shown
    Hidden,
}

#[derive(Clone, Debug)]
pub struct FormControl {
    /// Index of the form it belongs to; controls outside any form can't be submitted
    pub form: Option<usize>,
    pub kind: ControlKind,
    pub name: String,
    /// Initial text, or the value sent for a checked box, radio button or pressed button
    pub value: String,
    /// Initially checked (checkbox, radio)
    pub checked: bool,
    /// Initially selected option (select)
    pub selected: usize,
    /// Columns reserved inside the control's brackets
    pub width: usize,
    /// Button text, or a text field's placeholder
    pub label: String,
    pub max_length: Option<usize>,
}

/// What the user has entered into a control
#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    Text(String),
    Checked(bool),
    Selected(usize),
    /// Buttons and hidden fields have nothing to enter
    Fixed,
}

/// Characters escaped in Gemini and Gopher queries: everything but unreserved characters
const QUERY: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// Longest text a field accepts when the page sets no maxlength
const MAX_FIELD_CHARS: usize = 10_000;

impl FormControl {
    pub fn initial_value(&self) -> ControlValue {
        match self.kind {
            ControlKind::Text { .. } | ControlKind::TextArea => ControlValue::Text(self.value.clone()),
            ControlKind::Checkbox | ControlKind::Radio => ControlValue::Checked(self.checked),
            ControlKind::Select { .. } => ControlValue::Selected(self.selected),
            ControlKind::Submit | ControlKind::Hidden => ControlValue::Fixed,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self.kind, ControlKind::Text { .. } | ControlKind::TextArea)
    }

    pub fn is_password(&self) -> bool {
        matches!(self.kind, ControlKind::Text { password: true })
    }

    /// Most characters the field accepts
    pub fn max_chars(&self) -> usize {
        self.max_length.unwrap_or(MAX_FIELD_CHARS).min(MAX_FIELD_CHARS)
    }

    /// Text drawn inside the control's brackets, exactly `self.width` columns wide. While a text
    /// field is being edited it shows the end of the text and a cursor.
    pub fn display(&self, value: &ControlValue, editing: bool) -> String {
        match (&self.kind, value) {
            (ControlKind::Text { .. } | ControlKind::TextArea, ControlValue::Text(text)) => {
                if text.is_empty() && !editing {
                    // Empty and not being edited: show the placeholder, if any
                    return fit_start(&self.label, self.width, '_');
                }
                let shown = if self.is_password() {
                    "•".repeat(text.chars().count())
                } else {
                    text.replace('\n', "⏎")
                };
                if editing {
                    fit_end(&format!("{}▏", shown), self.width, '_')
                } else {
                    fit_start(&shown, self.width, '_')
                }
            }
            (ControlKind::Checkbox, ControlValue::Checked(on)) => {
                fit_start(if *on { "x" } else { " " }, self.width, ' ')
            }
            (ControlKind::Radio, ControlValue::Checked(on)) => {
                fit_start(if *on { "•" } else { " " }, self.width, ' ')
            }
            (ControlKind::Select { options }, ControlValue::Selected(i)) => {
                let label = options.get(*i).map(|o| o.label.as_str()).unwrap_or("");
                let inner = self.width.saturating_sub(2);
                format!("{} ▾", fit_start(label, inner, ' '))
            }
            _ => fit_start(&self.label, self.width, ' '),
        }
    }

    /// A short description for the status line
    pub fn describe(&self) -> String {
        let name = if self.name.is_empty() { String::new() } else { format!(" \"{}\"", self.name) };
        match &self.kind {
            ControlKind::Text { password: true } => format!("Password field{}", name),
            ControlKind::Text { .. } => format!("Text field{}", name),
            ControlKind::TextArea => format!("Text area{}", name),
            ControlKind::Checkbox => format!("Checkbox{}", name),
            ControlKind::Radio => format!("Option{}", name),
            ControlKind::Select { .. } => format!("Choice{}", name),
            ControlKind::Submit => format!("Button \"{}\"", self.label),
            ControlKind::Hidden => String::new(),
        }
    }
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

/// The start of `text`, padded with `pad` to exactly `width` columns
fn fit_start(text: &str, width: usize, pad: char) -> String {
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = char_width(ch);
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.extend(std::iter::repeat_n(pad, width - used));
    out
}

/// The end of `text`, padded with `pad` to exactly `width` columns
fn fit_end(text: &str, width: usize, pad: char) -> String {
    let mut kept: Vec<char> = Vec::new();
    let mut used = 0;
    for ch in text.chars().rev() {
        let w = char_width(ch);
        if used + w > width {
            break;
        }
        kept.push(ch);
        used += w;
    }
    let mut out: String = kept.into_iter().rev().collect();
    out.extend(std::iter::repeat_n(pad, width - used));
    out
}

/// A request that submits a form
#[derive(Debug, PartialEq)]
pub struct Submission {
    pub method: FormMethod,
    pub url: String,
    /// application/x-www-form-urlencoded body, for POST
    pub body: Option<String>,
}

/// Where a form sends its data, resolved against the page: (method, URL without data)
pub fn form_target(form: &Form, base_url: &str) -> String {
    let action = form.action.as_deref().map(str::trim).filter(|a| !a.is_empty());
    match action {
        Some(action) => crate::network::resolve_url(base_url, action),
        // No action: the form submits to the page itself
        None => base_url.to_string(),
    }
}

/// Build the request for submitting `form`, pressed with button `submitter` if any.
/// Follows the HTML rules for which controls are sent, simplified: named, in this form, and
/// checked or selected where that applies.
pub fn build_submission(
    forms: &[Form],
    controls: &[FormControl],
    values: &[ControlValue],
    form: usize,
    submitter: Option<usize>,
    base_url: &str,
) -> Result<Submission, String> {
    let form_info = forms.get(form).ok_or("That form no longer exists")?;
    let target = form_target(form_info, base_url);
    if let Some(scheme) = crate::network::unsupported_link_scheme(&target) {
        return Err(format!("Not sent: forms can't be submitted to {}: addresses", scheme));
    }
    if let Some(host) = crate::network::hidden_destination(&target) {
        return Err(format!("Not sent: this form's address hides where it really goes ({})", host));
    }
    // A public page could otherwise make you send requests to your router, printer or local
    // services, many of which trust anything from inside the network (cross-site request forgery)
    if crate::network::is_local_network_url(&target) && !crate::network::is_local_network_url(base_url) {
        let host = url::Url::parse(&target).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        return Err(format!("Not sent: a public page's form can't send data to your local network ({})", host));
    }
    let mut url = url::Url::parse(&target).map_err(|_| format!("Not sent: invalid form address {}", target))?;
    url.set_fragment(None);

    let mut pairs = url::form_urlencoded::Serializer::new(String::new());
    let mut has_password = false;
    for (i, control) in controls.iter().enumerate() {
        if control.form != Some(form) || control.name.is_empty() {
            continue;
        }
        let value = values.get(i).cloned().unwrap_or_else(|| control.initial_value());
        match (&control.kind, value) {
            (ControlKind::Text { password }, ControlValue::Text(text)) => {
                has_password |= *password && !text.is_empty();
                pairs.append_pair(&control.name, &text);
            }
            (ControlKind::TextArea, ControlValue::Text(text)) => {
                // Line breaks are sent as CRLF
                pairs.append_pair(&control.name, &text.replace("\r\n", "\n").replace('\n', "\r\n"));
            }
            (ControlKind::Checkbox | ControlKind::Radio, ControlValue::Checked(true)) => {
                let sent = if control.value.is_empty() { "on" } else { control.value.as_str() };
                pairs.append_pair(&control.name, sent);
            }
            (ControlKind::Select { options }, ControlValue::Selected(selected)) => {
                if let Some(option) = options.get(selected) {
                    pairs.append_pair(&control.name, &option.value);
                }
            }
            (ControlKind::Hidden, _) => {
                pairs.append_pair(&control.name, &control.value);
            }
            (ControlKind::Submit, _) if submitter == Some(i) => {
                pairs.append_pair(&control.name, &control.value);
            }
            _ => {}
        }
    }
    let encoded = pairs.finish();

    // Gemini and Gopher take the text typed as the whole query: gemini://host/path?words, or
    // gopher://host/7selector%09words for a Gopher search
    if crate::network::is_small_web(url.as_str()) {
        let text = controls
            .iter()
            .zip(values)
            .find(|(c, _)| c.form == Some(form) && c.is_text())
            .and_then(|(_, v)| match v {
                ControlValue::Text(text) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let words: String = percent_encoding::utf8_percent_encode(&text, QUERY).to_string();
        if url.scheme() == "gopher" {
            let path = format!("{}%09{}", url.path(), words);
            url.set_path(&path);
        } else {
            url.set_query(Some(&words));
        }
        return Ok(Submission { method: FormMethod::Get, url: url.to_string(), body: None });
    }

    match form_info.method {
        FormMethod::Get => {
            if has_password {
                // It would end up in the address bar, history and server logs
                return Err("Not sent: this form would put your password in the web address".to_string());
            }
            url.set_query(if encoded.is_empty() { None } else { Some(&encoded) });
            Ok(Submission { method: FormMethod::Get, url: url.to_string(), body: None })
        }
        FormMethod::Post => Ok(Submission { method: FormMethod::Post, url: url.to_string(), body: Some(encoded) }),
    }
}

/// The button pressed when a form is submitted by pressing Enter in a text field: its first
/// submit button, as in other browsers
pub fn default_submitter(controls: &[FormControl], form: usize) -> Option<usize> {
    controls
        .iter()
        .position(|c| c.form == Some(form) && c.kind == ControlKind::Submit)
}
