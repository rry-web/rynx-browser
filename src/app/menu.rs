//! The menu (m): a short numbered list of what can be done here, chosen by its number, or by
//! stepping to it and choosing. What's listed depends on what's selected: a link, a field, or
//! neither; more is a step away under "Page..." and "Tabs...". Meant for glasses, where keys
//! aren't to hand: a few buttons (next, previous, choose, back), or a number said aloud, reach
//! everything, and nothing needs remembering.

use super::*;

/// One choice: what it says, and the action it runs (an action from keys.conf), or another menu
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuItem {
    pub label: &'static str,
    pub action: MenuAction,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
    Run(&'static str),
    Open(MenuKind),
    /// Push-to-talk: the voice controller listens for one command
    Talk,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuKind {
    Main,
    Page,
    Tabs,
}

pub struct Menu {
    pub kind: MenuKind,
    pub title: String,
    pub items: Vec<MenuItem>,
    /// The item stepped to (from 0)
    pub selected: usize,
    /// Digits typed so far, as with link hints
    pub typed: String,
}

const fn run(label: &'static str, action: &'static str) -> MenuItem {
    MenuItem { label, action: MenuAction::Run(action) }
}

const PAGE: MenuItem = MenuItem { label: "Page...", action: MenuAction::Open(MenuKind::Page) };
const TABS: MenuItem = MenuItem { label: "Tabs...", action: MenuAction::Open(MenuKind::Tabs) };
const TALK: MenuItem = MenuItem { label: "Talk (one voice command)", action: MenuAction::Talk };

/// What a key did to the menu
#[derive(Debug, PartialEq)]
pub enum MenuResult {
    Waiting,
    Chosen(MenuItem),
    Closed,
}

impl Menu {
    /// The menu of this kind for the tab as it is: `link` is the selected link region if it's on
    /// screen
    pub fn new(kind: MenuKind, link: Option<&LinkRegion>, talk: bool) -> Menu {
        let (title, mut items) = match kind {
            MenuKind::Main => match link {
                Some(region) if region.control.is_some() || region.url.is_empty() => (
                    "Field".to_string(),
                    vec![
                        run("Use the field", "open"),
                        run("Next link or field", "next_link"),
                        run("Previous link or field", "previous_link"),
                        run("Number the links", "hints"),
                        run("Back", "back"),
                    ],
                ),
                Some(region) => (
                    format!("Link: {}", crate::renderer::sanitize_for_terminal(&region.url)),
                    vec![
                        run("Open the link", "open"),
                        run("Open it in a new tab", "open_in_new_tab"),
                        run("Next link", "next_link"),
                        run("Previous link", "previous_link"),
                        run("Number the links", "hints"),
                        run("Back", "back"),
                    ],
                ),
                None => (
                    "Menu".to_string(),
                    vec![
                        run("Number the links", "hints"),
                        run("Next link", "next_link"),
                        run("Go to an address or search", "edit_address"),
                        run("Back", "back"),
                        run("Forward", "forward"),
                    ],
                ),
            },
            MenuKind::Page => (
                "Page".to_string(),
                vec![
                    run("Find on the page", "find"),
                    run("Reader mode on or off", "reader"),
                    run("Reload", "reload"),
                    run("Top of the page", "top"),
                    run("Bookmark it, or remove it", "bookmark"),
                    run("Bookmarks", "bookmarks"),
                    run("History", "history"),
                    run("Load the images", "load_images"),
                    run("Keys and help", "help"),
                ],
            ),
            MenuKind::Tabs => (
                "Tabs".to_string(),
                vec![
                    run("New tab", "new_tab"),
                    run("Next tab", "next_tab"),
                    run("Previous tab", "previous_tab"),
                    run("Close the tab", "close_tab"),
                    run("Reopen the tab closed last", "reopen_tab"),
                    run("Quit rynx", "quit"),
                ],
            ),
        };
        if kind == MenuKind::Main {
            items.extend([PAGE, TABS]);
            if talk {
                items.push(TALK);
            }
        }
        Menu { kind, title, items, selected: 0, typed: String::new() }
    }

    pub fn step(&mut self, forward: bool) {
        let n = self.items.len();
        self.selected = if forward { (self.selected + 1) % n } else { (self.selected + n - 1) % n };
        self.typed.clear();
    }

    /// A digit typed: chosen once no longer number could still be meant (as with link hints)
    pub fn digit(&mut self, digit: char) -> MenuResult {
        self.typed.push(digit);
        match self.typed.parse::<usize>() {
            Ok(n) if n >= 1 && n <= self.items.len() => {
                self.selected = n - 1;
                if n * 10 <= self.items.len() { MenuResult::Waiting } else { MenuResult::Chosen(self.items[n - 1]) }
            }
            _ => {
                self.typed.clear();
                MenuResult::Waiting
            }
        }
    }

    /// Item `number` (from 1)
    pub fn choose(&self, number: usize) -> Option<MenuItem> {
        number.checked_sub(1).and_then(|i| self.items.get(i)).copied()
    }
}

impl App {
    /// The selected link region, if it's among the `rows` on screen
    fn selected_on_screen(&mut self, rows: usize) -> Option<LinkRegion> {
        let tab = self.current_tab();
        let region = tab.link_regions.get(tab.selected_link_index)?;
        (region.line_index >= tab.scroll && region.line_index < tab.scroll + rows).then(|| region.clone())
    }

    /// Show a menu (m, for the main one)
    pub fn show_menu(&mut self, kind: MenuKind, rows: usize) {
        let link = self.selected_on_screen(rows);
        let menu = Menu::new(kind, link.as_ref(), self.talk_enabled);
        let tab = self.current_tab();
        tab.hints = None;
        tab.status_message = format!("{}: its number, or step to it and choose (Esc closes)", menu.title);
        tab.menu = Some(menu);
    }

    /// Push-to-talk (Space, or the menu's Talk): the voice controller, which asks for this
    /// through the control socket, listens until it hears a command, then turns it off
    pub fn set_listening(&mut self, on: bool) {
        if on && !self.talk_enabled {
            self.current_tab().status_message =
                String::from("No voice controller: talking needs glasses mode (scripts/rynx-glasses)");
            return;
        }
        self.listening = on;
        self.current_tab().status_message =
            String::from(if on { "Listening: say a command (Space stops)" } else { "Stopped listening" });
    }

    pub fn close_menu(&mut self) {
        let tab = self.current_tab();
        if tab.menu.take().is_some() {
            tab.status_message = String::from("Ready");
        }
    }
}
