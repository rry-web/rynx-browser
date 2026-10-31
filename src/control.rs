//! The control socket (--glasses, or --control-socket): another program on this computer, like
//! the voice controller in glasses/, drives rynx by naming actions rather than pressing keys.
//!
//! A Unix socket in a folder of its own in the settings folder, which only you can open (or where
//! RYNX_CONTROL_SOCKET says): one
//! command per line, and one line of JSON back for each, `{"ok": true, "message": ...}`. Any
//! program running as you could use it, and read the page with `read`; but such a program could
//! already type into the terminal rynx runs in, so nothing is opened that wasn't. It's off unless
//! rynx is started with one of those options.
//!
//! Commands:
//! - an action from keys.conf (`scroll_down`, `back`, `hints`, `next_tab`...): `actions` lists them
//! - `follow 12`, `follow_in_new_tab 12`: the link f (or ;) numbers 12
//! - `open words`: an address, or words to search for, as typed into the address bar
//! - `find words`: find on the page, and jump to the first match
//! - `type text`: typed into the field being typed in (or the address bar, or find); refused
//!   while browsing, where each letter would be a key
//! - `key Esc`: one key, as pressed (written as in keys.conf)
//! - `status`: the page's title and address, the status line and the mode
//! - `read`: the page's text from the top of the screen down, for reading aloud
//! - `heard words`: show in the status line what a voice controller heard
//! - `choose 3`: item 3 of the menu (m)
//! - `begin open`, `begin find`, `begin fill`: start typing an address or search, words to find,
//!   or into the page's text field (the first on screen, else on the page), which is emptied;
//!   `type` then types, `key Enter` sends, `key Esc` cancels
//! - `erase 5`: the last 5 characters typed taken back
//! - `talk on`, `talk off`: push-to-talk, as the talk key (Space) turns it on; `status` says
//!   `listening`

use crate::app::App;
use crate::event_handler::{follow_hint, handle_action, handle_key_event, run_menu_item};
use crate::models::InputMode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The socket's folder in the settings folder, and its name there
pub const SOCKET_FOLDER: &str = "control";
pub const SOCKET_NAME: &str = "rynx.sock";

/// Longest command taken: a line longer is refused, and the connection closed
pub const MAX_LINE: usize = 64 * 1024;

/// Most text `read` returns at once
const MAX_READ: usize = 6000;

/// A command from the socket, and where its answer goes
pub struct Request {
    pub line: String,
    pub reply: tokio::sync::oneshot::Sender<String>,
}

/// Another place for the socket (macOS allows socket paths of 104 bytes at most, which a long
/// home folder's settings folder can exceed)
pub const SOCKET_VARIABLE: &str = "RYNX_CONTROL_SOCKET";

/// Where the socket is: RYNX_CONTROL_SOCKET, or its folder in settings folder `config_dir`
pub fn socket_path(config_dir: &Path) -> PathBuf {
    match std::env::var_os(SOCKET_VARIABLE).filter(|p| !p.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => config_dir.join(SOCKET_FOLDER).join(SOCKET_NAME),
    }
}

/// Make the socket, in a folder only you can open. Done before the sandbox, which forbids it. A
/// socket left by a rynx that ended without removing it is replaced; one still answering means
/// another rynx is using it, which is an error.
#[cfg(unix)]
pub fn bind(path: &Path) -> std::io::Result<std::os::unix::net::UnixListener> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    let folder = path.parent().ok_or_else(|| std::io::Error::other("no folder for the socket"))?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(folder)?;
    // rynx's own folder, made earlier perhaps more open; a folder named in RYNX_CONTROL_SOCKET
    // is left as it is (it may be /tmp)
    if folder.file_name().is_some_and(|name| name == SOCKET_FOLDER) {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
    }
    if UnixStream::connect(path).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another rynx is using {}", path.display()),
        ));
    }
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Answer connections to the socket: each line becomes a Request, and its answer is written back
#[cfg(unix)]
pub fn serve(listener: std::os::unix::net::UnixListener) -> std::io::Result<tokio::sync::mpsc::Receiver<Request>> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::UnixListener::from_std(listener)?;
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(connection(stream, tx.clone()));
        }
    });
    Ok(rx)
}

#[cfg(unix)]
async fn connection(stream: tokio::net::UnixStream, requests: tokio::sync::mpsc::Sender<Request>) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    loop {
        let mut line = Vec::new();
        match (&mut reader).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let answer = if line.len() > MAX_LINE {
            error("command too long")
        } else {
            let (reply, answer) = tokio::sync::oneshot::channel();
            let line = String::from_utf8_lossy(&line).trim().to_string();
            if requests.send(Request { line, reply }).await.is_err() {
                return;
            }
            match answer.await {
                Ok(answer) => answer,
                Err(_) => return,
            }
        };
        let too_long = answer.contains("command too long");
        if writer.write_all(format!("{answer}\n").as_bytes()).await.is_err() || too_long {
            return;
        }
    }
}

fn error(message: &str) -> String {
    json!({ "ok": false, "message": message }).to_string()
}

/// Carry out one command. The answer, and whether rynx should quit.
pub fn run_command(app: &mut App, line: &str, width: u16, height: u16) -> (String, bool) {
    match command(app, line, width, height) {
        Ok((answer, quit)) => (answer.to_string(), quit),
        Err(message) => (error(&message), false),
    }
}

fn command(app: &mut App, line: &str, width: u16, height: u16) -> Result<(Value, bool), String> {
    let (name, rest) = line.split_once(' ').map(|(n, r)| (n, r.trim())).unwrap_or((line, ""));
    let failed = |e: std::io::Error| e.to_string();
    let done =
        |app: &mut App, quit: bool| Ok((json!({ "ok": true, "message": app.current_tab().status_message }), quit));
    match name {
        "" => Err("no command".into()),
        "actions" => {
            let actions: Vec<Value> = crate::keys::actions().map(|(n, about)| json!([n, about])).collect();
            Ok((json!({ "ok": true, "actions": actions }), false))
        }
        "status" => {
            let mode = format!("{:?}", app.current_tab().input_mode);
            let (tabs, index, listening) = (app.tabs.len(), app.active_tab_index, app.listening);
            let tab = app.current_tab();
            Ok((
                json!({
                    "ok": true,
                    "message": tab.status_message,
                    "title": tab.page_title,
                    "address": tab.page_url.clone().unwrap_or_else(|| tab.url_input.clone()),
                    "mode": if tab.hints.is_some() {
                        "Hints".to_string()
                    } else if tab.menu.is_some() {
                        "Menu".to_string()
                    } else {
                        mode
                    },
                    "listening": listening,
                    "tab": index + 1,
                    "tabs": tabs,
                }),
                false,
            ))
        }
        "read" => {
            let tab = app.current_tab();
            let mut text = String::new();
            for line in tab.rendered_content.iter().skip(tab.scroll) {
                let line = line.to_string();
                let line = line.trim();
                if text.len() + line.len() > MAX_READ {
                    break;
                }
                if !line.is_empty() {
                    text.push_str(line);
                    text.push('\n');
                }
            }
            Ok((json!({ "ok": true, "title": tab.page_title, "text": text }), false))
        }
        "heard" => {
            app.current_tab().status_message = format!("Heard: {}", rest);
            Ok((json!({ "ok": true }), false))
        }
        "follow" | "follow_in_new_tab" => {
            let number = rest.parse::<usize>().map_err(|_| format!("{name} needs a number"))?;
            back_to_browsing(app, width, height).map_err(failed)?;
            let quit = follow_hint(app, number, name == "follow_in_new_tab", width, height).map_err(failed)??;
            done(app, quit)
        }
        "choose" => {
            let number = rest.parse::<usize>().map_err(|_| "choose needs a number".to_string())?;
            let menu = app.current_tab().menu.as_ref().ok_or("No menu open: say \"menu\" first")?;
            let item =
                menu.choose(number).ok_or_else(|| format!("No item {} ({} in the menu)", number, menu.items.len()))?;
            let quit = run_menu_item(app, item, width, height).map_err(failed)?;
            done(app, quit)
        }
        // Off quietly, when the voice controller has heard its command: the status line keeps
        // what the command said
        "talk" => {
            match rest {
                "on" => app.set_listening(true),
                "off" => app.listening = false,
                "" => app.set_listening(!app.listening),
                _ => return Err("talk on, or talk off".into()),
            }
            Ok((json!({ "ok": true, "listening": app.listening, "message": app.current_tab().status_message }), false))
        }
        "begin" => {
            back_to_browsing(app, width, height).map_err(failed)?;
            match rest {
                "open" | "find" => {
                    let action = if rest == "open" { "edit_address" } else { "find" };
                    let key = crate::keys::action_key(action).expect("an action");
                    handle_action(app, key, width, height).map_err(failed)?;
                    if rest == "open" {
                        app.current_tab().url_input.clear();
                    }
                }
                "fill" => {
                    let rows = height.saturating_sub(crate::ui::chrome_rows()) as usize;
                    let region = text_field(app, rows).ok_or("No text field on this page to type into")?;
                    let tab = app.current_tab();
                    tab.selected_link_index = region;
                    let line = tab.link_regions[region].line_index;
                    if line < tab.scroll || line >= tab.scroll + rows {
                        tab.scroll_to_line(line, rows);
                    }
                    let open = crate::keys::action_key("open").expect("open is an action");
                    handle_action(app, open, width, height).map_err(failed)?;
                    if app.current_tab().input_mode != InputMode::FormEdit {
                        return Err("That field can't be typed into".into());
                    }
                    let clear = KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL);
                    handle_key_event(app, clear, width, height).map_err(failed)?;
                }
                _ => return Err("begin open, begin find, or begin fill".into()),
            }
            done(app, false)
        }
        "erase" => {
            let count = rest.parse::<usize>().map_err(|_| "erase needs a number".to_string())?;
            let tab = app.current_tab();
            if tab.input_mode == InputMode::Normal || tab.hints.is_some() || tab.menu.is_some() {
                return Err("Not typing: nothing to erase".into());
            }
            for _ in 0..count.min(MAX_LINE) {
                handle_key_event(app, KeyEvent::from(KeyCode::Backspace), width, height).map_err(failed)?;
            }
            done(app, false)
        }
        "open" | "find" if rest.is_empty() => Err(format!("{name} needs words")),
        "open" => {
            back_to_browsing(app, width, height).map_err(failed)?;
            let edit = crate::keys::action_key("edit_address").expect("edit_address is an action");
            handle_action(app, edit, width, height).map_err(failed)?;
            app.current_tab().url_input.clear();
            type_text(app, rest, width, height).map_err(failed)?;
            handle_key_event(app, KeyEvent::from(KeyCode::Enter), width, height).map_err(failed)?;
            // The page arrives later; the status line then says how it went
            Ok((json!({ "ok": true, "message": format!("Opening {}", app.current_tab().url_input) }), false))
        }
        "find" => {
            back_to_browsing(app, width, height).map_err(failed)?;
            let find = crate::keys::action_key("find").expect("find is an action");
            handle_action(app, find, width, height).map_err(failed)?;
            type_text(app, rest, width, height).map_err(failed)?;
            handle_key_event(app, KeyEvent::from(KeyCode::Enter), width, height).map_err(failed)?;
            let rows = height.saturating_sub(crate::ui::chrome_rows()) as usize;
            let tab = app.current_tab();
            if let Some(line) = tab.search_state.as_ref().and_then(|s| s.matches.first()).map(|m| m.line_index) {
                tab.scroll_to_line(line, rows);
            }
            done(app, false)
        }
        "type" => {
            // Only into something being typed in: while browsing, each letter is a key (q quits)
            let tab = app.current_tab();
            if tab.input_mode == InputMode::Normal || tab.hints.is_some() || tab.menu.is_some() {
                return Err("Not typing into a field: say \"links\" and the field's number first".into());
            }
            type_text(app, rest, width, height).map_err(failed)?;
            done(app, false)
        }
        "key" => {
            let key = crate::keys::parse_key(rest).ok_or_else(|| format!("unknown key \"{rest}\""))?;
            let quit = handle_key_event(app, key, width, height).map_err(failed)?;
            done(app, quit)
        }
        action => {
            let key = crate::keys::action_key(action).ok_or_else(|| format!("unknown command \"{action}\""))?;
            // With the menu open, the actions that step through links and choose work it
            if app.current_tab().menu.is_none() || !MENU_ACTIONS.contains(&action) {
                back_to_browsing(app, width, height).map_err(failed)?;
            }
            let quit = handle_action(app, key, width, height).map_err(failed)?;
            done(app, quit)
        }
    }
}

/// The page's text field to type into: the first on screen (of `rows`), else the first on the
/// page; never a password field
fn text_field(app: &mut App, rows: usize) -> Option<usize> {
    let tab = app.current_tab();
    let fields: Vec<usize> = tab
        .link_regions
        .iter()
        .enumerate()
        .filter(|(_, region)| {
            region.control.and_then(|c| tab.controls.get(c)).is_some_and(|c| {
                matches!(
                    c.kind,
                    crate::forms::ControlKind::Text { password: false } | crate::forms::ControlKind::TextArea
                )
            })
        })
        .map(|(i, _)| i)
        .collect();
    let on_screen = |&&i: &&usize| {
        let line = tab.link_regions[i].line_index;
        line >= tab.scroll && line < tab.scroll + rows
    };
    fields.iter().find(on_screen).or(fields.first()).copied()
}

/// Actions the open menu takes as its own (stepping, choosing, back), rather than closing first
const MENU_ACTIONS: &[&str] =
    &["next_link", "previous_link", "open", "back", "menu", "scroll_down", "scroll_up", "cursor_down", "cursor_up"];

/// Leave typing, find, copy mode, the menu or the numbered links, as Esc would, so an action does what it
/// does while browsing. Boxes asking a question (downloads, tools) stay until answered.
fn back_to_browsing(app: &mut App, width: u16, height: u16) -> std::io::Result<()> {
    let tab = app.current_tab();
    if tab.input_mode != InputMode::Normal || tab.hints.is_some() || tab.menu.is_some() {
        handle_key_event(app, KeyEvent::from(KeyCode::Esc), width, height)?;
    }
    Ok(())
}

/// Each character as a key press (line breaks left out: Enter would send a form)
fn type_text(app: &mut App, text: &str, width: u16, height: u16) -> std::io::Result<()> {
    for c in text.chars().filter(|c| !c.is_control()) {
        handle_key_event(app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), width, height)?;
    }
    Ok(())
}
