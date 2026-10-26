use crate::app::App;
use crate::constants::{MOUSE_SCROLL_LINES, UI_HEIGHT_OFFSET, UI_ROW_OFFSET};
use crate::models::{DownloadStatus, InputMode};
use crate::network::NetworkResponse;
use crate::constants::*;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::Backend;
use std::io::Result;

/// Determines if a URL likely points to a downloadable file based on extension or patterns
fn is_downloadable_file(url: &str) -> bool {
    let u = url.to_lowercase();
    // Restored common types that users expect to download via click
    let binary_exts = [
        "zip", "pdf", "exe", "dmg", "pkg", "deb", "iso", "mp4", "mp3",
        "png", "jpg", "jpeg", "gif", "docx", "xlsx", "tar", "gz", "ogg", "txt"
    ];

    if let Some(dot) = u.rfind('.') {
        let ext = u[dot + 1..].split('?').next().unwrap_or("");
        if binary_exts.contains(&ext) { return true; }
    }

    // Catch common dynamic download paths
    ["/download/", "/files/", "/assets/", "/attachments/"].iter().any(|p| u.contains(p))
}

/// Rows available for page content: tab bar (3) + URL bar (3) + content borders (2) are subtracted
fn viewport_height(terminal_height: u16) -> usize {
    terminal_height.saturating_sub(UI_HEIGHT_OFFSET) as usize
}

pub fn handle_key_event<B: Backend>(
    app: &mut App,
    key: KeyEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<bool> {
    let active_mode = app.current_tab().input_mode;

    match active_mode {
        InputMode::Normal => handle_normal_mode::<B>(app, key, terminal_width, terminal_height),
        InputMode::Editing => handle_editing_mode(app, key),
        InputMode::Visual => handle_visual_mode(app, key, terminal_height),
        InputMode::Search => handle_search_mode(app, key),
    }
}

fn handle_normal_mode<B: Backend>(
    app: &mut App,
    key: KeyEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<bool> {
    match key.code {
        // --- VISUAL MODE ---
        KeyCode::Char('v') => app.current_tab().enter_visual_mode(),

        // --- DOWNLOAD ---
        KeyCode::Char('d') => {
            let download_dir = app.download_dir.clone();
            let tab = app.current_tab();
            if let Some(region) = tab.link_regions.get(tab.selected_link_index) {
                let url = crate::network::resolve_url(tab.link_base(), &region.url);
                tab.initiate_download_request(url, &download_dir);
            }
        }

        KeyCode::Char('y') | KeyCode::Char('Y') if app.current_tab().download_prompt.is_some() => {
            if let Some(prompt) = app.current_tab().download_prompt.take() {
                app.trigger_download(prompt);
            }
        }

        KeyCode::Char('n') | KeyCode::Char('N') if app.current_tab().download_prompt.is_some() => {
            app.current_tab().download_prompt = None;
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
        KeyCode::Char('n') => app.add_tab(None),
        KeyCode::Char('t') => {
            let tab = app.current_tab();
            if let Some(region) = tab.link_regions.get(tab.selected_link_index) {
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
            app.current_tab().search_state = Some(crate::models::SearchState {
                query: String::new(),
                matches: Vec::new(),
                current_match_index: 0,
            });
            app.current_tab().status_message =
                String::from("SEARCH MODE - Type query and press Enter");
        }
        KeyCode::Char('>') | KeyCode::Char('<') => {
            let tab = app.current_tab();
            if key.code == KeyCode::Char('>') {
                tab.next_search_match();
            } else {
                tab.previous_search_match();
            }
            // Auto-scroll to the current search match
            let current_line = tab
                .search_state
                .as_ref()
                .and_then(|s| s.matches.get(s.current_match_index))
                .map(|m| m.line_index);
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

            // Update the status message
            tab.status_message = if tab.is_source_view {
                String::from("Viewing Source")
            } else {
                String::from("Viewing Rendered")
            };

            // Re-render immediately
            app.render_tab(active_index, terminal_width);
        }

        // --- VISUAL NAV ---
        KeyCode::Char('h') => {
            app.current_tab().cursor_char = app.current_tab().cursor_char.saturating_sub(1)
        }
        KeyCode::Char('l') => {
            let tab = app.current_tab();
            let line_len = tab
                .rendered_content
                .get(tab.cursor_line)
                .map(|l| l.width())
                .unwrap_or(0);
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
            if !tab.link_regions.is_empty() {
                if key.code == KeyCode::Tab {
                    tab.selected_link_index =
                        (tab.selected_link_index + 1) % tab.link_regions.len();
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
                // Resolve URL (Handle relative paths), then load it or jump within the page
                let new_url = crate::network::resolve_url(tab.link_base(), &region.url);
                app.follow_link(new_url);
            }
        }
        KeyCode::Char('p') => {
            app.i2p_mode = !app.i2p_mode; // Toggle
        }
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
                tab.history.push(previous_url);
            }

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
                    let sanitized = text.replace(|c: char| c == '\n' || c == '\r', "");
                    app.current_tab().url_input.push_str(&sanitized);
                }
                Err(e) => app.current_tab().status_message = e,
            }
        }

        // COMBINED: CLEAR AND PASTE (Using Ctrl + K)
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            match app.paste_from_clipboard() {
                Ok(text) => {
                    let sanitized = text.replace(|c: char| c == '\n' || c == '\r', "");
                    app.current_tab().url_input.clear();
                    app.current_tab().url_input.push_str(&sanitized);
                }
                // Keep the address rather than clearing it when there is nothing to paste
                Err(e) => app.current_tab().status_message = e,
            }
        }
        KeyCode::Char(c) => {
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
    match key.code {
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
            let line_len = tab.rendered_content.get(tab.cursor_line).map(|l| l.width()).unwrap_or(0);
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

            let line_len = tab.rendered_content.get(tab.cursor_line).map(|l| l.width()).unwrap_or(0);
            tab.cursor_char = tab.cursor_char.min(line_len);

            if let Some(ref mut sel) = tab.selection {
                sel.end_line = tab.cursor_line;
                sel.end_char = tab.cursor_char;
            }
        }
        // MOVE RIGHT
        KeyCode::Char('l') => {
            let tab = app.current_tab();
            let line_len = tab.rendered_content.get(tab.cursor_line).map(|l| l.width()).unwrap_or(0);
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
                // 2. Now we can safely borrow the clipboard
                let result = app.copy_to_clipboard(text_to_copy);

                // 3. Re-borrow the tab to update status and reset mode
                let tab = app.current_tab();
                tab.status_message = match result {
                    Ok(()) => String::from("Text yanked to clipboard!"),
                    Err(e) => e,
                };
                tab.input_mode = InputMode::Normal;
                tab.selection = None;
            } else {
                let tab = app.current_tab();
                tab.input_mode = InputMode::Normal;
                tab.selection = None;
            }
        }
        KeyCode::Esc => {
            app.current_tab().input_mode = InputMode::Normal;
            app.current_tab().selection = None;
            app.current_tab().status_message = String::from("Ready");
        }
        _ => {}
    }
    Ok(false)
}

pub fn handle_mouse_event<B: Backend>(
    app: &mut App,
    mouse: MouseEvent,
    terminal_width: u16,
    terminal_height: u16,
) -> Result<()> {
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

                if mouse.column >= popup_x && mouse.column < (popup_x + popup_w) &&
                   mouse.row >= popup_y && mouse.row < popup_y + popup_h
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
                let found_link = tab.link_regions.iter().find(|link| {
                    link.line_index == real_line_idx
                        && click_x >= link.x_start
                        && click_x < link.x_end
                });

                if let Some(region) = found_link {
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
pub fn process_network_events<B: Backend>(app: &mut App, terminal_width: u16) -> Result<()> {
    while let Ok(response) = app.rx.try_recv() {
        handle_network_event::<B>(app, response, terminal_width)?;
    }
    Ok(())
}

pub fn handle_network_event<B: Backend>(
    app: &mut App,
    response: NetworkResponse,
    terminal_width: u16,
) -> Result<()> {
    let target_id = match &response {
        NetworkResponse::Success(id, ..) => *id,
        NetworkResponse::Error(id, ..) => *id,
        NetworkResponse::Loading(id) => *id,
        NetworkResponse::Info(id, ..) => *id,
        NetworkResponse::DownloadProgress(id, ..) => *id,
        NetworkResponse::DownloadFinished(id, ..) => *id,
        NetworkResponse::DownloadError(id, ..) => *id,
    };

    if let Some(index) = app.tabs.iter().position(|t| t.id == target_id) {
        match response {
            NetworkResponse::DownloadProgress(_, downloaded, total) => {
                let tab = &mut app.tabs[index];
                // Update in place so the filename set by trigger_download is kept
                let d = tab.download_state.get_or_insert_with(|| crate::models::Download {
                    _id: target_id,
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
                    _id: target_id,
                    filename: String::new(),
                    bytes_downloaded: 0,
                    total_size: None,
                    status: DownloadStatus::Active,
                });
                d.status = DownloadStatus::Completed;
                d.filename = filename.clone();
                tab.status_message = format!("Download complete: {}", filename);
            }
            NetworkResponse::Success(_, title, html_source, final_url) => {
                let tab = &mut app.tabs[index];
                tab.page_title = title;
                tab.html_source = html_source;
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

                app.render_tab(index, terminal_width);
                if let Some(fragment) = fragment {
                    app.tabs[index].jump_to_fragment(&fragment);
                }
            }
            NetworkResponse::DownloadError(_, msg) => {
                let tab = &mut app.tabs[index];
                // The download may fail before any progress created its state
                match tab.download_state {
                    Some(ref mut d) => d.status = DownloadStatus::Failed(msg.clone()),
                    None => {
                        tab.download_state = Some(crate::models::Download {
                            _id: target_id,
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
                tab.html_source = format!("<h1>Error</h1><hr><p style='color:red'>{}</p>", msg);
                tab.reset_page_state();
                tab.page_url = None;
                tab.pending_fragment = None;
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
            // Search is already performed during typing, just exit search mode
            tab.input_mode = InputMode::Normal;
        }
        KeyCode::Char(c) => {
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
