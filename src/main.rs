use rynx_browser::app::App;
use rynx_browser::event_handler::{handle_key_event, handle_mouse_event, process_network_events};
use rynx_browser::ui::ui;

use std::{error::Error, io, time::Duration};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};

// MAIN LOOP (ASYNC)
#[tokio::main] // This macro turns main() into an async runtime
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
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

    // Setup Channel
    let (tx, rx) = tokio::sync::mpsc::channel(rynx_browser::constants::CHANNEL_CAPACITY);
    let app = App::new(tx, rx)?;

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
    let mut size = terminal.size()?;
    loop {
        terminal.draw(|f| ui(f, &app))?;

        // Handle network events
        process_network_events::<B>(&mut app, size.width)?;

        // Handle input events: wait briefly for the first, then everything already queued before
        // redrawing. One event per frame let touchpad bursts pile up and replay long after.
        let mut timeout = Duration::from_millis(rynx_browser::constants::EVENT_POLL_TIMEOUT_MS);
        while event::poll(timeout)? {
            timeout = Duration::ZERO;
            match event::read()? {
                Event::Resize(width, height) => {
                    size.width = width;
                    size.height = height;
                    app.resize_all_tabs(width);
                }
                Event::Key(key) => {
                    if handle_key_event::<B>(&mut app, key, size.width, size.height)? {
                        return Ok(()); // Quit signal received
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event::<B>(&mut app, mouse, size.width, size.height)?;
                }
                _ => {}
            }
        }
    }
}
