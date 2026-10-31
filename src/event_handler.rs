use crate::app::App;
use crate::constants::*;
use crate::constants::{MOUSE_SCROLL_LINES, UI_HEIGHT_OFFSET, UI_ROW_OFFSET};
use crate::models::{DownloadStatus, InputMode};
use crate::network::NetworkResponse;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::io::Result;

/// Whether a clicked link should go straight to the download prompt: its file name has an
/// extension for something that can't be shown as a page. Other links are opened, and a file that
/// turns out not to be displayable gets a download page anyway.
fn is_downloadable_file(url: &str) -> bool {
    const BINARY_EXTENSIONS: &[&str] = &[
        "zip", "pdf", "exe", "dmg", "pkg", "deb", "iso", "mp4", "mp3", "png", "jpg", "jpeg", "gif", "docx", "xlsx",
        "tar", "gz", "ogg",
    ];
    // Only the path's file name counts, not the host or a query like ?file=a.zip
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let file_name = parsed.path_segments().and_then(|mut s| s.next_back()).unwrap_or("");
    file_name.rsplit_once('.').is_some_and(|(_, ext)| BINARY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Rows available for page content: tab bar (3) + URL bar (3) + content borders (2) are subtracted
fn viewport_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(UI_HEIGHT_OFFSET) as usize
}

pub fn handle_key_event(app: &mut App, key: KeyEvent, terminal_width: u16, terminal_height: u16) -> Result<bool> {
    // Some terminals (and stty settings, e.g. on openSUSE) send ^H for Backspace, which arrives as
    // Ctrl+H; without this it typed an "h"
    let key = if key.code == KeyCode::Char('h') && key.modifiers == KeyModifiers::CONTROL {
        KeyEvent::new_with_kind(KeyCode::Backspace, KeyModifiers::NONE, key.kind)
    } else {
        key
    };
    // The image viewer covers the page: its own keys move about the image and between the page's
    // images, and Esc (or q, or i) closes it
    if app.images.viewer.is_some() {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('i') | KeyCode::Enter => app.close_viewer(),
            KeyCode::Char('f' | 'd') if ctrl => app.viewer_scroll(1, 0, true),
            KeyCode::Char('b' | 'u') if ctrl => app.viewer_scroll(-1, 0, true),
            KeyCode::Char('j') | KeyCode::Down => app.viewer_scroll(3, 0, false),
            KeyCode::Char('k') | KeyCode::Up => app.viewer_scroll(-3, 0, false),
            KeyCode::Char('l') | KeyCode::Right => app.viewer_scroll(0, 6, false),
            KeyCode::Char('h') | KeyCode::Left => app.viewer_scroll(0, -6, false),
            KeyCode::Char(' ') | KeyCode::PageDown => app.viewer_scroll(1, 0, true),
            KeyCode::Char('b') | KeyCode::PageUp => app.viewer_scroll(-1, 0, true),
            KeyCode::Char('g') | KeyCode::Home => app.viewer_to_end(false),
            KeyCode::Char('G') | KeyCode::End => app.viewer_to_end(true),
            KeyCode::Char('+' | '=') => app.viewer_zoom(1),
            KeyCode::Char('-') => app.viewer_zoom(-1),
            KeyCode::Char('0') => app.viewer_zoom(0),
            KeyCode::Char('n' | ']') => app.viewer_step(1),
            KeyCode::Char('p' | 'N' | '[') => app.viewer_step(-1),
            _ => {}
        }
        return Ok(false);
    }
    // Confirmation boxes take every key until they're answered
    if app.current_tab().tool_prompt.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => app.run_tool(),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                let tab = app.current_tab();
                tab.tool_prompt = None;
                tab.status_message = String::from("Cancelled: nothing was sent");
            }
            _ => {}
        }
        return Ok(false);
    }
    if app.current_tab().download_prompt.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(prompt) = app.current_tab().download_prompt.take() {
                    app.trigger_download(prompt);
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.current_tab().download_prompt = None,
            _ => {}
        }
        return Ok(false);
    }

    let active_mode = app.current_tab().input_mode;

    match active_mode {
        // Browsing keys can be rebound (keys.conf): each is handled as its action's default key
        InputMode::Normal => match app.keymap.translate(key) {
            Some(key) => handle_normal_mode(app, key, terminal_width, terminal_height),
            None => Ok(false),
        },
        InputMode::Editing => handle_editing_mode(app, key),
        InputMode::Visual => handle_visual_mode(app, key, terminal_height),
        InputMode::Search => handle_search_mode(app, key),
        InputMode::FormEdit => handle_form_edit_mode(app, key, terminal_width, terminal_height),
    }
}

/// Text pasted into the terminal (bracketed paste), as one piece: into the address bar, find or
/// the field being typed into, never read as keys. Without this, a pasted line break was Enter
/// and the text after it ran as commands (t opened a tab, q quit).
pub fn handle_paste(app: &mut App, text: &str) {
    // On one line, without control characters; text areas keep their line breaks
    let one_line = |text: &str| -> String {
        text.split(['\r', '\n'])
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .filter(|c| !c.is_control())
            .collect()
    };
    let tab = app.current_tab();
    match tab.input_mode {
        InputMode::Editing => tab.url_input.push_str(&one_line(text)),
        InputMode::Search => {
            if let Some(search) = tab.search_state.as_mut() {
                search.query.push_str(&one_line(text));
                let query = search.query.clone();
                tab.perform_search(&query);
            }
        }
        InputMode::FormEdit => {
            let multiline = tab
                .editing_control
                .and_then(|i| tab.controls.get(i))
                .is_some_and(|c| c.kind == crate::forms::ControlKind::TextArea);
            let pasted: String = if multiline {
                text.replace("\r\n", "\n").chars().filter(|c| *c == '\n' || !c.is_control()).collect()
            } else {
                one_line(text)
            };
            tab.edit_field(|field| field.push_str(&pasted));
        }
        InputMode::Normal | InputMode::Visual => {
            tab.status_message = String::from("Pasted text goes into the address bar (e), find (/) or a form field");
        }
    }
}

/// Typing into a form field
fn handle_form_edit_mode(app: &mut App, key: KeyEvent, terminal_width: u16, terminal_height: u16) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let tab = app.current_tab();
    let Some(control) = tab.editing_control.and_then(|i| tab.controls.get(i)).cloned() else {
        tab.stop_editing();
        return Ok(false);
    };
    let multiline = control.kind == crate::forms::ControlKind::TextArea;
    match key.code {
        KeyCode::Esc => {
            tab.stop_editing();
            tab.status_message = String::from("Ready");
        }
        KeyCode::Enter if multiline => tab.edit_field(|text| text.push('\n')),
        // Enter in a text field submits its form, pressed with its first button
        KeyCode::Enter => match control.form {
            Some(form) => {
                let submitter = crate::forms::default_submitter(&tab.controls, form);
                app.submit_form(form, submitter);
            }
            None => {
                tab.stop_editing();
                tab.status_message = String::from("This field isn't part of a form");
            }
        },
        KeyCode::Backspace => tab.edit_field(|text| {
            text.pop();
        }),
        KeyCode::Char('u') if ctrl => tab.edit_field(String::clear),
        KeyCode::Char('v') if ctrl => {
            if let Ok(pasted) = app.paste_from_clipboard() {
                let pasted: String = if multiline {
                    pasted.chars().filter(|c| *c == '\n' || !c.is_control()).collect()
                } else {
                    pasted.chars().filter(|c| !c.is_control()).collect()
                };
                app.current_tab().edit_field(|text| text.push_str(&pasted));
            }
        }
        KeyCode::Char(c) if !ctrl && !c.is_control() => tab.edit_field(|text| text.push(c)),
        // Tab leaves the field and moves on, as in normal mode
        KeyCode::Tab | KeyCode::BackTab => {
            tab.stop_editing();
            return handle_normal_mode(app, key, terminal_width, terminal_height);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_normal_mode(app: &mut App, key: KeyEvent, terminal_width: u16, terminal_height: u16) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let page = viewport_height(terminal_height) as isize;
    // Clearing the history needs two x presses in a row
    if key.code != KeyCode::Char('D') {
        app.confirm_clear_history = false;
    }
    // Restoring default settings or deleting snapshots needs the button pressed twice in a row
    if key.code != KeyCode::Enter {
        app.confirm_reset_settings = false;
        app.confirm_delete_snapshots = None;
    }
    match key.code {
        // --- PAGING ---
        KeyCode::PageDown => app.current_tab().scroll_by(page, viewport_height(terminal_height)),
        KeyCode::PageUp => app.current_tab().scroll_by(-page, viewport_height(terminal_height)),
        KeyCode::Char('f') if ctrl => app.current_tab().scroll_by(page, viewport_height(terminal_height)),
        KeyCode::Char('b') if ctrl => app.current_tab().scroll_by(-page, viewport_height(terminal_height)),
        KeyCode::Char('d') if ctrl => app.current_tab().scroll_by(page / 2, viewport_height(terminal_height)),
        KeyCode::Char('u') if ctrl => app.current_tab().scroll_by(-page / 2, viewport_height(terminal_height)),
        KeyCode::Char('g') | KeyCode::Home => app.current_tab().scroll_to_top(),
        KeyCode::Char('G') | KeyCode::End => app.current_tab().scroll_to_bottom(viewport_height(terminal_height)),

        // --- HISTORY ---
        KeyCode::Right | KeyCode::Char('L') => app.go_forward(),
        KeyCode::Char('H') => app.go_back(),
        KeyCode::Char('r') => app.reload(),
        KeyCode::Char('b') => app.toggle_bookmark(),
        KeyCode::Char('B') => app.follow_link(crate::bookmarks::BOOKMARKS_PAGE.to_string()),
        KeyCode::Char('s') => app.follow_link(crate::history::HISTORY_PAGE.to_string()),
        KeyCode::Char('D') => app.clear_history_key(),

        // --- VISUAL MODE ---
        KeyCode::Char('v') => app.current_tab().enter_visual_mode(),

        // --- DOWNLOAD ---
        KeyCode::Char('d') => {
            let download_dir = app.download_dir.clone();
            let tab = app.current_tab();
            if let Some(region) = tab.link_regions.get(tab.selected_link_index).filter(|r| r.control.is_none()) {
                let url = crate::network::resolve_url(tab.link_base(), &region.url);
                tab.initiate_download_request(url, &download_dir);
            }
        }

        KeyCode::Esc => {
            let tab = app.current_tab();

            // Check if there is a download state to clear
            if let Some(state) = &tab.download_state {
                match state.status {
                    // Only allow clearing if it's NOT actively downloading
                    DownloadStatus::Completed | DownloadStatus::Failed(_) => {
                        tab.download_state = None; // This removes the data, so ui.rs stops rendering it
                        tab.status_message = String::from("Ready");
                    }
                    _ => {} // Do nothing if the download is still Active
                }
            }
        }
        // --- TAB CONTROLS ---
        KeyCode::Char('t') => app.add_tab(None),
        KeyCode::Char('X') => app.reopen_tab(),
        KeyCode::Char('Y') => app.copy_address(),
        KeyCode::Char('O') => {
            app.add_tab(None);
            let tab = app.current_tab();
            tab.url_before_edit = Some(tab.url_input.clone());
            tab.url_input.clear();
            tab.input_mode = InputMode::Editing;
            tab.status_message = String::from("EDIT MODE - Type URL and press Enter");
        }
        KeyCode::Char('F') => {
            let tab = app.current_tab();
            if let Some(region) = tab.link_regions.get(tab.selected_link_index).filter(|r| r.control.is_none()) {
                let full_url = crate::network::resolve_url(tab.link_base(), &region.url);
                app.open_link_in_new_tab(full_url);
            }
        }
        KeyCode::Char('w') => app.close_tab(),
        KeyCode::Char(']') => {
            app.active_tab_index = (app.active_tab_index + 1) % app.tabs.len();
        }
        KeyCode::Char('[') => {
            if app.active_tab_index > 0 {
                app.active_tab_index -= 1;
            } else {
                app.active_tab_index = app.tabs.len() - 1;
            }
        }

        // --- PAGE CONTROLS (Targeting current_tab) ---
        KeyCode::Char('q') => return Ok(true), // Signal to quit
        KeyCode::Char('e') => {
            let tab = app.current_tab();
            tab.url_before_edit = Some(tab.url_input.clone());
            tab.input_mode = InputMode::Editing;
            tab.status_message = String::from("EDIT MODE - Type URL and press Enter");
        }
        KeyCode::Char('/') => {
            app.current_tab().input_mode = InputMode::Search;
            app.current_tab().search_state =
                Some(crate::models::SearchState { query: String::new(), matches: Vec::new(), current_match_index: 0 });
            app.current_tab().status_message = String::from("SEARCH MODE - Type query and press Enter");
        }
        KeyCode::Char('>') | KeyCode::Char('<') => {
            let tab = app.current_tab();
            if key.code == KeyCode::Char('>') {
                tab.next_search_match();
            } else {
                tab.previous_search_match();
            }
            // Auto-scroll to the current search match
            let current_line =
                tab.search_state.as_ref().and_then(|s| s.matches.get(s.current_match_index)).map(|m| m.line_index);
            if let Some(line) = current_line {
                tab.scroll_to_line(line, viewport_height(terminal_height));
            }
        }
        KeyCode::Down => app.current_tab().scroll_by(1, viewport_height(terminal_height)),
        KeyCode::Up => app.current_tab().scroll_by(-1, viewport_height(terminal_height)),
        KeyCode::Char('V') => {
            let active_index = app.active_tab_index;
            let tab = app.current_tab();
            tab.is_source_view = !tab.is_source_view; // Toggle
            tab.content_changed();

            // Update the status message
            tab.status_message =
                if tab.is_source_view { String::from("Viewing Source") } else { String::from("Viewing Rendered") };

            // Re-render immediately
            app.render_tab(active_index, terminal_width);
        }

        KeyCode::Char('R') => {
            let active_index = app.active_tab_index;
            let tab = app.current_tab();
            tab.reader_mode = !tab.reader_mode;
            tab.content_changed();
            tab.status_message = if tab.reader_mode {
                String::from("Reader mode: only the page's main text (R again for the whole page)")
            } else {
                String::from("Reader mode off: the whole page")
            };
            app.render_tab(active_index, terminal_width);
        }

        // --- VISUAL NAV ---
        KeyCode::Char('h') => app.current_tab().cursor_char = app.current_tab().cursor_char.saturating_sub(1),
        KeyCode::Char('l') => {
            let tab = app.current_tab();
            let line_len = tab.rendered_content.get(tab.cursor_line).map(crate::renderer::display_width).unwrap_or(0);
            tab.cursor_char = (tab.cursor_char + 1).min(line_len);
        }
        KeyCode::Char('k') => {
            let tab = app.current_tab();
            tab.cursor_line = tab.cursor_line.saturating_sub(1);
            tab.scroll_to_line(tab.cursor_line, viewport_height(terminal_height));
        }
        KeyCode::Char('j') => {
            let tab = app.current_tab();
            let max_lines = tab.rendered_content.len().saturating_sub(1);
            tab.cursor_line = (tab.cursor_line + 1).min(max_lines);
            tab.scroll_to_line(tab.cursor_line, viewport_height(terminal_height));
        }

        // HISTORY BACK
        KeyCode::Backspace | KeyCode::Left => app.go_back(),

        // LINK NAVIGATION (Tab)
        KeyCode::Tab | KeyCode::BackTab => {
            let tab = app.current_tab();
            // After a jump within the page, Tab goes to the first link after the jump target
            // (already selected, but off screen), and Shift+Tab to the one before it
            let after_jump = std::mem::take(&mut tab.tab_from_jump);
            if !tab.link_regions.is_empty() {
                if key.code == KeyCode::Tab && after_jump {
                    // Selected already
                } else if key.code == KeyCode::Tab {
                    tab.selected_link_index = (tab.selected_link_index + 1) % tab.link_regions.len();
                } else {
                    //backward tab traversal
                    tab.selected_link_index = if tab.selected_link_index > 0 {
                        tab.selected_link_index - 1
                    } else {
                        tab.link_regions.len() - 1
                    };
                }

                let selected_line = tab.link_regions[tab.selected_link_index].line_index;
                tab.scroll_to_line(selected_line, viewport_height(terminal_height));
            }
        }

        // LINK SELECTION (Enter)
        KeyCode::Enter => {
            let tab = app.current_tab();

            if let Some(region) = tab.link_regions.get(tab.selected_link_index) {
                if let Some(control) = region.control {
                    app.activate_control(control);
                } else if region.url.is_empty() && region.image.is_some() {
                    // An image's placeholder (not inside a link): view it
                    app.view_image(viewport_height(terminal_height));
                } else {
                    // Resolve URL (Handle relative paths), then load it or jump within the page
                    let new_url = crate::network::resolve_url(tab.link_base(), &region.url);
                    app.follow_link(new_url);
                }
            }
        }
        KeyCode::Char('p') => app.toggle_i2p_mode(),
        KeyCode::Char('S') => app.follow_link(crate::app::SETTINGS_PAGE.to_string()),
        KeyCode::Char('A') => app.open_archived_copy(),
        KeyCode::Char('T') => app.toggle_tracking(),
        KeyCode::Char('I') => app.load_images(app.active_tab_index, true),
        KeyCode::Char('U') => app.follow_link(crate::update::UPDATE_PAGE.to_string()),
        KeyCode::Char('i') => app.view_image(viewport_height(terminal_height)),
        KeyCode::Char('+') => app.zoom_images(1),
        KeyCode::Char('-') => app.zoom_images(-1),
        KeyCode::Char('Z') => app.open_snapshots(),
        KeyCode::Char('?') => app.follow_link(crate::app::HELP_PAGE.to_string()),
        KeyCode::Char('C') => app.toggle_cookies(),
        _ => {}
    }
    Ok(false)
}

fn handle_editing_mode(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Enter => {
            let tab = app.current_tab();
            // Save the page we're leaving, not the address that was just typed
            if let Some(previous_url) = tab.url_before_edit.take().filter(|u| !u.is_empty()) {
                let entry = tab.history_entry(previous_url);
                tab.history.push(entry);
            }
            tab.forward_history.clear();
            tab.cancel_restore();

            app.load_url_input(false);
            app.current_tab().input_mode = InputMode::Normal;
        }
        // COPY LINE (from address bar to clipboard)
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let current_input = app.current_tab().url_input.clone();
            app.current_tab().status_message = match app.copy_to_clipboard(current_input) {
                Ok(()) => String::from("Address copied to clipboard!"),
                Err(e) => e,
            };
        }
        // CLEAR LINE (Standard Terminal Shortcut)
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.current_tab().url_input.clear();
        }

        // PASTE (Standard Shortcut)
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match app.paste_from_clipboard() {
                Ok(text) => {
                    // Sanitize to remove newlines for the address bar
                    let sanitized = text.replace(['\n', '\r'], "");
                    app.current_tab().url_input.push_str(&sanitized);
                }
                Err(e) => app.current_tab().status_message = e,
            }
        }

        // COMBINED: CLEAR AND PASTE (Using Ctrl + K)
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match app.paste_from_clipboard() {
                Ok(text) => {
                    let sanitized = text.replace(['\n', '\r'], "");
                    app.current_tab().url_input.clear();
                    app.current_tab().url_input.push_str(&sanitized);
                }
                // Keep the address rather than clearing it when there is nothing to paste
                Err(e) => app.current_tab().status_message = e,
            }
        }
        // Other Ctrl combinations aren't text
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
            app.current_tab().url_input.push(c);
        }
        KeyCode::Backspace => {
            app.current_tab().url_input.pop();
        }
        KeyCode::Esc => {
            let tab = app.current_tab();
            // Cancelling discards the edit and restores the original address
            if let Some(previous_url) = tab.url_before_edit.take() {
                tab.url_input = previous_url;
            }
            tab.input_mode = InputMode::Normal;
            tab.status_message = String::from("Ready");
        }
        _ => {}
    }
    Ok(false)
}

fn handle_visual_mode(app: &mut App, key: KeyEvent, terminal_height: u16) -> Result<bool> {
    // The arrow keys move the selection like h j k l
    let code = match key.code {
        KeyCode::Left => KeyCode::Char('h'),
        KeyCode::Down => KeyCode::Char('j'),
        KeyCode::Up => KeyCode::Char('k'),
        KeyCode::Right => KeyCode::Char('l'),
        other => other,
    };
    match code {
        KeyCode::Char('h') => {
            let tab = app.current_tab();
            tab.cursor_char = tab.cursor_char.saturating_sub(1);
            if let Some(ref mut sel) = tab.selection {
                sel.end_char = tab.cursor_char;
            }
        }
        // MOVE DOWN
        KeyCode::Char('j') => {
            let tab = app.current_tab();
            let max_lines = tab.rendered_content.len().saturating_sub(1);
            tab.cursor_line = (tab.cursor_line + 1).min(max_lines);
            tab.scroll_to_line(tab.cursor_line, viewport_height(terminal_height));

            // Ensure cursor_char is valid for the new line
            let line_len = tab.rendered_content.get(tab.cursor_line).map(crate::renderer::display_width).unwrap_or(0);
            tab.cursor_char = tab.cursor_char.min(line_len);

            if let Some(ref mut sel) = tab.selection {
                sel.end_line = tab.cursor_line;
                sel.end_char = tab.cursor_char;
            }
        }
        // MOVE UP
        KeyCode::Char('k') => {
            let tab = app.current_tab();
            tab.cursor_line = tab.cursor_line.saturating_sub(1);
            tab.scroll_to_line(tab.cursor_line, viewport_height(terminal_height));

            let line_len = tab.rendered_content.get(tab.cursor_line).map(crate::renderer::display_width).unwrap_or(0);
            tab.cursor_char = tab.cursor_char.min(line_len);

            if let Some(ref mut sel) = tab.selection {
                sel.end_line = tab.cursor_line;
                sel.end_char = tab.cursor_char;
            }
        }
        // MOVE RIGHT
        KeyCode::Char('l') => {
            let tab = app.current_tab();
            let line_len = tab.rendered_content.get(tab.cursor_line).map(crate::renderer::display_width).unwrap_or(0);
            tab.cursor_char = (tab.cursor_char + 1).min(line_len);

            if let Some(ref mut sel) = tab.selection {
                sel.end_char = tab.cursor_char;
            }
        }

        // YANK (Copy)
        KeyCode::Char('y') => {
            // 1. Get the text and finish the borrow of the tab immediately
            let text_to_copy = app.current_tab().extract_text_from_selection();

            if !text_to_copy.is_empty() {
                let count = text_to_copy.chars().count();
                // 2. Now we can safely borrow the clipboard
                let result = app.copy_to_clipboard(text_to_copy);

                // 3. Re-borrow the tab to update status and reset mode
                let tab = app.current_tab();
                tab.status_message = match result {
                    Ok(()) => format!("Copied {} characters to the clipboard", count),
                    Err(e) => e,
                };
                tab.input_mode = InputMode::Normal;
                tab.selection = None;
            } else {
                let tab = app.current_tab();
                tab.input_mode = InputMode::Normal;
                tab.selection = None;
                tab.status_message = String::from("Nothing was selected, so nothing was copied");
            }
        }
        KeyCode::Esc => {
            app.current_tab().input_mode = InputMode::Normal;
            app.current_tab().selection = None;
            app.current_tab().status_message = String::from("Left copy mode");
        }
        _ => {}
    }
    Ok(false)
}

pub fn handle_mouse_event(app: &mut App, mouse: MouseEvent, terminal_width: u16, terminal_height: u16) -> Result<()> {
    // The tool confirmation box is answered with the keyboard only, so a stray click can't run a tool
    if app.current_tab().tool_prompt.is_some() {
        return Ok(());
    }
    let download_dir = app.download_dir.clone();
    let tab = app.current_tab();
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            tab.scroll_by(MOUSE_SCROLL_LINES as isize, viewport_height(terminal_height));
        }
        MouseEventKind::ScrollUp => {
            tab.scroll_by(-(MOUSE_SCROLL_LINES as isize), viewport_height(terminal_height));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(prompt) = tab.download_prompt.take() {
                let popup_x = terminal_width / DOWNLOAD_PROMPT_X_DIVISOR;
                let popup_y = (terminal_height / DOWNLOAD_PROMPT_Y_DIVISOR).saturating_sub(DOWNLOAD_PROMPT_Y_OFFSET);
                let popup_w = terminal_width / DOWNLOAD_PROMPT_WIDTH_DIVISOR;
                let popup_h = DOWNLOAD_PROMPT_HEIGHT;

                if mouse.column >= popup_x
                    && mouse.column < (popup_x + popup_w)
                    && mouse.row >= popup_y
                    && mouse.row < popup_y + popup_h
                {
                    // Detect clicks on the button line (popup_y + offset)
                    if mouse.row == popup_y + DOWNLOAD_PROMPT_BUTTON_ROW_OFFSET {
                        if mouse.column < popup_x + (popup_w / 2) {
                            app.trigger_download(prompt);
                        } else {
                            tab.download_prompt = None;
                        }
                    } else {
                        tab.download_prompt = Some(prompt);
                    }
                    return Ok(());
                }
                tab.download_prompt = Some(prompt);
            }
            // 1. Determine which line was clicked
            if mouse.row >= UI_ROW_OFFSET {
                // UI_ROW_OFFSET is the UI offset
                let visual_line = (mouse.row - UI_ROW_OFFSET) as usize;
                let real_line_idx = visual_line + tab.scroll;
                let click_x = (mouse.column as usize).saturating_sub(1);

                // Clicking below the end of the page puts the cursor on the last line
                tab.cursor_line = real_line_idx.min(tab.rendered_content.len().saturating_sub(1));
                tab.cursor_char = click_x;

                // 2. Search the Link Regions for a match
                // We filter for links on this specific line
                let found = tab.link_regions.iter().position(|link| {
                    link.line_index == real_line_idx && click_x >= link.x_start && click_x < link.x_end
                });

                if let Some(control) = found.and_then(|i| tab.link_regions[i].control) {
                    // Clicking a form control selects and uses it
                    tab.selected_link_index = found.unwrap_or(0);
                    app.activate_control(control);
                } else if let Some(region) = found.map(|i| &tab.link_regions[i]) {
                    // 3. Determine if this should be a download or navigation
                    let full_url = crate::network::resolve_url(tab.link_base(), &region.url);

                    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                        app.open_link_in_new_tab(full_url);
                    } else if is_downloadable_file(&full_url) {
                        // download for file types
                        tab.initiate_download_request(full_url, &download_dir);
                    } else {
                        // Normal navigation for HTML pages
                        app.follow_link(full_url);
                    }
                }
            }
        }
        // Optional: can handle clicks here too!
        _ => {}
    }
    Ok(())
}

/// Handle every network message that is waiting, so bursts don't queue up behind the frame rate
pub fn process_network_events(app: &mut App, terminal_width: u16) -> Result<()> {
    while let Ok(response) = app.rx.try_recv() {
        handle_network_event(app, response, terminal_width)?;
    }
    Ok(())
}

pub fn handle_network_event(app: &mut App, response: NetworkResponse, terminal_width: u16) -> Result<()> {
    // Not about any one tab
    if let NetworkResponse::RouterProgress(progress) = response {
        app.finish_router_progress(progress);
        return Ok(());
    }
    let target_id = match &response {
        NetworkResponse::Success(id, ..) => *id,
        NetworkResponse::Error(id, ..) => *id,
        NetworkResponse::Loading(id) => *id,
        NetworkResponse::Info(id, ..) => *id,
        NetworkResponse::DownloadProgress(id, ..) => *id,
        NetworkResponse::DownloadFinished(id, ..) => *id,
        NetworkResponse::DownloadError(id, ..) => *id,
        NetworkResponse::Rendered(id, ..) => *id,
        NetworkResponse::ToolsListed(id, ..) => *id,
        NetworkResponse::ToolFinished(id, ..) => *id,
        NetworkResponse::ImageLoaded(id, ..) => *id,
        NetworkResponse::UpdateChecked(id, ..) => *id,
        NetworkResponse::UpdateDownloaded(id, ..) => *id,
        NetworkResponse::RouterProgress(_) => return Ok(()),
    };

    if let Some(index) = app.tabs.iter().position(|t| t.id == target_id) {
        match response {
            NetworkResponse::DownloadProgress(_, downloaded, total) => {
                let tab = &mut app.tabs[index];
                // Update in place so the filename set by trigger_download is kept
                let d = tab.download_state.get_or_insert_with(|| crate::models::Download {
                    filename: String::new(),
                    bytes_downloaded: 0,
                    total_size: None,
                    status: DownloadStatus::Active,
                });
                d.bytes_downloaded = downloaded;
                d.total_size = total;
                // Update status message for footer
                tab.status_message = match total {
                    Some(t) if t > 0 => format!("Downloading: {}%", (downloaded * 100) / t),
                    _ => format!("Downloading: {} bytes", downloaded),
                };
            }
            NetworkResponse::DownloadFinished(_, filename) => {
                let tab = &mut app.tabs[index];
                // Small files can finish before a throttled progress update creates the state
                let d = tab.download_state.get_or_insert_with(|| crate::models::Download {
                    filename: String::new(),
                    bytes_downloaded: 0,
                    total_size: None,
                    status: DownloadStatus::Active,
                });
                d.status = DownloadStatus::Completed;
                d.filename = filename.clone();
                tab.status_message = format!("Download complete: {}", filename);
            }
            NetworkResponse::Success(_, title, html_source, final_url, kind) => {
                let tab = &mut app.tabs[index];
                tab.page_title = title;
                tab.set_content(html_source, kind);
                tab.reset_page_state();
                tab.status_message = String::from("Loaded");

                // Show where redirects led, keeping the #fragment that was asked for
                let fragment = tab.pending_fragment.take();
                let page_url = match url::Url::parse(&final_url) {
                    Ok(mut url) => {
                        url.set_fragment(fragment.as_deref());
                        url.to_string()
                    }
                    Err(_) => final_url,
                };
                if tab.input_mode != InputMode::Editing {
                    tab.url_input = page_url.clone();
                }
                tab.page_url = Some(page_url);
                tab.record_visit_after_render = true;
                tab.load_images_after_render = true;
                // Back, Forward and Reload restore a position; a refresh is followed only on the
                // way to a page, so Back can still return past it
                tab.follow_refresh_after_render = !tab.is_returning();

                tab.jump_after_render(fragment);
                tab.restore_position_after_render();
                app.render_tab(index, terminal_width);
                if app.is_rendering(index) {
                    app.tabs[index].status_message = String::from(crate::app::RENDERING_STATUS);
                } else {
                    // Laid out already (otherwise this is done when the layout arrives)
                    app.hint_cookies_for_sign_in(index);
                }
            }
            NetworkResponse::Rendered(_, version, page) => {
                app.finish_render(index, version, *page);
            }
            NetworkResponse::ToolsListed(_, address, server, result) => {
                app.finish_listing_tools(index, address, server, result);
            }
            NetworkResponse::ToolFinished(_, server, title, result) => {
                app.finish_tool(index, server, title, result);
            }
            NetworkResponse::ImageLoaded(_, url, result) => {
                app.finish_image(index, url, result);
            }
            NetworkResponse::UpdateChecked(_, asked, result) => {
                app.finish_update_check(index, asked, result);
            }
            NetworkResponse::UpdateDownloaded(_, result) => {
                app.finish_update_download(index, result);
            }
            // Handled above
            NetworkResponse::RouterProgress(_) => {}
            NetworkResponse::DownloadError(_, msg) => {
                let tab = &mut app.tabs[index];
                // The download may fail before any progress created its state
                match tab.download_state {
                    Some(ref mut d) => d.status = DownloadStatus::Failed(msg.clone()),
                    None => {
                        tab.download_state = Some(crate::models::Download {
                            filename: String::new(),
                            bytes_downloaded: 0,
                            total_size: None,
                            status: DownloadStatus::Failed(msg.clone()),
                        })
                    }
                }
                tab.status_message = format!("Download failed: {}", msg);
            }
            NetworkResponse::Error(_, msg) => {
                let tab = &mut app.tabs[index];
                tab.page_title = String::from("Error");
                tab.set_content(
                    format!("<h1>Error</h1><hr><p style='color:red'>{}</p>", crate::network::escape_html(&msg)),
                    crate::network::PageKind::Html,
                );
                tab.reset_page_state();
                tab.page_url = None;
                tab.pending_fragment = None;
                tab.cancel_restore();
                tab.status_message = String::from("Error");
                app.render_tab(index, terminal_width);
            }
            NetworkResponse::Loading(_) => {
                let tab = &mut app.tabs[index];
                tab.page_title = String::from("Loading...");
                tab.status_message = String::from("Fetching...");
            }
            NetworkResponse::Info(_, msg) => {
                let tab = &mut app.tabs[index];
                tab.status_message = msg;
            }
        }
    }
    Ok(())
}

fn handle_search_mode(app: &mut App, key: KeyEvent) -> Result<bool> {
    let tab = app.current_tab();
    match key.code {
        KeyCode::Esc => {
            tab.clear_search();
        }
        KeyCode::Enter => {
            // Search is already performed during typing, just exit search mode, saying what it found
            tab.input_mode = InputMode::Normal;
            tab.status_message = match &tab.search_state {
                Some(s) if s.matches.len() == 1 => format!("1 match for \"{}\"", s.query),
                Some(s) if !s.matches.is_empty() => {
                    format!("{} matches for \"{}\": > and < move between them", s.matches.len(), s.query)
                }
                Some(s) => format!("No matches for \"{}\"", s.query),
                None => String::from("Ready"),
            };
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
            if let Some(search_state) = &mut tab.search_state {
                search_state.query.push(c);
                // Clone the query to avoid borrowing issues
                let query = search_state.query.clone();
                let _ = search_state; // Release the borrow
                tab.perform_search(&query);
            }
        }
        KeyCode::Backspace => {
            if let Some(search_state) = &mut tab.search_state {
                search_state.query.pop();
                if search_state.query.is_empty() {
                    tab.search_state = None;
                    tab.input_mode = InputMode::Normal;
                    tab.status_message = String::from("Ready");
                } else {
                    // Clone the query to avoid borrowing issues
                    let query = search_state.query.clone();
                    let _ = search_state; // Release the borrow
                    tab.perform_search(&query);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}
