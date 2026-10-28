//! Key bindings: which keys do what while browsing, changeable in keys.conf.
//!
//! Each action has default keys. keys.conf can add keys to an action (`reload = F5`), give it
//! only the keys listed (`reload = F5 only`), or none (`reload = none`). A pressed key is
//! translated to its action's first default key before the browsing keys are handled, so the
//! handling itself doesn't change. Typing (into fields, the address bar, find) and copy mode
//! keep their own keys.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use std::collections::HashMap;
use std::path::Path;

pub const KEYS_FILE: &str = "keys.conf";

/// The page listing every action and its keys
pub const KEYS_PAGE: &str = "about:keys";

/// An action: its name in keys.conf, what it does, and its default keys (the first is the one
/// the browsing keys are handled as)
struct Action {
    name: &'static str,
    about: &'static str,
    defaults: &'static [&'static str],
}

const ACTIONS: &[Action] = &[
    Action { name: "scroll_down", about: "Scroll down a line", defaults: &["Down"] },
    Action { name: "scroll_up", about: "Scroll up a line", defaults: &["Up"] },
    Action { name: "page_down", about: "Scroll down a screen", defaults: &["PageDown", "Ctrl+f"] },
    Action { name: "page_up", about: "Scroll up a screen", defaults: &["PageUp", "Ctrl+b"] },
    Action { name: "half_page_down", about: "Scroll down half a screen", defaults: &["Ctrl+d"] },
    Action { name: "half_page_up", about: "Scroll up half a screen", defaults: &["Ctrl+u"] },
    Action { name: "top", about: "Top of the page", defaults: &["g", "Home"] },
    Action { name: "bottom", about: "Bottom of the page", defaults: &["G", "End"] },
    Action { name: "cursor_left", about: "Move the cursor left", defaults: &["h"] },
    Action { name: "cursor_right", about: "Move the cursor right", defaults: &["l"] },
    Action { name: "cursor_up", about: "Move the cursor up", defaults: &["k"] },
    Action { name: "cursor_down", about: "Move the cursor down", defaults: &["j"] },
    Action { name: "next_link", about: "Select the next link or field", defaults: &["Tab"] },
    Action { name: "previous_link", about: "Select the previous link or field", defaults: &["Shift+Tab"] },
    Action { name: "open", about: "Open the link, or use the field", defaults: &["Enter"] },
    Action { name: "back", about: "Back", defaults: &["Backspace", "Left", "H"] },
    Action { name: "forward", about: "Forward", defaults: &["Right", "L"] },
    Action { name: "reload", about: "Reload", defaults: &["r"] },
    Action { name: "bookmark", about: "Bookmark the page, or remove it", defaults: &["b"] },
    Action { name: "bookmarks", about: "Show bookmarks", defaults: &["B"] },
    Action { name: "history", about: "Show history", defaults: &["s"] },
    Action { name: "clear_history", about: "Clear history (on the history page, twice)", defaults: &["D"] },
    Action { name: "select_text", about: "Copy mode: select text to copy", defaults: &["v"] },
    Action { name: "download", about: "Download the selected link", defaults: &["d"] },
    Action { name: "dismiss", about: "Clear a finished download", defaults: &["Esc"] },
    Action { name: "new_tab", about: "New tab", defaults: &["t"] },
    Action { name: "open_in_new_tab", about: "Open the selected link in a new tab", defaults: &["F"] },
    Action { name: "close_tab", about: "Close the tab", defaults: &["w", "x"] },
    Action { name: "reopen_tab", about: "Reopen the tab closed last", defaults: &["X"] },
    Action { name: "next_tab", about: "Next tab", defaults: &["]", "K"] },
    Action { name: "previous_tab", about: "Previous tab", defaults: &["[", "J"] },
    Action { name: "quit", about: "Quit", defaults: &["q"] },
    Action { name: "edit_address", about: "Type an address or search", defaults: &["e", "o"] },
    Action { name: "address_in_new_tab", about: "Type an address or search, in a new tab", defaults: &["O"] },
    Action { name: "copy_address", about: "Copy the page's address", defaults: &["Y"] },
    Action { name: "find", about: "Find on the page", defaults: &["/"] },
    Action { name: "next_match", about: "Next match", defaults: &[">", "n"] },
    Action { name: "previous_match", about: "Previous match", defaults: &["<", "N"] },
    Action { name: "view_source", about: "Show the page source", defaults: &["V"] },
    Action { name: "reader", about: "Reader mode", defaults: &["R"] },
    Action { name: "i2p", about: "Switch I2P mode", defaults: &["p"] },
    Action { name: "settings", about: "Settings", defaults: &["S"] },
    Action { name: "archive", about: "The Internet Archive's copy of the page", defaults: &["A"] },
    Action { name: "track", about: "Track the site (snapshots)", defaults: &["T"] },
    Action { name: "snapshots", about: "Snapshots of the page", defaults: &["Z"] },
    Action { name: "load_images", about: "Load the page's images", defaults: &["I"] },
    Action { name: "view_image", about: "View an image full screen", defaults: &["i"] },
    Action { name: "updates", about: "Updates", defaults: &["U"] },
    Action { name: "cookies", about: "Let this site keep cookies (stay signed in), or forget them", defaults: &["C"] },
    Action { name: "help", about: "The start page, with the keys", defaults: &["?"] },
];

/// A key as bindings see it: the key, and Ctrl and Alt (Shift is in the character, or Shift+Tab)
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Key {
    code: KeyCode,
    ctrl: bool,
    alt: bool,
}

impl Key {
    fn from_event(event: KeyEvent) -> Key {
        let code = match event.code {
            // Some terminals report Shift+Tab as Tab with Shift
            KeyCode::Tab if event.modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            code => code,
        };
        Key {
            code,
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
        }
    }

    fn event(self) -> KeyEvent {
        let mut modifiers = KeyModifiers::empty();
        if self.ctrl {
            modifiers |= KeyModifiers::CONTROL;
        }
        if self.alt {
            modifiers |= KeyModifiers::ALT;
        }
        KeyEvent { code: self.code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::empty() }
    }

    /// A key written as in keys.conf: "r", "R", "Ctrl+f", "Alt+x", "F5", "Enter", "Shift+Tab"
    fn parse(text: &str) -> Option<Key> {
        let mut key = Key { code: KeyCode::Null, ctrl: false, alt: false };
        let mut rest = text.trim();
        loop {
            let lower = rest.to_ascii_lowercase();
            if let Some(after) = lower.strip_prefix("ctrl+").map(|_| &rest[5..]) {
                key.ctrl = true;
                rest = after;
            } else if let Some(after) = lower.strip_prefix("alt+").map(|_| &rest[4..]) {
                key.alt = true;
                rest = after;
            } else {
                break;
            }
        }
        key.code = match rest.to_ascii_lowercase().as_str() {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "shift+tab" | "backtab" => KeyCode::BackTab,
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" | "ins" => KeyCode::Insert,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" => KeyCode::PageDown,
            "space" => KeyCode::Char(' '),
            f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)) => {
                KeyCode::F(f[1..].parse().ok()?)
            }
            _ => {
                let mut chars = rest.chars();
                let c = chars.next()?;
                if chars.next().is_some() || c.is_control() {
                    return None;
                }
                // Ctrl+F and Ctrl+f are the same key
                KeyCode::Char(if key.ctrl { c.to_ascii_lowercase() } else { c })
            }
        };
        Some(key)
    }

    fn name(self) -> String {
        let base = match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{}", n),
            KeyCode::BackTab => "Shift+Tab".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            other => format!("{:?}", other),
        };
        format!("{}{}{}", if self.ctrl { "Ctrl+" } else { "" }, if self.alt { "Alt+" } else { "" }, base)
    }
}

/// The browsing keys in force
#[derive(Clone, Debug)]
pub struct Keymap {
    /// Each bound key, and the action (index into ACTIONS) it does
    bindings: HashMap<Key, usize>,
    /// What was wrong in keys.conf, shown on the keys page
    problems: Vec<String>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut bindings = HashMap::new();
        for (index, action) in ACTIONS.iter().enumerate() {
            for key in action.defaults {
                bindings.insert(Key::parse(key).expect("default keys parse"), index);
            }
        }
        Keymap { bindings, problems: Vec::new() }
    }
}

impl Keymap {
    /// Read keys.conf's text. Problems (unknown actions or keys) are returned as messages, and
    /// a key taken from one action by another is noted too.
    pub fn parse(text: &str) -> (Keymap, Vec<String>) {
        let mut keymap = Keymap::default();
        let mut problems = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, keys)) = line.split_once('=') else {
                problems.push(format!("line {}: expected action = keys", number + 1));
                continue;
            };
            let name = name.trim();
            let Some(index) = ACTIONS.iter().position(|a| a.name == name) else {
                problems.push(format!("line {}: unknown action \"{}\"", number + 1, name));
                continue;
            };
            let mut keys = keys.trim();
            let mut only = false;
            if keys.eq_ignore_ascii_case("none") {
                keys = "";
                only = true;
            } else if let Some(listed) = keys.strip_suffix(" only").or_else(|| keys.strip_suffix(" ONLY")) {
                keys = listed;
                only = true;
            }
            let parsed: Vec<Option<Key>> = keys.split(',').filter(|k| !k.trim().is_empty()).map(Key::parse).collect();
            if let Some(bad) = keys.split(',').filter(|k| !k.trim().is_empty()).zip(&parsed).find(|(_, k)| k.is_none())
            {
                problems.push(format!("line {}: unknown key \"{}\"", number + 1, bad.0.trim()));
                continue;
            }
            if only {
                keymap.bindings.retain(|_, action| *action != index);
            }
            for key in parsed.into_iter().flatten() {
                if let Some(previous) = keymap.bindings.insert(key, index)
                    && previous != index
                {
                    problems.push(format!(
                        "line {}: {} was {}, now {}",
                        number + 1,
                        key.name(),
                        ACTIONS[previous].name,
                        name
                    ));
                }
            }
        }
        keymap.problems = problems.clone();
        (keymap, problems)
    }

    /// Read keys.conf from `dir`, writing the commented template there if it's missing
    pub fn load_or_create(dir: &Path) -> (Keymap, Vec<String>) {
        let path = dir.join(KEYS_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => Keymap::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let _ = std::fs::write(&path, template());
                (Keymap::default(), Vec::new())
            }
            Err(e) => (Keymap::default(), vec![format!("couldn't read {}: {}", path.display(), e)]),
        }
    }

    /// The key a pressed key stands for: its action's first default key, as the browsing keys
    /// are handled. None if it isn't bound to anything.
    pub fn translate(&self, event: KeyEvent) -> Option<KeyEvent> {
        let index = *self.bindings.get(&Key::from_event(event))?;
        let canonical = Key::parse(ACTIONS[index].defaults[0]).expect("default keys parse");
        Some(canonical.event())
    }

    /// The keys bound to an action, by name
    pub fn keys_for(&self, name: &str) -> Vec<String> {
        let Some(index) = ACTIONS.iter().position(|a| a.name == name) else { return Vec::new() };
        let mut keys: Vec<String> = self.bindings.iter().filter(|(_, a)| **a == index).map(|(k, _)| k.name()).collect();
        // The defaults' order first, then the user's own, alphabetically
        let order = |k: &String| {
            ACTIONS[index].defaults.iter().position(|d| Key::parse(d).map(|d| d.name()).as_ref() == Some(k))
        };
        keys.sort_by_key(|k| (order(k).unwrap_or(usize::MAX), k.clone()));
        keys
    }

    /// Whether any binding differs from the defaults
    pub fn is_default(&self) -> bool {
        self.bindings == Keymap::default().bindings
    }

    /// The page listing every action and its keys
    pub fn page_html(&self) -> String {
        use crate::network::escape_html;
        let mut html = String::from(
            "<title>Keys</title><h1>Keys</h1><p>The keys for browsing. They can be changed in keys.conf, \
             in the settings folder: <b>reload = F5</b> adds a key, <b>reload = F5 only</b> replaces the \
             defaults, <b>reload = none</b> takes them away. Typing in fields, the address bar and find, \
             and copy mode, keep their own keys.</p>",
        );
        if !self.problems.is_empty() {
            html.push_str("<h2>Problems in keys.conf</h2><ul>");
            for problem in &self.problems {
                html.push_str(&format!("<li>{}</li>", escape_html(problem)));
            }
            html.push_str("</ul>");
        }
        html.push_str("<table><tr><th>Action</th><th>Keys</th><th>Does</th></tr>");
        for action in ACTIONS {
            let keys = self.keys_for(action.name);
            let keys = if keys.is_empty() { "(none)".to_string() } else { keys.join(", ") };
            html.push_str(&format!(
                "<tr><td>{}</td><td><b>{}</b></td><td>{}</td></tr>",
                action.name,
                escape_html(&keys),
                escape_html(action.about)
            ));
        }
        html + "</table>"
    }
}

/// keys.conf as written on first run: every action and its defaults, commented out
fn template() -> String {
    let mut text = String::from(
        "# rynx key bindings. Remove the # to change one:\n\
         #   reload = F5          adds F5 to reload's keys\n\
         #   reload = F5 only     makes F5 its only key\n\
         #   reload = none        takes its keys away\n\
         # Keys: a character (r, R, /), Ctrl+x, Alt+x, F1-F24, Enter, Esc, Tab, Shift+Tab, Backspace,\n\
         # Delete, Insert, Left, Right, Up, Down, Home, End, PageUp, PageDown, Space. Several: F5, Ctrl+r\n\
         # Typing in fields, the address bar and find, and copy mode, keep their own keys.\n\n",
    );
    for action in ACTIONS {
        text.push_str(&format!("# {}\n# {} = {}\n", action.about, action.name, action.defaults.join(", ")));
    }
    text
}
