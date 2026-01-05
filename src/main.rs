mod models;
mod network;
mod renderer;
mod app;
mod ui;

use crate::app::App;
use crate::models::{InputMode, LinkRegion};
use crate::network::NetworkResponse;
use crate::ui::ui;

use url::Url;
use std::{error::Error, io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind, MouseButton, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{
    backend::{Backend, CrosstermBackend},
    Terminal,
};

pub fn resolve_url(base: &str, target: &str) -> String {
    // If target is already a full URL (e.g. https://google.com), return it immediately
    if let Ok(url) = Url::parse(target) {
        return url.to_string();
    }

    // Handle internal pages or empty bases
    if base.is_empty() || base.starts_with("about:") || base == "New Tab" {
        // If we are on a help page, relative links can't be resolved,
        // so we treat the target as a potential new absolute URL or search query.
        return target.to_string();
    }

    // Try standard joining
    match Url::parse(base) {
        Ok(base_url) => {
            match base_url.join(target) {
                Ok(joined) => joined.to_string(),
                Err(_) => target.to_string(), // Fallback to target string if join fails
            }
        }
        Err(_) => target.to_string(), // Fallback if base is unparseable
    }
}


// MAIN LOOP (ASYNC)
#[tokio::main] // This macro turns main() into an async runtime
async fn main() -> Result<(), Box<dyn Error>> {
    // This hook catches panics and restores the terminal before printing the error
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(panic_info);
    }));
    // Setup Terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Setup Channel (Capacity of 10 messages)
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let app = App::new(tx, rx);

    // Initialize MCP
    //app.init_mcp().await;
    
    // Run App
    let res = run_app(&mut terminal, app).await;

    // Teardown
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err)
    }

    Ok(())
}


async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        let size = terminal.size()?;

        terminal.draw(|f| ui(f, &app))?;
        // HANDLE NETWORK
        if let Ok(response) = app.rx.try_recv() {
            let target_id = match &response {
                NetworkResponse::Success(id, ..) => *id,
                NetworkResponse::Error(id, ..) => *id,
                NetworkResponse::Loading(id) => *id,
                NetworkResponse::Info(id, ..) => *id,
            };
            if let Some(index) = app.tabs.iter().position(|t| t.id == target_id) {
                match response {
                    NetworkResponse::Success(_, title, html_source) => {
                        let tab = &mut app.tabs[index];
                        tab.page_title = title;
                        tab.html_source = html_source;
                        tab.scroll = 0;
                        tab.status_message = String::from("Loaded");
                        app.render_tab(index, size.width);
                    },
                    NetworkResponse::Error(_, msg) => {
                        let tab = &mut app.tabs[index];
                        tab.page_title = String::from("Error");
                        tab.html_source = format!("<h1>Error</h1><hr><p style='color:red'>{}</p>", msg);
                        tab.scroll = 0;
                        tab.status_message = String::from("Error");
                        app.render_tab(index, size.width);
                    },
                    NetworkResponse::Loading(_) => {
                        let tab = &mut app.tabs[index];
                        tab.page_title = String::from("Loading...");
                        tab.status_message = String::from("Fetching...");
                    },
                    NetworkResponse::Info(_, msg) => {
                        let tab = &mut app.tabs[index];
                        tab.status_message = msg;
                    },
                }
            }
        }

        // HANDLE INPUT
        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
                Event::Resize(width, _height) => {
                    app.resize_all_tabs(width);
                }
                Event::Key(key) => {
                    // Get the mode of the ACTIVE tab
                    let active_mode = app.current_tab().input_mode;
                    match active_mode {
                        InputMode::Normal => match key.code {
                            // --- TAB CONTROLS ---
                            KeyCode::Char('n') => app.add_tab(None),
                            KeyCode::Char('t') => {
                                let tab = app.current_tab();
                                if let Some(region) = tab.link_regions.get(tab.selected_link_index) {
                                    let full_url = resolve_url(&tab.url_input, &region.url);
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
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Char('e') => app.current_tab().input_mode = InputMode::Editing,
                            KeyCode::Down => app.current_tab().scroll = app.current_tab().scroll.saturating_add(1),
                            KeyCode::Up => app.current_tab().scroll = app.current_tab().scroll.saturating_sub(1),
                            KeyCode::Char('v') => {
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
                                app.render_tab(active_index, size.width);
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
                            /*
                            KeyCode::Tab => {
                                let tab = app.current_tab();
                                if !tab.link_regions.is_empty() {
                                    tab.selected_link_index = (tab.selected_link_index + 1) % tab.link_regions.len();
                                    // AUTOSCROLL
                                    let selected = &tab.link_regions[tab.selected_link_index];
                                    let viewport_height = size.height.saturating_sub(6) as usize; // Adjust based on your UI chunks

                                    if selected.line_index < tab.scroll {
                                        tab.scroll = selected.line_index;
                                    } else if selected.line_index >= tab.scroll + viewport_height {
                                        tab.scroll = selected.line_index - viewport_height + 1;
                                    }
                                }
                            }
                            */
                            // main.rs - KeyCode::Tab section
                            KeyCode::Tab => {
                                let tab = app.current_tab();
                                if !tab.link_regions.is_empty() {
                                    tab.selected_link_index = (tab.selected_link_index + 1) % tab.link_regions.len();

                                    // --- IMPROVED AUTOSCROLL ---
                                    let selected = &tab.link_regions[tab.selected_link_index];
                                    // We subtract 6 for the Tab bar (3) and URL bar (3),
                                    // and another 2 for the borders of the Browser block.
                                    let viewport_height = size.height.saturating_sub(8) as usize;

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
                                    let new_url = resolve_url(&tab.url_input, &region.url);
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
                        },

                        // --- EDITING MODE ---
                        InputMode::Editing => match key.code {
                            KeyCode::Enter => {
                                let tab = app.current_tab();
                                // Save history
                                if !tab.url_input.is_empty() {
                                    tab.history.push(tab.url_input.clone());
                                }
                                
                                app.submit_request();
                                app.current_tab().input_mode = InputMode::Normal;
                            }
                            KeyCode::Char(c) => {
                                app.current_tab().url_input.push(c);
                            }
                            KeyCode::Backspace => {
                                app.current_tab().url_input.pop();
                            }
                            KeyCode::Esc => {
                                app.current_tab().input_mode = InputMode::Normal;
                            }
                            _ => {}
                        },
                    }
                }
                Event::Mouse(mouse) => {
                    let tab = app.current_tab();
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            tab.scroll = tab.scroll.saturating_add(3); // Scroll down 3 lines
                        }
                        MouseEventKind::ScrollUp => {
                            tab.scroll = tab.scroll.saturating_sub(3); // Scroll up 3 lines
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            // 1. Determine which line was clicked
                            if mouse.row >= 7 { // 7 is the UI offset
                                let visual_line = (mouse.row - 7) as usize;
                                let real_line_idx = visual_line + tab.scroll;
                                let click_x = (mouse.column as usize).saturating_sub(1);

                                // 2. Search the Link Regions for a match
                                // We filter for links on this specific line
                                let found_link = tab.link_regions.iter().find(|link| {
                                    link.line_index == real_line_idx &&
                                    click_x >= link.x_start &&
                                    click_x < link.x_end
                                });

                                if let Some(region) = found_link {
                                    // 3. Navigate
                                    let full_url = resolve_url(&tab.url_input, &region.url);

                                    if mouse.modifiers.contains(KeyModifiers::CONTROL) {
                                        app.open_link_in_new_tab(full_url);
                                    } else {
                                        if !tab.url_input.is_empty() {
                                            tab.history.push(tab.url_input.clone());
                                        }
                                        tab.url_input = full_url;
                                        app.submit_request();
                                    }
                                }
                            }
                        }
                        // Optional: You can handle clicks here too!
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

