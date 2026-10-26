//! Site menus: lists of short links drawn on one line (`Home · News · Sport`), with submenus
//! on labelled lines.

use super::*;

/// Words in class names that mark a list as a menu or a horizontal list (e.g. "nav-links",
/// "vector-menu-content-list", MediaWiki's "hlist")
const MENU_CLASS_WORDS: &[&str] =
    &["nav", "navbar", "navigation", "menu", "menubar", "hlist", "breadcrumb", "breadcrumbs", "pagination", "toolbar"];

/// Menu items longer than this are content, not navigation
const MAX_MENU_ITEM_CHARS: usize = 60;

/// Submenus deeper than this are drawn as ordinary lists
const MAX_MENU_DEPTH: usize = 8;

/// The words in a class attribute, split at separators and camelCase humps:
/// "NoJsNavigation-styles__List" gives no, js, navigation, styles, list
fn class_words(classes: &str) -> Vec<String> {
    let mut words = Vec::new();
    for part in classes.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut word = String::new();
        let mut previous_lower = false;
        for c in part.chars() {
            if c.is_ascii_uppercase() && previous_lower && !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            previous_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
            word.push(c.to_ascii_lowercase());
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    words
}

fn has_menu_class(elem: &scraper::node::Element) -> bool {
    elem.attr("class")
        .is_some_and(|classes| class_words(classes).iter().any(|w| MENU_CLASS_WORDS.contains(&w.as_str())))
}

fn is_navigation_region(elem: &scraper::node::Element) -> bool {
    matches!(elem.name(), "nav" | "footer") || matches!(elem.attr("role"), Some("navigation" | "menubar" | "menu"))
}

/// A list is shown as a menu if it's in a navigation region (<nav>, <footer>, role=navigation)
/// or marked as a menu by its class. Numbered lists only count inside navigation regions.
pub(super) fn looks_like_menu(list: ElementRef) -> bool {
    // Bounded, so pathologically deep nesting can't make this quadratic
    let in_navigation = is_navigation_region(list.value())
        || list.ancestors().filter_map(ElementRef::wrap).take(50).any(|a| is_navigation_region(a.value()));
    if list.value().name() == "ol" {
        return in_navigation;
    }
    in_navigation
        || has_menu_class(list.value())
        || list.ancestors().filter_map(ElementRef::wrap).take(2).any(|a| has_menu_class(a.value()))
        || lays_out_in_a_row(list.value())
}

/// Whether a list's own classes lay its items out side by side, as utility CSS frameworks write
/// it: "flex", "inline-flex", "lg:flex lg:flex-row" (but not "flex-col"). Its items still have
/// to be short links or labels for it to be drawn as a menu.
fn lays_out_in_a_row(list: &scraper::node::Element) -> bool {
    let Some(classes) = list.attr("class") else { return false };
    let words = class_words(classes);
    let has = |w: &str| words.iter().any(|word| word == w);
    (has("flex") || has("inline")) && !has("col") && !has("column")
}

/// One entry of a menu: a link (or a plain label) and its submenu, if any
pub(super) struct MenuItem<'a> {
    pub(super) label: String,
    pub(super) href: Option<String>,
    pub(super) submenu: Option<ElementRef<'a>>,
    /// An item holding a form control (e.g. a button) is drawn as itself, on the menu's line
    pub(super) with_control: Option<ElementRef<'a>>,
}

/// Form controls, which a menu item may hold one of instead of a link
const CONTROL_TAGS: &[&str] = &["button", "input", "select", "textarea"];

/// What a list item holds, if it could be a menu entry: its visible text outside any submenu,
/// its links outside the submenu, and the submenu. None as soon as it holds too much text or more
/// than one link. The scan never enters the submenu or hidden content and stops early, so each item
/// costs a small, bounded amount of work however large or deeply nested the page is.
fn scan_menu_item(li: ElementRef) -> Option<(String, Option<ElementRef>, Option<ElementRef>, bool)> {
    // The submenu is a list near the top of the item (li > ul, or li > div > ul)
    let mut submenu = None;
    let mut frontier: Vec<ElementRef> = vec![li];
    for _ in 0..3 {
        let mut next = Vec::new();
        for element in frontier {
            for child in element.children().filter_map(ElementRef::wrap) {
                if submenu.is_none() && matches!(child.value().name(), "ul" | "ol" | "menu") {
                    submenu = Some(child);
                }
                next.push(child);
            }
        }
        if submenu.is_some() || next.is_empty() {
            break;
        }
        frontier = next;
    }

    let mut text = String::new();
    let mut link = None;
    let mut control = false;
    let mut stack = vec![*li];
    while let Some(node) = stack.pop() {
        match node.value() {
            Node::Text(t) => {
                text.push_str(&t.text);
                text.push(' ');
                if text.len() > MAX_MENU_ITEM_CHARS * 4 {
                    return None;
                }
            }
            Node::Element(elem) => {
                let skip = submenu.is_some_and(|s| s.id() == node.id())
                    || SKIPPED_TAGS.contains(&elem.name())
                    || is_hidden(elem);
                if skip {
                    continue;
                }
                let is_link = elem.name() == "a" && elem.attr("href").is_some();
                let is_control = CONTROL_TAGS.contains(&elem.name()) && elem.attr("type") != Some("hidden");
                if is_link || is_control {
                    if link.is_some() || control {
                        // Several links in one item is a sentence or a compound entry
                        return None;
                    }
                    if is_link {
                        link = ElementRef::wrap(node);
                    } else {
                        control = true;
                    }
                }
                // Children in reverse, so text comes out in document order
                let children: Vec<_> = node.children().collect();
                stack.extend(children.into_iter().rev());
            }
            _ => {}
        }
    }
    // Also trims non-breaking spaces at the ends, which menus sometimes use for padding
    let text = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_matches(char::is_whitespace).to_string();
    (text.chars().count() <= MAX_MENU_ITEM_CHARS).then_some((text, link, submenu, control))
}

/// The visible text of a link in a menu
fn link_label(link: ElementRef) -> String {
    let text = link.text().collect::<String>();
    let text = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.trim_matches(char::is_whitespace).to_string();
    if text.is_empty() {
        // An icon link: use its accessible name
        link.value().attr("aria-label").or_else(|| link.value().attr("title")).unwrap_or("").to_string()
    } else {
        text
    }
}

/// The entries of a list if every item is a short link or label (with an optional submenu), as
/// in a site menu; None if it holds anything more, which makes it content, or has fewer than
/// `min_items` entries
pub(super) fn menu_items(list: ElementRef, min_items: usize) -> Option<Vec<MenuItem>> {
    let mut items: Vec<MenuItem> = Vec::new();
    for li in list.children().filter_map(ElementRef::wrap) {
        // Some sites put a submenu beside its item instead of inside it: <li>News</li><ul>...</ul>
        if matches!(li.value().name(), "ul" | "ol" | "menu") {
            let has_items = li.children().filter_map(ElementRef::wrap).any(|c| c.value().name() == "li");
            if let Some(last) = items.last_mut().filter(|item| item.submenu.is_none() && has_items) {
                last.submenu = Some(li);
            }
            continue;
        }
        if li.value().name() != "li" || is_hidden(li.value()) {
            continue;
        }
        let (label, link, submenu, control) = scan_menu_item(li)?;
        match link {
            None if control => items.push(MenuItem { label, href: None, submenu, with_control: Some(li) }),
            None if label.is_empty() && submenu.is_none() => continue,
            None => items.push(MenuItem { label, href: None, submenu, with_control: None }),
            Some(link) => {
                let text = link_label(link);
                if text.is_empty() {
                    continue;
                }
                let href = link.value().attr("href").map(str::to_string);
                items.push(MenuItem { label: text, href, submenu, with_control: None });
            }
        }
    }
    (items.len() >= min_items.max(1)).then_some(items)
}
impl DomRenderer {
    /// A menu as one wrapped line, "Home · News · Sport", followed by a line per submenu headed
    /// by its parent, "News: US & Canada · UK · Africa"
    pub(super) fn render_menu(&mut self, items: &[MenuItem], heading: Option<&str>, depth: usize) {
        // Menus count towards the nesting limit, so menus nested in menus can't recurse without
        // bound (each level may fall back to walking a submenu, which can start a new menu)
        self.depth += 1;
        self.render_menu_lines(items, heading, depth);
        self.depth -= 1;
    }

    fn render_menu_lines(&mut self, items: &[MenuItem], heading: Option<&str>, depth: usize) {
        self.flush_line();
        if let Some(heading) = heading {
            self.add_style(|s| s.add_modifier(Modifier::BOLD));
            self.push_word(&format!("{}:", sanitize_for_terminal(heading)));
            self.style_stack.pop();
            self.note_space();
        }
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                if self.current_line_width + 2 > self.max_width {
                    // No room for the separator: the line break separates the items instead
                    self.flush_line();
                } else {
                    self.add_style(|s| s.fg(Color::DarkGray));
                    self.push_word("·");
                    self.style_stack.pop();
                    self.note_space();
                }
            }
            match (&item.href, item.with_control) {
                (Some(href), _) => self.push_link_text(&item.label, href),
                (None, Some(li)) => {
                    // The item's own content, so its control works: walked on this line
                    self.inline_depth += 1;
                    for child in li.children() {
                        if item.submenu.is_none_or(|s| s.id() != child.id()) {
                            self.walk(child);
                        }
                    }
                    self.inline_depth -= 1;
                }
                (None, None) => self.push_text(&item.label),
            }
            self.note_space();
        }
        self.flush_line();

        for item in items {
            let Some(submenu) = item.submenu else {
                continue;
            };
            match menu_items(submenu, 1) {
                Some(subitems) if depth < MAX_MENU_DEPTH => self.render_menu(&subitems, Some(&item.label), depth + 1),
                // Not a plain menu (or nested too deeply): draw it as usual, unless it's empty
                _ if submenu.text().any(|t| !t.trim().is_empty()) => self.walk(*submenu),
                _ => {}
            }
        }
    }
}
