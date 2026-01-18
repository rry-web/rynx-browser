mod app;
mod event_handler;
mod models;
mod network;
mod renderer;
mod ui;

use crate::app::App;
use crate::event_handler::{handle_key_event, handle_mouse_event, handle_network_event};
use crate::models::LinkRegion;
use crate::ui::ui;

use std::{error::Error, io, time::Duration};
use url::Url;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
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

        // Handle network events
        if let Ok(response) = app.rx.try_recv() {
            handle_network_event::<B>(&mut app, response, size.width)?;
        }

        // Handle input events
        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
                Event::Resize(width, _height) => {
                    app.resize_all_tabs(width);
                }
                Event::Key(key) => {
                    if handle_key_event::<B>(&mut app, key, size.width)? {
                        return Ok(()); // Quit signal received
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event::<B>(&mut app, mouse, size.height)?;
                }
                _ => {}
            }
        }
    }
}
