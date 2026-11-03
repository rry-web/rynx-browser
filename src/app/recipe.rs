//! Recipe mode (c): the page's recipe a card at a time, for cooking with your hands busy or on
//! glasses. The first card is the recipe itself (what it makes, its times, its ingredients), then
//! one card for each step, with the step's picture where the site gives one. The pictures are
//! loaded as `i` loads an image: asked for, so whatever the image setting, and never a public
//! page's from the local network.
//!
//! n and p (and the keys for the next and previous link, so a remote's buttons work) move between
//! cards, j and k scroll a long one, i goes back to the ingredients, a digit to that step, and Esc
//! (or back) closes it. The voice controller says "next step" and so on through the control
//! socket's `recipe` command.

use super::images::{ImageStatus, fill_cells};
use super::*;
use crate::renderer::Recipe;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui_image::protocol::StatefulProtocol;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The demo recipe (pancakes), with a picture for each step: it and its pictures are in rynx
/// itself, so the demo works without a network
pub const DEMO_PAGE: &str = "about:recipe";
pub(super) const DEMO_HTML: &str = include_str!("../../assets/recipe/pancakes.html");
const DEMO_PICTURES: &str = "about:recipe-pictures/";

/// One of the demo's pictures, by its address
pub(super) fn demo_picture(url: &str) -> Option<&'static [u8]> {
    Some(match url.strip_prefix(DEMO_PICTURES)? {
        "dry.svg" => include_bytes!("../../assets/recipe/dry.svg"),
        "wet.svg" => include_bytes!("../../assets/recipe/wet.svg"),
        "stir.svg" => include_bytes!("../../assets/recipe/stir.svg"),
        "pan.svg" => include_bytes!("../../assets/recipe/pan.svg"),
        "bubbles.svg" => include_bytes!("../../assets/recipe/bubbles.svg"),
        "flip.svg" => include_bytes!("../../assets/recipe/flip.svg"),
        "stack.svg" => include_bytes!("../../assets/recipe/stack.svg"),
        _ => return None,
    })
}

/// A picture's address and the cells it was made to fill, and its drawing state
type Drawn = ((String, (u16, u16)), StatefulProtocol);

/// A recipe being shown
pub struct RecipeView {
    pub recipe: Arc<Recipe>,
    /// The tab it's from, and the address its pictures' addresses are relative to
    tab_id: usize,
    base: String,
    /// The card shown: 0 is the recipe and its ingredients, then one for each step
    pub card: usize,
    /// Rows of the card's text scrolled past, and how many could be (from the last frame)
    pub scroll: usize,
    max_scroll: AtomicUsize,
    /// The picture drawn: its address and cells, and its drawing state
    drawn: Mutex<Option<Drawn>>,
}

impl std::fmt::Debug for RecipeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecipeView").field("recipe", &self.recipe.name).field("card", &self.card).finish()
    }
}

impl RecipeView {
    /// Whether it's showing the recipe of the tab with this id
    pub fn is_on_tab(&self, tab_id: usize) -> bool {
        self.tab_id == tab_id
    }

    /// Cards in all: the recipe, then its steps
    pub fn cards(&self) -> usize {
        self.recipe.steps.len() + 1
    }

    /// Where the view is, as said aloud or shown: "Ingredients", "Step 3 of 8"
    pub fn position(&self) -> String {
        match self.card {
            0 => format!("Ingredients ({} steps)", self.recipe.steps.len()),
            n => format!("Step {} of {}", n, self.recipe.steps.len()),
        }
    }

    /// The card's picture's address, as written: a step's own, and the recipe's on the first card
    fn picture(&self, card: usize) -> Option<&str> {
        match card {
            0 => self.recipe.image.as_deref(),
            n => self.recipe.steps.get(n - 1)?.image.as_deref(),
        }
    }

    /// The card's text, as lines to wrap: (text, bold)
    fn text(&self, card: usize) -> Vec<(String, bool)> {
        let recipe = &self.recipe;
        let mut lines = Vec::new();
        if card == 0 {
            let times = [("Prep", &recipe.prep_time), ("Cook", &recipe.cook_time), ("Total", &recipe.total_time)];
            let facts: Vec<String> = recipe
                .yields
                .iter()
                .cloned()
                .chain(times.iter().filter_map(|(label, time)| time.as_ref().map(|t| format!("{label} {t}"))))
                .collect();
            if !facts.is_empty() {
                lines.push((facts.join(" · "), false));
            }
            if let Some(description) = &recipe.description {
                lines.push((description.clone(), false));
            }
            if !recipe.ingredients.is_empty() {
                lines.push(("Ingredients".to_string(), true));
                lines.extend(recipe.ingredients.iter().map(|i| (format!("• {i}"), false)));
            }
            return lines;
        }
        let Some(step) = recipe.steps.get(card - 1) else { return lines };
        if let Some(section) = &step.section {
            lines.push((section.clone(), true));
        }
        lines.push((step.text.clone(), false));
        lines
    }

    /// What the control socket answers with: where the view is, and the card's text
    pub fn describe(&self) -> String {
        let text: Vec<String> = self.text(self.card).into_iter().map(|(t, _)| t).collect();
        format!("{}: {}", self.position(), text.join(" "))
    }
}

fn text_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

/// Text cut into rows of at most `width` columns, at spaces where it can be
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        let needed = if row.is_empty() { text_width(word) } else { text_width(&row) + 1 + text_width(word) };
        if needed > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        // A word longer than the row is cut
        for c in word.chars() {
            if text_width(&row) + unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) > width {
                rows.push(std::mem::take(&mut row));
            }
            row.push(c);
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

impl App {
    /// The recipe view, if it's open on the active tab (one left open on another tab is closed)
    fn recipe_view_here(&mut self) -> Option<&mut RecipeView> {
        let id = self.tabs[self.active_tab_index].id;
        if self.recipe_view.as_ref().is_some_and(|v| v.tab_id != id) {
            self.recipe_view = None;
        }
        self.recipe_view.as_mut()
    }

    /// Say so when a page that just arrived has a recipe
    pub(super) fn hint_recipe(&mut self, index: usize) {
        if self.tabs[index].recipe.is_none() {
            return;
        }
        let key = self.keymap.keys_for("recipe").into_iter().next();
        let how = match (key, self.talk_enabled) {
            (Some(key), true) => format!("{key} (or say \"recipe\")"),
            (Some(key), false) => key,
            (None, true) => String::from("say \"recipe\""),
            (None, false) => String::from("the menu"),
        };
        self.tabs[index].status_message = format!("This page has a recipe: {how} shows it step by step");
    }

    /// c: show the page's recipe a card at a time (or close it)
    pub fn toggle_recipe(&mut self) {
        if self.recipe_view_here().is_some() {
            return self.close_recipe();
        }
        self.open_recipe();
    }

    pub fn open_recipe(&mut self) {
        if self.recipe_view_here().is_some() {
            return;
        }
        let tab = &self.tabs[self.active_tab_index];
        let Some(recipe) = tab.recipe.clone() else {
            self.current_tab().status_message = String::from("This page has no recipe rynx can read");
            return;
        };
        self.recipe_view = Some(RecipeView {
            recipe,
            tab_id: tab.id,
            base: tab.link_base().to_string(),
            card: 0,
            scroll: 0,
            max_scroll: AtomicUsize::new(0),
            drawn: Mutex::default(),
        });
        self.close_menu();
        self.current_tab().hints = None;
        self.show_card(0);
    }

    pub fn close_recipe(&mut self) {
        if self.recipe_view.take().is_some() {
            self.current_tab().status_message = String::from("Ready");
        }
    }

    /// The next card (or with a negative `step`, a previous one); false (and the status line says
    /// why) if there's none that way
    pub fn recipe_step(&mut self, step: isize) -> bool {
        let Some(view) = self.recipe_view_here() else { return false };
        let last = view.cards() - 1;
        let card = view.card.saturating_add_signed(step).min(last);
        if card == view.card {
            let note = if step > 0 { "That's the last step" } else { "That's the start of the recipe" };
            self.current_tab().status_message = note.to_string();
            return false;
        }
        self.show_card(card);
        true
    }

    /// Card `card`: 0 for the ingredients, a step's number for that step; false (and the status
    /// line says why) if there's no such step
    pub fn recipe_go(&mut self, card: usize) -> bool {
        let Some(view) = self.recipe_view_here() else { return false };
        if card >= view.cards() {
            let steps = view.recipe.steps.len();
            self.current_tab().status_message = format!("There's no step {card}: this recipe has {steps}");
            return false;
        }
        self.show_card(card);
        true
    }

    /// Whether the recipe view is open on the active tab
    pub fn recipe_open(&self) -> bool {
        self.recipe_view.as_ref().is_some_and(|v| v.tab_id == self.tabs[self.active_tab_index].id)
    }

    /// Scroll the card's text by `rows`
    pub fn recipe_scroll(&mut self, rows: isize) {
        let Some(view) = self.recipe_view_here() else { return };
        let max = view.max_scroll.load(Ordering::Relaxed);
        view.scroll = view.scroll.saturating_add_signed(rows).min(max);
    }

    /// Show a card: say where the view is, and load its picture and the next card's
    fn show_card(&mut self, card: usize) {
        let Some(view) = self.recipe_view.as_mut() else { return };
        view.card = card;
        view.scroll = 0;
        let position = view.position();
        let pictures: Vec<String> = [card, card + 1]
            .iter()
            .filter_map(|&c| view.picture(c))
            .map(|src| crate::network::resolve_url(&view.base, src))
            .collect();
        let keys = if card + 1 < self.recipe_view.as_ref().map_or(0, RecipeView::cards) {
            "n next, p previous"
        } else {
            "the last step; p previous"
        };
        self.current_tab().status_message = format!("{position} · {keys}, Esc closes");
        let index = self.active_tab_index;
        for url in pictures {
            let wanted = matches!(self.images.status(&url), ImageStatus::NotLoaded | ImageStatus::Dropped);
            if wanted && self.may_load(&url).is_ok() {
                self.fetch_image(index, url);
            }
        }
    }

    /// Draw the recipe view over `area` (the page's place)
    pub fn draw_recipe(&self, f: &mut ratatui::Frame, area: Rect) {
        let Some(view) = &self.recipe_view else { return };
        if view.tab_id != self.tabs[self.active_tab_index].id || area.width < 4 || area.height < 3 {
            return;
        }
        f.render_widget(Clear, area);
        let title = format!(" {} · {} ", view.recipe.name, view.position());
        let help = " n next · p previous · i ingredients · Esc closes ";
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                fit(&title, area.width.saturating_sub(2) as usize),
                Style::default().add_modifier(Modifier::BOLD),
            ))
            .title_bottom(fit(help, area.width.saturating_sub(2) as usize));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };

        // The text, wrapped to the card
        let mut rows: Vec<Line> = Vec::new();
        for (i, (text, bold)) in view.text(view.card).into_iter().enumerate() {
            if i > 0 && (bold || view.card == 0 && !text.starts_with('•')) {
                rows.push(Line::default());
            }
            let style = if bold { Style::default().add_modifier(Modifier::BOLD) } else { Style::default() };
            rows.extend(wrap(&text, inner.width as usize).into_iter().map(|r| Line::from(Span::styled(r, style))));
        }

        // The picture above it, as large as leaves room for the text, which gets at least half
        // the card when it's long (or a note while it loads)
        let picture = view.picture(view.card).map(|src| crate::network::resolve_url(&view.base, src));
        let mut top = 0u16;
        match picture.as_deref().map(|url| (url, self.images.status(url))) {
            Some((url, ImageStatus::Decoded(image))) => {
                let text_rows = if rows.is_empty() { 0 } else { rows.len().min(inner.height as usize / 2) as u16 + 1 };
                let room = inner.height.saturating_sub(text_rows).max(1);
                let font = self.images.picker.font_size();
                let cells = picture_cells(image.width(), image.height(), font, inner.width, room);
                let target =
                    Rect { x: inner.x + (inner.width - cells.0) / 2, y: inner.y, width: cells.0, height: cells.1 };
                let mut drawn = view.drawn.lock().unwrap_or_else(|e| e.into_inner());
                let key = (url.to_string(), cells);
                if drawn.as_ref().is_none_or(|(k, _)| *k != key) {
                    let filled = fill_cells(image, cells, self.images.picker.font_size());
                    *drawn = Some((key, self.images.picker.new_resize_protocol(filled)));
                }
                if let Some((_, protocol)) = drawn.as_mut() {
                    let widget = ratatui_image::StatefulImage::default().resize(ratatui_image::Resize::Scale(None));
                    f.render_stateful_widget(widget, target, protocol);
                }
                top = cells.1 + 1;
            }
            Some((url, ImageStatus::Dropped)) => {
                // Dropped from memory: decoded again after this frame
                self.images.missing.lock().unwrap_or_else(|e| e.into_inner()).insert(url.to_string());
                rows.insert(0, Line::from("[Loading the picture...]"));
            }
            Some((_, ImageStatus::Loading)) => rows.insert(0, Line::from("[Loading the picture...]")),
            Some((_, ImageStatus::Failed(reason))) => {
                rows.insert(0, Line::from(format!("[The picture couldn't be loaded: {reason}]")))
            }
            _ => {}
        }
        let text_area = Rect { y: inner.y + top, height: inner.height.saturating_sub(top), ..inner };
        let max_scroll = rows.len().saturating_sub(text_area.height as usize);
        view.max_scroll.store(max_scroll, Ordering::Relaxed);
        let shown: Vec<Line> = rows.into_iter().skip(view.scroll.min(max_scroll)).collect();
        f.render_widget(Paragraph::new(shown), text_area);
    }
}

/// The cells to draw a picture of `width` by `height` pixels in: as large as fits in `columns`
/// by `rows`, keeping its shape on a terminal whose cells are `font` pixels
fn picture_cells(width: u32, height: u32, font: ratatui_image::FontSize, columns: u16, rows: u16) -> (u16, u16) {
    let (fw, fh) = (font.width.max(1) as f64, font.height.max(1) as f64);
    let (iw, ih) = (width.max(1) as f64, height.max(1) as f64);
    let mut c = columns.max(1) as f64;
    let mut r = (c * fw * ih / iw / fh).round().max(1.0);
    if r > rows.max(1) as f64 {
        r = rows.max(1) as f64;
        c = (r * fh * iw / ih / fw).round().clamp(1.0, columns.max(1) as f64);
    }
    (c as u16, r as u16)
}

/// Text cut to `width` columns
fn fit(text: &str, width: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if text_width(&out) + unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) > width {
            break;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wraps_at_spaces_and_cuts_long_words() {
        assert_eq!(wrap("Whisk the eggs and milk", 10), ["Whisk the", "eggs and", "milk"]);
        assert_eq!(wrap("Supercalifragilistic", 8), ["Supercal", "ifragili", "stic"]);
        assert!(wrap("", 10).is_empty());
    }

    #[test]
    fn pictures_fit_the_room_left_keeping_their_shape() {
        let font = ratatui_image::FontSize::new(10, 20);
        // Wide: the width decides
        assert_eq!(picture_cells(400, 200, font, 40, 30), (40, 10));
        // Tall: the rows decide
        assert_eq!(picture_cells(200, 400, font, 40, 10), (10, 10));
    }
}
