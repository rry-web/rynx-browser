//! Form controls: reserved boxes on the page, which the UI fills with what the user enters.

use super::*;

impl DomRenderer {
    /// Register a form control. None if it's disabled, or outside any form, where only scripts
    /// could use it: such controls are drawn greyed out and can't be selected or sent.
    pub(super) fn add_control(&mut self, elem: &scraper::node::Element, mut control: FormControl) -> Option<usize> {
        if elem.attr("disabled").is_some() {
            return None;
        }
        // form="id" attaches a control to a form elsewhere on the page
        control.form = match elem.attr("form") {
            Some(id) => self.form_ids.get(id).copied(),
            None => self.current_form,
        };
        control.form?;
        self.controls.push(control);
        Some(self.controls.len() - 1)
    }

    /// Draw a control's box, e.g. "[text____]", as one unit that is clickable as the control
    fn push_control_box(&mut self, control: Option<usize>, text: &str) {
        let saved = self.active_control;
        self.active_control = control;
        let style =
            if control.is_some() { |s: Style| s.fg(Color::Yellow) } else { |s: Style| s.add_modifier(Modifier::DIM) };
        self.add_style(style);
        self.push_word(text);
        self.style_stack.pop();
        self.active_control = saved;
    }

    /// Draw a control with its initial contents, reserving `width` columns inside the brackets
    fn push_new_control(&mut self, elem: &scraper::node::Element, control: FormControl, open: &str, close: &str) {
        let inner = control.display(&control.initial_value(), false);
        let index = self.add_control(elem, control);
        self.push_control_box(index, &format!("{}{}{}", open, sanitize_for_terminal(&inner), close));
    }

    /// Room for a text field: its size attribute (default 20), within reason, and narrow enough
    /// to fit on a line with its brackets
    fn field_width(&self, requested: Option<&str>, default: usize) -> usize {
        let wanted = requested.and_then(|n| n.trim().parse::<usize>().ok()).unwrap_or(default);
        wanted.clamp(4, 40).min(self.max_width.saturating_sub(2).max(4))
    }

    pub(super) fn render_input(&mut self, elem: &scraper::node::Element) {
        let kind = elem.attr("type").unwrap_or("text").trim().to_ascii_lowercase();
        let control = |kind: ControlKind, width: usize, label: &str| FormControl {
            form: None,
            kind,
            name: elem.attr("name").unwrap_or("").to_string(),
            value: elem.attr("value").unwrap_or("").to_string(),
            checked: elem.attr("checked").is_some(),
            selected: 0,
            width,
            label: sanitize_for_terminal(label).into_owned(),
            max_length: elem.attr("maxlength").and_then(|n| n.trim().parse().ok()),
        };
        match kind.as_str() {
            "hidden" => {
                self.add_control(elem, control(ControlKind::Hidden, 0, ""));
            }
            "submit" | "image" => {
                let label = elem
                    .attr("value")
                    .or_else(|| elem.attr("alt"))
                    .filter(|l| !l.trim().is_empty())
                    .unwrap_or("Submit")
                    .trim();
                let width = label.chars().map(char_width).sum::<usize>() + 2;
                self.push_new_control(elem, control(ControlKind::Submit, width, &format!(" {} ", label)), "[", "]");
            }
            // Without JavaScript these do nothing
            "button" | "reset" => {
                let label = elem.attr("value").unwrap_or(if kind == "reset" { "Reset" } else { "" }).trim();
                if !label.is_empty() {
                    self.push_control_box(None, &format!("[ {} ]", sanitize_for_terminal(label)));
                }
            }
            "checkbox" => self.push_new_control(elem, control(ControlKind::Checkbox, 1, ""), "[", "]"),
            "radio" => self.push_new_control(elem, control(ControlKind::Radio, 1, ""), "(", ")"),
            "file" => self.push_control_box(None, "[file upload not supported]"),
            other => {
                let width = self.field_width(elem.attr("size"), 20);
                let kind = ControlKind::Text { password: other == "password" };
                self.push_new_control(elem, control(kind, width, elem.attr("placeholder").unwrap_or("")), "[", "]");
            }
        }
    }

    pub(super) fn render_textarea(&mut self, element: ElementRef) {
        let elem = element.value();
        let width = self.field_width(elem.attr("cols"), 30);
        let control = FormControl {
            form: None,
            kind: ControlKind::TextArea,
            name: elem.attr("name").unwrap_or("").to_string(),
            value: element.text().collect(),
            checked: false,
            selected: 0,
            width,
            label: sanitize_for_terminal(elem.attr("placeholder").unwrap_or("")).into_owned(),
            max_length: elem.attr("maxlength").and_then(|n| n.trim().parse().ok()),
        };
        self.push_new_control(elem, control, "[", "]");
    }

    pub(super) fn render_select(&mut self, element: ElementRef) {
        let elem = element.value();
        let mut options = Vec::new();
        let mut selected = 0;
        for option in element.descendants().filter_map(ElementRef::wrap) {
            if option.value().name() != "option" || option.value().attr("disabled").is_some() {
                continue;
            }
            let label: String =
                option.text().collect::<String>().split_ascii_whitespace().collect::<Vec<_>>().join(" ");
            let value = option.value().attr("value").map(str::to_string).unwrap_or_else(|| label.clone());
            if option.value().attr("selected").is_some() {
                selected = options.len();
            }
            options.push(SelectOption { value, label: sanitize_for_terminal(&label).into_owned() });
        }
        let widest = options.iter().map(|o| o.label.chars().map(char_width).sum::<usize>()).max().unwrap_or(0);
        let width = (widest + 2).clamp(4, 40).min(self.max_width.saturating_sub(2).max(4));
        let control = FormControl {
            form: None,
            kind: ControlKind::Select { options },
            name: elem.attr("name").unwrap_or("").to_string(),
            value: String::new(),
            checked: false,
            selected,
            width,
            label: String::new(),
            max_length: None,
        };
        self.push_new_control(elem, control, "[", "]");
    }
}
