use rynx::app::App;
use rynx::bookmarks::Bookmarks;
use rynx::event_handler::{handle_key_event, handle_mouse_event, process_network_events};
use rynx::history::History;
use rynx::sandbox::SandboxStatus;
use rynx::settings::Settings;
use rynx::ui::ui;

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
    while event::poll(Duration::from_millis(rynx::constants::EXIT_DRAIN_TIMEOUT_MS))? {
        let _ = event::read()?;
    }
    Ok(())
}

const USAGE: &str = "\
Usage: rynx [OPTIONS]

A terminal web browser with no JavaScript.

Options:
      --no-sandbox  Don't restrict filesystem access (Linux, macOS). Use only to troubleshoot, e.g. if
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
                println!("rynx {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                println!("{}", USAGE);
                return Ok(());
            }
            other => {
                eprintln!("rynx: unknown option '{}'\n\n{}", other, USAGE);
                std::process::exit(2);
            }
        }
    }

    // How this copy was installed decides how it's updated; a waiting update is installed now,
    // before the sandbox keeps rynx from writing to its own program file, and then started
    let installed = rynx::update::installed();
    let update_dir = rynx::settings::cache_dir().map(|d| d.join("update"));
    let mut update_note = std::env::var("RYNX_UPDATED").ok().map(|v| format!("Updated to rynx {}", v));
    if let (rynx::update::Installed::Updatable(program), Some(dir)) = (&installed, &update_dir) {
        match rynx::update::install_pending(dir, None) {
            Ok(Some(version)) => restart(program, &version),
            Ok(None) => {}
            Err(e) => update_note = Some(format!("The update wasn't installed: {}", e)),
        }
    }

    // Settings are read (and the settings folder created) before sandboxing
    let config_dir = rynx::settings::config_dir();
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
        // The cache and snapshot folders are made now even if unused (they stay empty), so turning
        // the cache on or tracking a site works without restarting
        let (cache_dir, snapshots_dir) = (rynx::settings::cache_dir(), rynx::settings::snapshots_dir());
        for dir in
            [Some(&download_dir), config_dir.as_ref(), cache_dir.as_ref(), snapshots_dir.as_ref()].into_iter().flatten()
        {
            if !dir.as_os_str().is_empty() && std::fs::create_dir_all(dir).is_ok() {
                writable.push(dir);
            }
        }
        rynx::sandbox::restrict_filesystem(&writable)
    } else {
        SandboxStatus::Disabled
    };

    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run(
        settings,
        problems,
        config_dir,
        sandbox,
        (installed, update_dir, update_note),
    ))
}

/// Start the newly installed version in place of this one, with the same arguments
fn restart(program: &std::path::Path, version: &str) -> ! {
    let mut command = std::process::Command::new(program);
    command.args(std::env::args_os().skip(1)).env("RYNX_UPDATED", version);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        eprintln!("rynx: updated to {}, but couldn't start it ({}); start rynx again", version, error);
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(error) => {
                eprintln!("rynx: updated to {}, but couldn't start it ({}); start rynx again", version, error);
                std::process::exit(1);
            }
        }
    }
}

async fn run(
    settings: Settings,
    settings_problems: Vec<String>,
    config_dir: Option<std::path::PathBuf>,
    sandbox: SandboxStatus,
    (installed, update_dir, update_note): (rynx::update::Installed, Option<std::path::PathBuf>, Option<String>),
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
    let (tx, rx) = tokio::sync::mpsc::channel(rynx::constants::CHANNEL_CAPACITY);
    let mut app = App::with_settings(tx, rx, settings)?;
    app.config_dir = config_dir.clone();
    // Asked now: the terminal answers on standard input, which the event loop reads from later
    app.images.detect(app.settings.image_protocol);
    app.cache = std::sync::Arc::new(rynx::cache::Cache::new(rynx::settings::cache_dir()));
    app.cache.configure(app.settings.disk_cache, app.settings.disk_cache_i2p, app.settings.cache_size_mb);
    app.snapshots = std::sync::Arc::new(rynx::snapshots::Snapshots::load(
        rynx::settings::snapshots_dir(),
        config_dir.as_ref().map(|d| d.join(rynx::snapshots::TRACKED_FILE)),
    ));
    if let Some(dir) = &config_dir {
        app.bookmarks = Bookmarks::load(dir.join(rynx::bookmarks::BOOKMARKS_FILE));
        app.gemini_hosts = std::sync::Arc::new(std::sync::Mutex::new(rynx::gemini::KnownHosts::load(
            dir.join(rynx::gemini::KNOWN_HOSTS_FILE),
        )));
        if app.settings.save_history {
            app.history = History::load(dir.join(rynx::history::HISTORY_FILE));
        }
    }
    let sandbox_note = sandbox.describe();
    if !sandbox_note.is_empty() {
        app.current_tab().status_message = format!("Ready ({})", sandbox_note);
    }
    if let Some(dir) = &config_dir {
        let (keymap, problems) = rynx::keys::Keymap::load_or_create(dir);
        app.keymap = keymap;
        if let Some(problem) = problems.first() {
            let more = if problems.len() > 1 { " (and more)" } else { "" };
            app.current_tab().status_message =
                format!("Problem in keys.conf, {}{} (about:keys lists them)", problem, more);
        }
    }
    if let Some(problem) = settings_problems.first() {
        let more = if settings_problems.len() > 1 { " (and more)" } else { "" };
        app.current_tab().status_message = format!("Problem in settings.conf, {}{}", problem, more);
    }
    app.installed = installed;
    app.update_dir = update_dir;
    if let Some(note) = &update_note {
        app.current_tab().status_message = note.clone();
    }
    app.update_note = update_note;
    app.open_home_page();
    app.check_for_updates_if_due();

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
        // Images the frame needed that had been dropped from memory are decoded again
        app.restore_missing_images();

        // Handle network events
        process_network_events(&mut app, size.width)?;

        // Handle input events: wait briefly for the first, then everything already queued before
        // redrawing. One event per frame let touchpad bursts pile up and replay long after.
        let mut timeout = Duration::from_millis(rynx::constants::EVENT_POLL_TIMEOUT_MS);
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
