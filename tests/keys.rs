use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rynx::app::App;
use rynx::event_handler::handle_key_event;
use rynx::keys::Keymap;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::empty())
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn translated(keymap: &Keymap, event: KeyEvent) -> Option<(KeyCode, bool)> {
    keymap.translate(event).map(|k| (k.code, k.modifiers.contains(KeyModifiers::CONTROL)))
}

#[test]
fn the_defaults_are_the_keys_rynx_always_had() {
    let (keymap, problems) = Keymap::parse("");
    assert!(problems.is_empty());
    assert!(keymap.is_default());
    // Each key stands for itself, and the second keys of an action for its first
    assert_eq!(translated(&keymap, key(KeyCode::Char('r'))), Some((KeyCode::Char('r'), false)));
    assert_eq!(translated(&keymap, ctrl('f')), Some((KeyCode::PageDown, false)));
    assert_eq!(translated(&keymap, key(KeyCode::Home)), Some((KeyCode::Char('g'), false)));
    assert_eq!(translated(&keymap, ctrl('d')), Some((KeyCode::Char('d'), true)));
    // d alone is download, not half a page
    assert_eq!(translated(&keymap, key(KeyCode::Char('d'))), Some((KeyCode::Char('d'), false)));
    assert_eq!(translated(&keymap, key(KeyCode::F(5))), None);
}

#[test]
fn the_template_written_on_first_run_changes_nothing() {
    let dir = std::env::temp_dir().join(format!("rynx-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (first, problems) = Keymap::load_or_create(&dir);
    assert!(problems.is_empty() && first.is_default());
    let written = std::fs::read_to_string(dir.join(rynx::keys::KEYS_FILE)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(written.contains("# reload = r"), "{written}");
    let (again, problems) = Keymap::parse(&written);
    assert!(problems.is_empty(), "{problems:?}");
    assert!(again.is_default());
}

#[test]
fn keys_can_be_added_replaced_and_removed() {
    let (keymap, problems) =
        Keymap::parse("reload = F5, Ctrl+R\nquit = Ctrl+q only\nbookmark = none\n# a comment\n\nnext_tab = Alt+l");
    assert!(problems.is_empty(), "{problems:?}");
    assert!(!keymap.is_default());
    assert_eq!(translated(&keymap, key(KeyCode::F(5))), Some((KeyCode::Char('r'), false)));
    // Ctrl+R is written for Ctrl+r: terminals report it in lower case
    assert_eq!(translated(&keymap, ctrl('r')), Some((KeyCode::Char('r'), false)));
    assert_eq!(translated(&keymap, key(KeyCode::Char('r'))), Some((KeyCode::Char('r'), false)));
    assert_eq!(translated(&keymap, ctrl('q')), Some((KeyCode::Char('q'), false)));
    assert_eq!(translated(&keymap, key(KeyCode::Char('q'))), None);
    assert_eq!(translated(&keymap, key(KeyCode::Char('b'))), None);
    let alt_l = KeyEvent::new(KeyCode::Char('l'), KeyModifiers::ALT);
    assert_eq!(translated(&keymap, alt_l), Some((KeyCode::Char(']'), false)));
    assert_eq!(keymap.keys_for("reload"), vec!["r", "Ctrl+r", "F5"]);
    assert_eq!(keymap.keys_for("bookmark"), Vec::<String>::new());
}

#[test]
fn a_key_taken_from_another_action_is_noted() {
    let (keymap, problems) = Keymap::parse("reload = q");
    assert_eq!(problems, vec!["line 1: q was quit, now reload"]);
    assert_eq!(translated(&keymap, key(KeyCode::Char('q'))), Some((KeyCode::Char('r'), false)));
    assert!(keymap.keys_for("quit").is_empty());
}

#[test]
fn mistakes_are_reported_and_the_rest_still_applies() {
    let (keymap, problems) =
        Keymap::parse("relaod = F5\nreload = Hyper+x\nreload\nquit = F12 only\nback = Ctrl+Enter, abc");
    assert_eq!(
        problems,
        vec![
            "line 1: unknown action \"relaod\"",
            "line 2: unknown key \"Hyper+x\"",
            "line 3: expected action = keys",
            "line 5: unknown key \"abc\"",
        ]
    );
    // A line with a mistake changes nothing; the good lines apply
    assert_eq!(keymap.keys_for("back"), vec!["Backspace", "Left", "H"]);
    assert_eq!(keymap.keys_for("quit"), vec!["F12"]);
    let page = keymap.page_html();
    assert!(page.contains("Problems in keys.conf") && page.contains("&quot;relaod&quot;"), "{page}");
}

#[test]
fn shifted_keys_are_their_characters() {
    let keymap = Keymap::default();
    // Terminals send G with Shift; it's the same key as G
    let shift_g = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
    assert_eq!(translated(&keymap, shift_g), Some((KeyCode::Char('G'), false)));
    // Shift+Tab, however it arrives
    let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
    assert_eq!(translated(&keymap, shift_tab), Some((KeyCode::BackTab, false)));
    assert_eq!(translated(&keymap, key(KeyCode::BackTab)), Some((KeyCode::BackTab, false)));
}

#[tokio::test]
async fn the_browser_uses_the_keys_from_keys_conf() {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.keymap = Keymap::parse("quit = Ctrl+q only\nsettings = F2").0;
    // q no longer quits; Ctrl+q does
    assert!(!handle_key_event(&mut app, key(KeyCode::Char('q')), 80, 24).unwrap());
    assert!(handle_key_event(&mut app, ctrl('q'), 80, 24).unwrap());
    handle_key_event(&mut app, key(KeyCode::F(2)), 80, 24).unwrap();
    assert_eq!(app.current_tab().url_input, rynx::app::SETTINGS_PAGE);
    // Typing in the address bar keeps every key: q is a letter there
    handle_key_event(&mut app, key(KeyCode::Char('e')), 80, 24).unwrap();
    let before = app.current_tab().url_input.len();
    assert!(!handle_key_event(&mut app, key(KeyCode::Char('q')), 80, 24).unwrap());
    assert_eq!(app.current_tab().url_input.len(), before + 1);
}

#[tokio::test]
async fn the_keys_page_lists_the_keys_in_force() {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let mut app = App::new(tx, rx).unwrap();
    app.render_in_background = false;
    app.keymap = Keymap::parse("reload = F5").0;
    app.follow_link(rynx::keys::KEYS_PAGE.to_string());
    let tab = app.current_tab();
    assert_eq!(tab.url_input, rynx::keys::KEYS_PAGE);
    let text: String = tab.rendered_content.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("r, F5"), "{text}");
    // Pages can't change what a key does: there's nothing on the keys page to submit
    assert!(tab.controls.is_empty());
}
