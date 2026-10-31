//! Link hints: every link and field on screen labelled with a number, and typing the number
//! opens it. Numbers rather than letters, so they can be said ("follow 12") as well as typed.

use super::*;

/// The labels shown, and what's been typed so far
pub struct Hints {
    /// Open the link in a new tab, rather than in this one (fields can't be, so aren't labelled)
    pub new_tab: bool,
    pub typed: String,
    /// Each label's number, and the link region it stands for
    pub targets: Vec<(usize, usize)>,
}

/// What a typed key did to the hints
#[derive(Debug, PartialEq)]
pub enum HintResult {
    /// Waiting for more digits
    Waiting,
    /// This link region was chosen
    Chosen(usize),
    /// Cancelled, or nothing matched
    Cancelled,
}

impl Hints {
    /// Label what's on screen: the lines from `top`, `rows` of them. Fields only when the link
    /// opens here. A link wrapped onto the next line gets one label, on its first part.
    pub fn for_screen(regions: &[LinkRegion], top: usize, rows: usize, new_tab: bool) -> Hints {
        let mut targets = Vec::new();
        for (index, region) in regions.iter().enumerate() {
            if region.line_index < top || region.line_index >= top + rows {
                continue;
            }
            if new_tab && (region.control.is_some() || region.url.is_empty()) {
                continue;
            }
            let continues = index.checked_sub(1).and_then(|i| regions.get(i)).is_some_and(|before| {
                before.line_index + 1 == region.line_index
                    && before.url == region.url
                    && before.control == region.control
                    && before.image == region.image
                    && region.x_start == 0
            });
            if !continues {
                targets.push((targets.len() + 1, index));
            }
        }
        Hints { new_tab, typed: String::new(), targets }
    }

    /// The label drawn for each link region on screen, among those the typing still matches
    pub fn labels(&self) -> impl Iterator<Item = (String, usize)> + '_ {
        self.targets
            .iter()
            .map(|(number, region)| (number.to_string(), *region))
            .filter(|(label, _)| label.starts_with(&self.typed))
    }

    /// A digit typed: chosen once no longer label could still be meant (with 12 labels, "7"
    /// is 7 at once, but "1" waits for "12")
    pub fn digit(&mut self, digit: char) -> HintResult {
        self.typed.push(digit);
        let typed: usize = match self.typed.parse() {
            Ok(n) => n,
            Err(_) => return HintResult::Cancelled,
        };
        if self.labels().next().is_none() {
            return HintResult::Cancelled;
        }
        let longer_possible = typed.checked_mul(10).is_some_and(|n| n <= self.targets.len()) && typed > 0;
        if longer_possible { HintResult::Waiting } else { self.chosen() }
    }

    /// Enter: the label typed exactly, if there is one
    pub fn chosen(&self) -> HintResult {
        let typed: Option<usize> = self.typed.parse().ok();
        match self.targets.iter().find(|(number, _)| Some(*number) == typed) {
            Some((_, region)) => HintResult::Chosen(*region),
            None => HintResult::Cancelled,
        }
    }
}

impl App {
    /// Label the links on screen (f, or ; to open them in new tabs)
    pub fn show_hints(&mut self, rows: usize, new_tab: bool) {
        let tab = self.current_tab();
        let hints = Hints::for_screen(&tab.link_regions, tab.scroll, rows, new_tab);
        if hints.targets.is_empty() {
            tab.status_message =
                String::from(if new_tab { "No links on screen to open" } else { "No links or fields on screen" });
            return;
        }
        tab.status_message = format!(
            "{}: type its number (Esc cancels)",
            if new_tab { "Open which link in a new tab" } else { "Follow which link" }
        );
        tab.hints = Some(hints);
    }

    /// Stop showing the labels, saying why
    pub fn cancel_hints(&mut self, why: &str) {
        let tab = self.current_tab();
        tab.hints = None;
        tab.status_message = why.to_string();
    }
}
