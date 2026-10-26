use rynx_browser::app::App;
use rynx_browser::bookmarks::Bookmarks;
use rynx_browser::event_handler::{handle_key_event, handle_mouse_event, process_network_events};
use rynx_browser::history::History;
use rynx_browser::sandbox::SandboxStatus;
use rynx_browser::settings::Settings;
use rynx_browser::ui::ui;

use std::{error::Error, fmt, io, time::Duration};

use crossterm::{
    Command,
    event::{self, DisableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};

/// Like crossterm's EnableMouseCapture, but without motion reporting (?1003), which the browser
/// never uses and which floods the input queue with an event for every pointer movement.
struct EnableClickAndWheelCapture;

impl Command for EnableClickAndWheelCapture {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        // ?1000: button presses, releases and wheel; ?1006: SGR encoding for large terminals
        f.write_str("\x1B[?1000h\x1B[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        crossterm::event::EnableMouseCapture.execute_winapi()
    }
}

/// Discard input still queued at exit so stray mouse reports don't land in the shell
fn drain_pending_input() -> io::Result<()> {
    while event::poll(Duration::from_millis(rynx_browser::constants::EXIT_DRAIN_TIMEOUT_MS))? {
        let _ = event::read()?;
    }
    Ok(())
}

const USAGE: &str = "\
Usage: rynx_browser [OPTIONS]

A terminal web browser with no JavaScript.

Options:
      --no-sandbox  Don't restrict filesystem access (Linux). Use only to troubleshoot, e.g. if
                    downloads fail because your Downloads folder is somewhere unusual.
  -V, --version     Print the version and exit
  -h, --help        Print this help and exit

Key bindings are listed on the start page and in the README.";

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut sandbox_enabled = true;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-sandbox" => sandbox_enabled = false,
            "-V" | "--version" => {
                println!("rynx_browser {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{}", USAGE);
                return Ok(());
            }
            other => {
                eprintln!("rynx_browser: unknown option '{}'\n\n{}", other, USAGE);
                std::process::exit(2);
            }
        }
    }

    // Settings are read (and the settings folder created) before sandboxing
    let config_dir = rynx_browser::settings::config_dir();
    let (settings, problems) = match &config_dir {
        Some(dir) => Settings::load_or_create(dir),
        None => (Settings::default(), Vec::new()),
    };
    let download_dir = settings.download_dir.clone().unwrap_or_else(App::default_download_dir);

    // Sandbox while this is still the only thread: every thread started afterwards (the async
    // runtime, clipboard, background rendering) inherits the restrictions. The writable folders
    // must exist first, since nothing outside them can be created later.
    let sandbox = if sandbox_enabled {
        let mut writable: Vec<&std::path::Path> = Vec::new();
        for dir in [Some(&download_dir), config_dir.as_ref()].into_iter().flatten() {
            if !dir.as_os_str().is_empty() && std::fs::create_dir_all(dir).is_ok() {
                writable.push(dir);
            }
        }
        rynx_browser::sandbox::restrict_filesystem(&writable)
    } else {
        SandboxStatus::Disabled
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run(settings, problems, config_dir, sandbox))
}

async fn run(
    settings: Settings,
    settings_problems: Vec<String>,
    config_dir: Option<std::path::PathBuf>,
    sandbox: SandboxStatus,
) -> Result<(), Box<dyn Error + Send + Sync>> {
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
    execute!(stdout, EnterAlternateScreen, EnableClickAndWheelCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Setup Channel
    let (tx, rx) = tokio::sync::mpsc::channel(rynx_browser::constants::CHANNEL_CAPACITY);
    let mut app = App::with_settings(tx, rx, settings)?;
    if let Some(dir) = &config_dir {
        app.bookmarks = Bookmarks::load(dir.join(rynx_browser::bookmarks::BOOKMARKS_FILE));
        app.gemini_hosts = std::sync::Arc::new(std::sync::Mutex::new(rynx_browser::gemini::KnownHosts::load(
            dir.join(rynx_browser::gemini::KNOWN_HOSTS_FILE),
        )));
        if app.settings.save_history {
            app.history = History::load(dir.join(rynx_browser::history::HISTORY_FILE));
        }
    }
    let sandbox_note = sandbox.describe();
    if !sandbox_note.is_empty() {
        app.current_tab().status_message = format!("Ready ({})", sandbox_note);
    }
    if let Some(problem) = settings_problems.first() {
        let more = if settings_problems.len() > 1 { " (and more)" } else { "" };
        app.current_tab().status_message = format!("Problem in settings.conf, {}{}", problem, more);
    }
    app.open_home_page();

    // Run App
    let res = run_app(&mut terminal, app).await;

    // Teardown: stop mouse reporting and drain input while still in raw mode
    execute!(terminal.backend_mut(), DisableMouseCapture)?;
    drain_pending_input()?;
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
        std::process::exit(1);
    }

    // Exit now rather than returning: the runtime would wait for any page still being laid out
    // on a background thread, which can't be interrupted. The terminal is already restored.
    std::process::exit(0);
}

async fn run_app<B: Backend<Error = io::Error>>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    let mut size = terminal.size()?;
    // Lay out the start page for the actual terminal width
    app.resize_all_tabs(size.width);
    loop {
        terminal.draw(|f| ui(f, &app))?;

        // Handle network events
        process_network_events(&mut app, size.width)?;

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
                    if handle_key_event(&mut app, key, size.width, size.height)? {
                        return Ok(()); // Quit signal received
                    }
                }
                Event::Mouse(mouse) => {
                    handle_mouse_event(&mut app, mouse, size.width, size.height)?;
                }
                _ => {}
            }
        }
    }
}
