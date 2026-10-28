//! Running rynx in a pseudo-terminal, typing and clicking, and reading the screen back

#![allow(dead_code)]

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ROWS: u16 = 40;
pub const COLS: u16 = 140;

/// A home folder of its own, removed afterwards
pub struct Home(pub PathBuf);

impl Home {
    pub fn new(name: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("rynx-terminal-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Downloads")).unwrap();
        // Canonical, as the sandbox compares real paths (/var is /private/var on macOS)
        Home(dir.canonicalize().unwrap())
    }

    /// Where rynx keeps its settings in this home
    pub fn config(&self) -> PathBuf {
        let dir = if cfg!(target_os = "macos") {
            self.0.join("Library/Application Support/rynx-browser")
        } else {
            self.0.join(".config/rynx-browser")
        };
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// settings.conf with these lines, plus downloads here and no update checks unless asked
    pub fn settings(&self, lines: &str) {
        let mut text = format!("download_dir = {}\n", self.downloads().display());
        if !lines.contains("updates =") {
            text.push_str("updates = off\n");
        }
        text.push_str(lines);
        std::fs::write(self.config().join("settings.conf"), text).unwrap();
    }

    pub fn downloads(&self) -> PathBuf {
        self.0.join("Downloads")
    }

    /// A file in the settings folder, if it's there
    pub fn read_config(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.config().join(name)).ok()
    }

    /// Wait for a file to have these contents (downloads finish in the background)
    pub fn wait_for_file(&self, path: &Path, contents: &[u8]) {
        for _ in 0..150 {
            if std::fs::read(path).is_ok_and(|d| d == contents) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("{} never had the expected contents (has {:?})", path.display(), std::fs::read(path).map(|d| d.len()));
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What the program asked the terminal to do besides drawing
#[derive(Default)]
pub struct Requests {
    pub titles: Vec<String>,
    pub clipboard: Vec<String>,
}

impl vt100::Callbacks for Requests {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.titles.push(String::from_utf8_lossy(title).into_owned());
    }
    fn set_window_icon_name(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.titles.push(String::from_utf8_lossy(title).into_owned());
    }
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, _: &[u8], data: &[u8]) {
        self.clipboard.push(String::from_utf8_lossy(data).into_owned());
    }
}

/// rynx running in a pseudo-terminal
pub struct Rynx {
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    screen: Arc<Mutex<vt100::Parser<Requests>>>,
    /// Everything it wrote
    output: Arc<Mutex<Vec<u8>>>,
}

impl Rynx {
    pub fn start(home: &Home) -> Rynx {
        Rynx::start_with(Path::new(env!("CARGO_BIN_EXE_rynx")), home, &[], &[])
    }

    pub fn start_args(home: &Home, args: &[&str]) -> Rynx {
        Rynx::start_with(Path::new(env!("CARGO_BIN_EXE_rynx")), home, args, &[])
    }

    pub fn start_with(program: &Path, home: &Home, args: &[&str], env: &[(&str, String)]) -> Rynx {
        let pty = native_pty_system()
            .openpty(PtySize { rows: ROWS, cols: COLS, pixel_width: 0, pixel_height: 0 })
            .expect("a pseudo-terminal");
        let mut command = CommandBuilder::new(program);
        command.args(args);
        // A clean environment: the terminal running the tests mustn't change what rynx does
        command.env_clear();
        command.env("HOME", &home.0);
        command.env("TERM", "xterm-256color");
        command.env("LANG", "en_US.UTF-8");
        command.env("PATH", std::env::var("PATH").unwrap_or_default());
        for (key, value) in env {
            command.env(key, value);
        }
        command.cwd(&home.0);
        let child = pty.slave.spawn_command(command).expect("rynx starts");
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let writer = pty.master.take_writer().unwrap();
        let screen = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            ROWS.try_into().unwrap(),
            COLS.try_into().unwrap(),
            0,
            Requests::default(),
        )));
        let output = Arc::new(Mutex::new(Vec::new()));
        let (into_screen, into_output) = (Arc::clone(&screen), Arc::clone(&output));
        std::thread::spawn(move || {
            let mut buffer = [0u8; 16384];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                into_screen.lock().unwrap().process(&buffer[..n]);
                into_output.lock().unwrap().extend_from_slice(&buffer[..n]);
            }
        });
        Rynx { child, writer, master: pty.master, screen, output }
    }

    /// The screen's text, one line per row
    pub fn screen(&self) -> String {
        self.screen.lock().unwrap().screen().contents()
    }

    /// The screen's text with runs of spaces squeezed, so words wrapped in borders still match
    pub fn text(&self) -> String {
        self.screen().split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The screen's rows
    pub fn rows(&self) -> Vec<String> {
        self.screen.lock().unwrap().screen().rows(0, COLS).collect()
    }

    /// Titles and clipboard contents the program asked the terminal for
    pub fn requests(&self) -> (Vec<String>, Vec<String>) {
        let parser = self.screen.lock().unwrap();
        (parser.callbacks().titles.clone(), parser.callbacks().clipboard.clone())
    }

    /// Everything the program wrote, as text
    pub fn output(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    /// The status line's text (in the page's top border)
    pub fn status(&self) -> String {
        let text = self.text();
        text.split("[Status: ").nth(1).and_then(|s| s.split(']').next()).unwrap_or("").to_string()
    }

    /// Wait until `wanted` is on the screen
    pub fn wait_for(&self, wanted: &str) {
        self.wait_for_within(wanted, Duration::from_secs(15));
    }

    pub fn wait_for_within(&self, wanted: &str, limit: Duration) {
        let start = Instant::now();
        while start.elapsed() < limit {
            if self.text().contains(wanted) {
                return;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("\"{wanted}\" never appeared. The screen:\n{}", self.screen());
    }

    /// Wait until `gone` is no longer on the screen
    pub fn wait_gone(&self, gone: &str) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if !self.text().contains(gone) {
                return;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("\"{gone}\" stayed. The screen:\n{}", self.screen());
    }

    /// Wait for the screen to stop changing (a page laid out, a status settled)
    pub fn settle(&self) {
        let mut last = self.screen();
        let mut still = 0;
        let start = Instant::now();
        while still < 4 && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(60));
            let now = self.screen();
            if now == last {
                still += 1;
            } else {
                still = 0;
                last = now;
            }
        }
    }

    /// Type text (each byte as a key press)
    pub fn keys(&mut self, keys: &str) {
        self.writer.write_all(keys.as_bytes()).unwrap();
        self.writer.flush().unwrap();
        // Time to handle each, as a person typing would give it
        std::thread::sleep(Duration::from_millis(120));
    }

    /// Type text into a field, as fast as a person typing quickly
    pub fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            let mut buffer = [0u8; 4];
            self.writer.write_all(ch.encode_utf8(&mut buffer).as_bytes()).unwrap();
            self.writer.flush().unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    /// Type an address into the address bar and go
    pub fn open(&mut self, address: &str) {
        self.keys("e");
        // Ctrl+U clears what's there
        self.keys("\u{15}");
        self.type_text(address);
        self.keys("\r");
    }

    /// Cells of the page area (inside its border) drawn reversed: the cursor
    pub fn reversed_cells(&self) -> Vec<(u16, u16)> {
        self.page_cells(|cell| cell.inverse())
    }

    /// Cells of the page area with this background (ANSI colour number: 2 green, 3 yellow, 6 cyan)
    pub fn cells_with_background(&self, colour: u8) -> Vec<(u16, u16)> {
        self.page_cells(|cell| cell.bgcolor() == vt100::Color::Idx(colour))
    }

    fn page_cells(&self, wanted: impl Fn(&vt100::Cell) -> bool) -> Vec<(u16, u16)> {
        let parser = self.screen.lock().unwrap();
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let mut found = Vec::new();
        // The page sits below the tab bar and address bar (rows 0..=5) and its top border (6)
        for row in 7..rows.saturating_sub(1) {
            for col in 1..cols.saturating_sub(1) {
                if screen.cell(row, col).is_some_and(&wanted) {
                    found.push((row, col));
                }
            }
        }
        found
    }

    /// Where `text` is on the screen: (row, column), from 0
    pub fn find(&self, text: &str) -> Option<(u16, u16)> {
        self.rows()
            .iter()
            .enumerate()
            .find_map(|(row, line)| line.find(text).map(|byte| (row as u16, line[..byte].chars().count() as u16)))
    }

    fn mouse(&mut self, button: u8, row: u16, col: u16) {
        // SGR mouse reports, which rynx asks for: press, then release
        let press = format!("\x1b[<{};{};{}M", button, col + 1, row + 1);
        let release = format!("\x1b[<{};{};{}m", button, col + 1, row + 1);
        self.writer.write_all(press.as_bytes()).unwrap();
        if button < 64 {
            self.writer.write_all(release.as_bytes()).unwrap();
        }
        self.writer.flush().unwrap();
        std::thread::sleep(Duration::from_millis(120));
    }

    /// Scroll from the top of the page until `text` is on screen
    pub fn scroll_to(&mut self, text: &str) {
        self.keys("g");
        for _ in 0..30 {
            if self.find(text).is_some() {
                return;
            }
            self.keys("\u{1b}[6~");
            self.settle();
        }
        panic!("\"{text}\" isn't on the page. The screen:\n{}", self.screen());
    }

    pub fn click(&mut self, row: u16, col: u16) {
        self.mouse(0, row, col);
    }

    /// Click on `text` where it is on the screen
    pub fn click_text(&mut self, text: &str) {
        let (row, col) =
            self.find(text).unwrap_or_else(|| panic!("no \"{text}\" to click. The screen:\n{}", self.screen()));
        self.click(row, col);
    }

    /// Ctrl+click on `text`
    pub fn ctrl_click_text(&mut self, text: &str) {
        let (row, col) =
            self.find(text).unwrap_or_else(|| panic!("no \"{text}\" to click. The screen:\n{}", self.screen()));
        self.mouse(16, row, col);
    }

    /// Scroll the wheel down (positive) or up, over the page
    pub fn wheel(&mut self, steps: i32) {
        let button = if steps > 0 { 65 } else { 64 };
        for _ in 0..steps.abs() {
            let press = format!("\x1b[<{};{};{}M", button, COLS / 2, ROWS / 2);
            self.writer.write_all(press.as_bytes()).unwrap();
        }
        self.writer.flush().unwrap();
        std::thread::sleep(Duration::from_millis(150));
    }

    /// Resize the terminal, as dragging its window does
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).unwrap();
        self.screen.lock().unwrap().set_size(rows.max(1).try_into().unwrap(), cols.max(1).try_into().unwrap());
        std::thread::sleep(Duration::from_millis(250));
    }

    pub fn is_running(&mut self) -> bool {
        self.child.try_wait().unwrap().is_none()
    }

    /// The process's memory in use (resident), in MB
    pub fn memory_mb(&self) -> u64 {
        let pid = self.child.process_id().expect("a pid");
        let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().unwrap_or(0) / 1024
    }

    /// Quit with q and wait for it to exit cleanly
    pub fn quit(&mut self) {
        self.keys("q");
        assert!(self.wait_exit(), "rynx exited with an error. The screen:\n{}", self.screen());
    }

    /// Wait for it to exit, returning whether it exited cleanly
    pub fn wait_exit(&mut self) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status.success();
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        panic!("rynx didn't exit. The screen:\n{}", self.screen());
    }

    /// Whether the terminal was given back as it was: out of the alternate screen, cursor shown
    pub fn left_terminal_as_found(&self) -> bool {
        let text = self.output();
        let after = |on: &str, off: &str| text.rfind(off).is_some_and(|o| text.rfind(on).is_none_or(|n| o > n));
        after("\x1b[?1049h", "\x1b[?1049l") && after("\x1b[?25l", "\x1b[?25h") && after("\x1b[?1000h", "\x1b[?1000l")
    }
}

impl Drop for Rynx {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A small local web site, served on another thread for as long as it's kept
pub struct Site {
    pub server: wiremock::MockServer,
    pub runtime: tokio::runtime::Runtime,
}

impl Site {
    /// Pages of HTML at these paths (for GET)
    pub fn new(pages: &[(&str, &str)]) -> Site {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let server = runtime.block_on(wiremock::MockServer::start());
        let site = Site { server, runtime };
        for (at, html) in pages {
            site.page(at, html);
        }
        site
    }

    pub fn page(&self, at: &str, html: &str) {
        self.respond(at, wiremock::ResponseTemplate::new(200).set_body_raw(html.as_bytes().to_vec(), "text/html"));
    }

    /// Any response at a path (for GET)
    pub fn respond(&self, at: &str, response: wiremock::ResponseTemplate) {
        use wiremock::matchers::{method, path};
        self.runtime
            .block_on(wiremock::Mock::given(method("GET")).and(path(at)).respond_with(response).mount(&self.server));
    }

    pub fn mount(&self, mock: wiremock::Mock) {
        self.runtime.block_on(mock.mount(&self.server));
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.server.uri(), path)
    }

    /// How many requests reached `path`
    pub fn requests(&self, path: &str) -> usize {
        self.runtime
            .block_on(self.server.received_requests())
            .unwrap_or_default()
            .iter()
            .filter(|r| r.url.path() == path)
            .count()
    }

    /// The requests that reached `path`
    pub fn received(&self, path: &str) -> Vec<wiremock::Request> {
        self.runtime
            .block_on(self.server.received_requests())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.url.path() == path)
            .collect()
    }
}

/// A long page: `n` numbered paragraphs, with ids
pub fn long_page(title: &str, n: usize) -> String {
    let mut html = format!("<title>{title}</title><h1>{title}</h1>");
    for i in 1..=n {
        html.push_str(&format!("<p id=\"p{i}\">Paragraph {i} of {title}, with some words to fill the line.</p>"));
    }
    html
}

pub fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
