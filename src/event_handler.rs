use crate::app::App;
use crate::constants::{MOUSE_SCROLL_LINES, UI_HEIGHT_OFFSET, UI_ROW_OFFSET};
use crate::models::{DownloadStatus, InputMode};
use crate::network::NetworkResponse;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::Backend;
use std::io::Result;

/// Determines if a URL likely points to a downloadable file based on extension or patterns
fn is_downloadable_file(url: &str) -> bool {
    let u = url.to_lowercase();
    // Restored common types that users expect to download via click
    let binary_exts = [
        "zip", "pdf", "exe", "dmg", "pkg", "deb", "iso", "mp4", "mp3",
        "png", "jpg", "jpeg", "gif", "docx", "xlsx", "tar", "gz"
    ];

    if let Some(dot) = u.rfind('.') {
        let ext = u[dot + 1..].split('?').next().unwrap_or("");
        if binary_exts.contains(&ext) { return true; }
    }

    // Catch common dynamic download paths
    ["/download/", "/files/", "/assets/", "/attachments/"].iter().any(|p| u.contains(p))
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
        InputMode::Visual => handle_visual_mode(app, key),
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
            let tab = app.current_tab();
            if let Some(region) = tab.link_regions.get(tab.selected_link_index) {
                let url = crate::network::resolve_url(&tab.url_input, &region.url);
                tab.initiate_download_request(url);
            }
        }

        KeyCode::Char('y') | KeyCode::Char('Y') if app.current_tab().download_prompt.is_some() => {
            if let Some(prompt) = app.current_tab().download_prompt.take() {
                app.trigger_download(prompt.url);
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
                let full_url = crate::network::resolve_url(&tab.url_input, &region.url);
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
            app.current_tab().input_mode = InputMode::Editing;
            app.current_tab().status_message = String::from("EDIT MODE - Type URL and press Enter");
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
        KeyCode::Char('>') => {
            let tab = app.current_tab();
            tab.next_search_match();
            // Auto-scroll to the current search match
            if let Some(search_state) = &tab.search_state {
                if let Some(current_match) =
                    search_state.matches.get(search_state.current_match_index)
                {
                    let viewport_height = terminal_height.saturating_sub(UI_HEIGHT_OFFSET) as usize;

                    if current_match.line_index < tab.scroll {
                        // If match is above current view, jump to it
                        tab.scroll = current_match.line_index;
                    } else if current_match.line_index >= tab.scroll + viewport_height {
                        // If match is below, scroll just enough to make it visible at the bottom
                        tab.scroll = current_match.line_index - viewport_height + 1;
                    }
                }
            }
        }
        KeyCode::Char('<') => {
            let tab = app.current_tab();
            tab.previous_search_match();
            // Auto-scroll to the current search match
            if let Some(search_state) = &tab.search_state {
                if let Some(current_match) =
                    search_state.matches.get(search_state.current_match_index)
                {
                    let viewport_height = terminal_height.saturating_sub(UI_HEIGHT_OFFSET) as usize;

                    if current_match.line_index < tab.scroll {
                        // If match is above current view, jump to it
                        tab.scroll = current_match.line_index;
                    } else if current_match.line_index >= tab.scroll + viewport_height {
                        // If match is below, scroll just enough to make it visible at the bottom
                        tab.scroll = current_match.line_index - viewport_height + 1;
                    }
                }
            }
        }
        KeyCode::Down => app.current_tab().scroll = app.current_tab().scroll.saturating_add(1),
        KeyCode::Up => app.current_tab().scroll = app.current_tab().scroll.saturating_sub(1),
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
            // Auto-scroll up if cursor goes off-screen
            if tab.cursor_line < tab.scroll {
                tab.scroll = tab.cursor_line;
            }
        }
        KeyCode::Char('j') => {
            let tab = app.current_tab();
            let max_lines = tab.rendered_content.len().saturating_sub(1);
            tab.cursor_line = (tab.cursor_line + 1).min(max_lines);

            // Auto-scroll down if cursor goes off-screen
            let viewport_height = terminal_height.saturating_sub(UI_HEIGHT_OFFSET) as usize;
            if tab.cursor_line >= tab.scroll + viewport_height {
                tab.scroll = tab.cursor_line - viewport_height + 1;
            }
        }

        // HISTORY BACK
        KeyCode::Backspace | KeyCode::Left => {
            let tab = app.current_tab();
            if let Some(previous_url) = tab.history.pop() {
                tab.url_input = previous_url;
                app.submit_request();
            }
        }

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

                // --- IMPROVED AUTOSCROLL ---
                let selected = &tab.link_regions[tab.selected_link_index];
                // We subtract 6 for the Tab bar (3) and URL bar (3),
                // and another 2 for the borders of the Browser block.
                let viewport_height = terminal_height.saturating_sub(UI_HEIGHT_OFFSET) as usize;

                if selected.line_index < tab.scroll {
                    // If link is above current view, jump to it
                    tab.scroll = selected.line_index;
                } else if selected.line_index >= tab.scroll + viewport_height {
                    // If link is below, scroll just enough to make it visible at the bottom
                    tab.scroll = selected.line_index - viewport_height + 1;
                }
            }
        }

        // LINK SELECTION (Enter)
        KeyCode::Enter => {
            let tab = app.current_tab();

            if let Some(region) = tab.link_regions.get(tab.selected_link_index) {
                // 1. Save History
                if !tab.url_input.is_empty() {
                    tab.history.push(tab.url_input.clone());
                }

                // 2. Resolve URL (Handle relative paths)
                let new_url = crate::network::resolve_url(&tab.url_input, &region.url);
                tab.url_input = new_url;

                // 3. Submit
                app.submit_request(); // This function already looks at current_tab()

                // 4. Reset
                app.current_tab().selected_link_index = 0;
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
            // Save history
            if !tab.url_input.is_empty() {
                tab.history.push(tab.url_input.clone());
            }

            app.submit_request();
            app.current_tab().input_mode = InputMode::Normal;
        }
        // COPY LINE (from address bar to clipboard)
        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let current_input = app.current_tab().url_input.clone();
            if let Ok(_) = app.clipboard.set_text(current_input) {
                app.current_tab().status_message = String::from("Address copied to clipboard!");
            }
        }
        // CLEAR LINE (Standard Terminal Shortcut)
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.current_tab().url_input.clear();
        }

        // PASTE (Standard Shortcut)
        KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Ok(text) = app.clipboard.get_text() {
                // Sanitize to remove newlines for the address bar
                let sanitized = text.replace(|c: char| c == '\n' || c == '\r', "");
                app.current_tab().url_input.push_str(&sanitized);
            }
        }

        // COMBINED: CLEAR AND PASTE (Using Ctrl + K)
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.current_tab().url_input.clear();
            if let Ok(text) = app.clipboard.get_text() {
                let sanitized = text.replace(|c: char| c == '\n' || c == '\r', "");
                app.current_tab().url_input.push_str(&sanitized);
            }
        }
        KeyCode::Char(c) => {
            app.current_tab().url_input.push(c);
        }
        KeyCode::Backspace => {
            app.current_tab().url_input.pop();
        }
        KeyCode::Esc => {
            app.current_tab().input_mode = InputMode::Normal;
            app.current_tab().status_message = String::from("Ready");
        }
        _ => {}
    }
    Ok(false)
}

fn handle_visual_mode(app: &mut App, key: KeyEvent) -> Result<bool> {
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

            // Ensure cursor_char is valid for the new line
            let line_len = tab.rendered_content[tab.cursor_line].width();
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

            let line_len = tab.rendered_content[tab.cursor_line].width();
            tab.cursor_char = tab.cursor_char.min(line_len);

            if let Some(ref mut sel) = tab.selection {
                sel.end_line = tab.cursor_line;
                sel.end_char = tab.cursor_char;
            }
        }
        // MOVE RIGHT
        KeyCode::Char('l') => {
            let tab = app.current_tab();
            let line_len = tab.rendered_content[tab.cursor_line].width();
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
                let _ = app.clipboard.set_text(text_to_copy);

                // 3. Re-borrow the tab to update status and reset mode
                let tab = app.current_tab();
                tab.status_message = String::from("Text yanked to clipboard!");
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
    let tab = app.current_tab();
    if let Some(prompt) = tab.download_prompt.take() {
        let popup_x = terminal_width / 4;
        let popup_y = (terminal_height / 2).saturating_sub(4);
        let popup_w = terminal_width / 2;
        let popup_h = 9;

        if mouse.column >= popup_x && mouse.column < (popup_x + popup_w) &&
           mouse.row >= popup_y && mouse.row < popup_y + popup_h
        {
            // Detect clicks on the button line (popup_y + 6)
            if mouse.row == popup_y + 6 {
                if mouse.column < popup_x + (popup_w / 2) {
                    app.trigger_download(prompt.url);
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
    match mouse.kind {
        MouseEventKind::ScrollDown => {
            tab.scroll = tab.scroll.saturating_add(MOUSE_SCROLL_LINES); // Scroll down by configured amount
        }
        MouseEventKind::ScrollUp => {
            tab.scroll = tab.scroll.saturating_sub(MOUSE_SCROLL_LINES); // Scroll up by configured amount
        }
        MouseEventKind::Down(MouseButton::Left) => {
            // 1. Determine which line was clicked
            if mouse.row >= UI_ROW_OFFSET {
                // UI_ROW_OFFSET is the UI offset
                let visual_line = (mouse.row - UI_ROW_OFFSET) as usize;
                let real_line_idx = visual_line + tab.scroll;
                let click_x = (mouse.column as usize).saturating_sub(1);

                tab.cursor_line = real_line_idx;
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
                    let full_url = crate::network::resolve_url(&tab.url_input, &region.url);

                    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                        app.open_link_in_new_tab(full_url);
                    } else if is_downloadable_file(&full_url) {
                        // download for file types
                        tab.initiate_download_request(full_url);
                    } else {
                        // Normal navigation for HTML pages
                        if !tab.url_input.is_empty() {
                            tab.history.push(tab.url_input.clone());
                        }
                        tab.url_input = full_url;
                        app.submit_request();
                    }
                }
            }
        }
        // Optional: can handle clicks here too!
        _ => {}
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
    };

    if let Some(index) = app.tabs.iter().position(|t| t.id == target_id) {
        match response {
            NetworkResponse::DownloadProgress(_, downloaded, total) => {
                let tab = &mut app.tabs[index];
                tab.download_state = Some(crate::models::Download {
                    _id: target_id,
                    filename: String::from("Downloading..."),
                    bytes_downloaded: downloaded,
                    total_size: total,
                    status: crate::models::DownloadStatus::Active,
                });
                // Update status message for footer
                tab.status_message = match total {
                    Some(t) => format!("Downloading: {}%", (downloaded * 100) / t),
                    None => format!("Downloading: {} bytes", downloaded),
                };
            }
            NetworkResponse::DownloadFinished(_, filename) => {
                let tab = &mut app.tabs[index];
                //tab.download_state = None; // Clear progress state
                if let Some(ref mut d) = tab.download_state {
                    d.status = crate::models::DownloadStatus::Completed;
                    d.filename = filename.clone();
                }
                tab.status_message = format!("Download complete: {}", filename);
            }
            NetworkResponse::Success(_, title, html_source) => {
                let tab = &mut app.tabs[index];
                tab.page_title = title;
                tab.html_source = html_source;
                tab.scroll = 0;
                tab.status_message = String::from("Loaded");
                app.render_tab(index, terminal_width);
            }
            NetworkResponse::Error(_, msg) => {
                let tab = &mut app.tabs[index];
                if let Some(ref mut d) = tab.download_state {
                    d.status = crate::models::DownloadStatus::Failed(msg.clone());
                }
                tab.page_title = String::from("Error");
                tab.html_source = format!("<h1>Error</h1><hr><p style='color:red'>{}</p>", msg);
                tab.scroll = 0;
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
