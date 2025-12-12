use url::Url;
use regex::Regex;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseEventKind, MouseButton, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap, Tabs, Clear},
    Terminal,
};
use std::{error::Error, io, time::Duration};
use tokio::sync::mpsc;

// MESSAGING
// This enum defines what the Background Task can send to the Main Thread
enum NetworkResponse {
    Success(usize, String, String), //tabid title content
    Error(usize, String), //tabid message
    Loading(usize), //tabid
}

#[derive(Clone)]
struct LinkRegion {
    url: String,
    line_index: usize, // Which line is this on?
    x_start: usize,    // Column start
    x_end: usize,      // Column end
}

struct BrowserTab {
    id: usize,              // Unique ID to track network requests
    url_input: String,
    rendered_content: Vec<Line<'static>>,
    link_regions: Vec<LinkRegion>,
    page_title: String,
    scroll: usize,
    history: Vec<String>,
    selected_link_index: usize,
    input_mode: InputMode,  // Each tab can be in a different mode!
}

impl BrowserTab {
    fn new(id: usize, initial_url: String) -> Self {
        let (lines, links) = layout_page("NAVIGATION\n
                                          Up / Down Arrow: Scroll page by 1 line.\n
                                          Scroll Wheel: Scroll page by 3 lines.\n
                                          Tab: Cycle selection through links on the screen.\n
                                          Enter: Open the currently selected link.\n
                                          Mouse Left Click: Open the clicked link.\n
                                          Ctrl + Click: Open the clicked link in a New Tab.\n
                                          Backspace / Left Arrow: Go back in history.\n
                                          --------------------------------------------------\n
                                          BROWSER CONTROL\n
                                          t: Open the currently selected link (highlighted via Tab) in a New Tab.\n
                                          n: Open a blank New Tab.\n
                                          w: Close the current tab.\n
                                          [ and ]: Switch between Previous / Next tab.\n
                                          e: Enter 'Edit Mode' to type a new URL.\n
                                          p: Toggle i2p proxy mode (BETA prototype and not fully implemented + tested)\n
                                          q: Quit the browser.\n
                                          --------------------------------------------------\n
                                          EDIT MODE (after pressing 'e')\n
                                          Typing: Type URL or Search Query. (search broken because of anti AI scraper measures)\n
                                          Enter: Submit request.\n
                                          Esc: Cancel and return to Normal Mode.\n
                                          --------------------------------------------------\n
                                          SUPPLEMENTARY NOTES\n
                                          Weird things may happen when you encounter Javascript heavy sites, especially\n
                                          when they have anti-AI scraping counter measures. You may need to just quit and restart.\n");
        BrowserTab {
            id,
            url_input: initial_url,
            rendered_content: lines,
            link_regions: links,
            page_title: String::from("New Tab"),
            scroll: 0,
            history: Vec::new(),
            selected_link_index: 0,
            input_mode: InputMode::Normal,
        }
    }
}

struct App {
    tabs: Vec<BrowserTab>,  // The list of open tabs
    active_tab_index: usize,// Which one are we looking at?
    id_counter: usize,      // To generate unique IDs
    tx: mpsc::Sender<NetworkResponse>,
    rx: mpsc::Receiver<NetworkResponse>,
    i2p_mode: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum InputMode {
    Normal,
    Editing,
}

fn strict_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        // 1. Stop infinite loops (limit to 10 redirects)
        if attempt.previous().len() > 10 {
            return attempt.error("Too many redirects");
        }

        // 2. Prevent SSRF: Don't allow redirects to localhost/127.0.0.1
        if let Some(host) = attempt.url().host_str() {
            if host == "localhost" || host == "127.0.0.1" || host == "::1" {
                return attempt.error("Blocked redirect to local network (SSRF Protection)");
            }
        }

        attempt.follow()
    })
}

impl App {
    fn new(tx: mpsc::Sender<NetworkResponse>, rx: mpsc::Receiver<NetworkResponse>) -> App {
        let initial_tab = BrowserTab::new(0, String::from("https://www.rust-lang.org"));
        App {
            tabs: vec![initial_tab],
            active_tab_index: 0,
            id_counter: 1, // Start next ID at 1
            tx,
            rx,
            i2p_mode: false, //clearweb
        }
    }

    // Helper to get the currently active tab
    fn current_tab(&mut self) -> &mut BrowserTab {
        &mut self.tabs[self.active_tab_index]
    }

    // Helper to open a URL in a new tab and immediately fetch it
    fn open_link_in_new_tab(&mut self, url: String) {
        let new_tab = BrowserTab::new(self.id_counter, url);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1; // Switch to it
        self.id_counter += 1;
        self.submit_request(); // Fetch immediately
    }

    fn add_tab(&mut self, url: Option<String>) {
        let start_url = url.unwrap_or_else(|| String::from("https://www.rust-lang.org"));
        let new_tab = BrowserTab::new(self.id_counter, start_url);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1; // Auto-switch to new tab
        self.id_counter += 1;
    }

    fn close_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.tabs.remove(self.active_tab_index);
            // Prevent index out of bounds if we closed the last tab
            if self.active_tab_index >= self.tabs.len() {
                self.active_tab_index = self.tabs.len() - 1;
            }
        }
    }
    fn submit_request(&mut self) {
        let tab = self.current_tab();
        let mut target_url = tab.url_input.clone();

        // URL Normalization
        if !target_url.starts_with("http://") && !target_url.starts_with("https://") {
            if target_url.contains('.') && !target_url.contains(' ') {
                target_url = format!("https://{}", target_url);
            } else {
                let safe_query = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("query", &target_url)
                    .finish();
                target_url = format!("https://search.marginalia.nu/search?{}", safe_query);
            }
        }

        tab.url_input = target_url.clone();
        let id = tab.id;
        let tx_clone = self.tx.clone();
        let use_i2p = self.i2p_mode;

        tokio::spawn(async move {
            let _ = tx_clone.send(NetworkResponse::Loading(id)).await;

            // --- FIX START: Construct Headers Correctly ---
            let mut headers = reqwest::header::HeaderMap::new();
            // We use from_static for string literals
            headers.insert("Referer", reqwest::header::HeaderValue::from_static(""));

            let mut builder = reqwest::Client::builder()
                .user_agent("RustBrowser/0.1.0 (your_email@example.com) reqwest/0.12")
                .timeout(Duration::from_secs(10))
                .default_headers(headers) // <--- Use default_headers() here
                .redirect(strict_redirect_policy());

            if use_i2p {
                if let Ok(proxy) = reqwest::Proxy::http("http://127.0.0.1:4444") {
                    builder = builder.proxy(proxy);
                }
            }

            match builder.build() {
                Ok(client) => {
                    match client.get(&target_url).send().await {
                        Ok(resp) => {
                            // --- SECURITY: Size Limit ---
                            if let Some(len) = resp.content_length() {
                                if len > 10 * 1024 * 1024 {
                                    let _ = tx_clone.send(NetworkResponse::Error(id, "Page too large (>10MB)".to_string())).await;
                                    return;
                                }
                            }

                            match resp.text().await {
                                Ok(html_text) => {
                                    let title = extract_title(&html_text);
                                    
                                    // Run our improved cleaner (fixes glitches)
                                    let cleaned_html = clean_html(&html_text);
                                    let markdown = html2md::parse_html(&cleaned_html);

                                    let _ = tx_clone.send(NetworkResponse::Success(id, title, markdown)).await;
                                }
                                Err(e) => { let _ = tx_clone.send(NetworkResponse::Error(id, e.to_string())).await; }
                            }
                        },
                        Err(e) => { let _ = tx_clone.send(NetworkResponse::Error(id, e.to_string())).await; }
                    }
                }
                Err(e) => { let _ = tx_clone.send(NetworkResponse::Error(id, e.to_string())).await; }
            }
        });
    }
}


fn layout_page(markdown: &str) -> (Vec<Line<'static>>, Vec<LinkRegion>) {
    let mut rendered_lines = Vec::new();
    let mut link_map = Vec::new();

    for (line_idx, line) in markdown.lines().enumerate() {
        // Headers (Red & Bold)
        if line.starts_with("# ") {
            let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
            rendered_lines.push(Line::from(Span::styled(line.to_string(), style)));
            continue;
        }

        // Manual Parser for Links
        let mut spans = Vec::new();
        let mut visual_x = 0; // Tracks the column index on screen

        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            // Check for the start of a link: '[' followed eventually by ']' then '('
            if chars[i] == '[' {
                // Attempt to find the matching ']'
                let mut text_end = None;
                for j in (i + 1)..chars.len() {
                    if chars[j] == ']' {
                        text_end = Some(j);
                        break;
                    }
                }

                // If we found ']', check if the very next char is '('
                if let Some(end_bracket) = text_end {
                    if end_bracket + 1 < chars.len() && chars[end_bracket + 1] == '(' {
                        // WE FOUND A LINK!
                        let link_text: String = chars[(i + 1)..end_bracket].iter().collect();

                        // Now, parse the URL by counting balanced parentheses
                        let url_start = end_bracket + 2;
                        let mut paren_balance = 1;
                        let mut url_end = None;

                        for k in url_start..chars.len() {
                            if chars[k] == '(' {
                                paren_balance += 1;
                            } else if chars[k] == ')' {
                                paren_balance -= 1;
                                if paren_balance == 0 {
                                    url_end = Some(k);
                                    break;
                                }
                            }
                        }

                        if let Some(end_paren) = url_end {
                            let url: String = chars[url_start..end_paren].iter().collect();

                            // Render the Link Text (Cyan/Underlined)
                            let style = Style::default().fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
                            spans.push(Span::styled(link_text.clone(), style));

                            // Register the Hitbox
                            let link_len = link_text.chars().count();
                            link_map.push(LinkRegion {
                                url: url, // Now contains the FULL url with parens!
                                line_index: line_idx,
                                x_start: visual_x,
                                x_end: visual_x + link_len,
                            });

                            visual_x += link_len;
                            i = end_paren + 1; // Skip past the entire link markup
                            continue;
                        }
                    }
                }
            }

            // If not a link (or a broken one), just print the character
            spans.push(Span::raw(chars[i].to_string()));
            visual_x += 1;
            i += 1;
        }

        rendered_lines.push(Line::from(spans));
    }

    (rendered_lines, link_map)
}

fn resolve_url(base: &str, target: &str) -> String {
    // 1. Try to parse the base URL (e.g., "https://rust-lang.org")
    if let Ok(base_url) = Url::parse(base) {
        // 2. Use the standard .join() method
        // This handles ".." parent directories and root "/" paths automatically
        if let Ok(joined) = base_url.join(target) {
            return joined.to_string();
        }
    }
    // If logic fails (or if target is already absolute like "https://google.com"), 
    // just return the target as is.
    target.to_string()
}

fn clean_html(html: &str) -> String {
    // 1. Remove <style>...</style> blocks
    let re_style = Regex::new(r"(?s)<style.*?>.*?</style>").unwrap();
    let no_style = re_style.replace_all(html, "");

    // 2. Remove <script>...</script> blocks
    let re_script = Regex::new(r"(?s)<script.*?>.*?</script>").unwrap();
    let no_script = re_script.replace_all(&no_style, "");

    // 3. Remove comments
    let re_comments = Regex::new(r"(?s)<!--.*?-->").unwrap();
    let result = re_comments.replace_all(&no_script, "");
   
    // 4. SECURITY: Strip ANSI Escape Codes (Terminal Injection)
    let re_ansi = Regex::new(r"[\u001b\u009b]\[[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]").unwrap();
    let no_ansi = re_ansi.replace_all(&result, "");

    // 5. CLEANUP: Strip Control Characters
    // This removes null bytes (\x00) and other non-printable junk
    let re_control = Regex::new(r"[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]").unwrap();
    let safe_result = re_control.replace_all(&no_ansi, "");

    safe_result.to_string()
}

fn extract_title(html: &str) -> String {
    let re = Regex::new(r"(?is)<title>(.*?)</title>").unwrap();
    
    if let Some(caps) = re.captures(html) {
        if let Some(matched) = caps.get(1) {
            // Decode HTML entities (optional, but good for "Rust &amp; Friends")
            // For now, we just trim whitespace
            return matched.as_str().trim().to_string();
        }
    }
    String::from("No Title")
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
    let (tx, rx) = mpsc::channel(10);
    let app = App::new(tx, rx);

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
        terminal.draw(|f| ui(f, &app))?;

        // HANDLE NETWORK
        if let Ok(response) = app.rx.try_recv() {
            let mut update_tab = |id: usize, updater: &dyn Fn(&mut BrowserTab)| {
                if let Some(tab) = app.tabs.iter_mut().find(|t| t.id == id) {
                    updater(tab);
                }
            };

            match response {
                NetworkResponse::Success(id, title, md) => update_tab(id, &|t| {
                    t.page_title = title.clone();
                    let (rendered, links) = layout_page(&md);
                    t.scroll = 0;
                    t.rendered_content = rendered;
                    t.link_regions = links;
                    t.selected_link_index = 0; // Reset link selection
                }),
                NetworkResponse::Error(id, msg) => update_tab(id, &|t| {
                    //t.content_lines = vec![format!("Error: {}", msg)];
                    let (rendered, links) = layout_page(&format!("Error: {}", msg));
                    t.rendered_content = rendered;
                    t.link_regions = links;
                    t.scroll = 0;
                }),
                NetworkResponse::Loading(id) => update_tab(id, &|t| {
                    t.page_title = String::from("Loading...");
                }),
            }
        }

        // HANDLE INPUT
        if event::poll(Duration::from_millis(10))? {
            match event::read()? {
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
                            
                            // HISTORY BACK
                            KeyCode::Backspace | KeyCode::Left => {
                                let tab = app.current_tab();
                                if let Some(previous_url) = tab.history.pop() {
                                    tab.url_input = previous_url;
                                    app.submit_request();
                                }
                            }

                            // LINK NAVIGATION (Tab)
                            KeyCode::Tab => {
                                let tab = app.current_tab();
                                if !tab.link_regions.is_empty() {
                                    tab.selected_link_index = (tab.selected_link_index + 1) % tab.link_regions.len();
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
                                let click_x = mouse.column as usize;

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


//UI RENDERING

fn ui(f: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab Bar
            Constraint::Length(3), // URL Input
            Constraint::Min(0),    // Content
        ].as_ref())
        .split(f.area());

    // RENDER TABS WIDGET
    let titles: Vec<Line> = app.tabs
        .iter()
        .map(|t| Line::from(format!(" {} ", t.page_title)))
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.active_tab_index)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    f.render_widget(tabs, chunks[0]);

    // RENDER ACTIVE TAB CONTENT
    // We only draw the tab that is currently selected
    let active_tab = &app.tabs[app.active_tab_index];

    // Render Input (same as before, but using active_tab data)
    let input_style = match active_tab.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Editing => Style::default().fg(Color::Yellow),
    };
    // Add a mode indicator to the URL block title
    let mode_text = if app.i2p_mode { " [I2P MODE ON] " } else { " [Clearweb] " };
    let input_block_title = format!("URL - {}", mode_text);

    let input = Paragraph::new(active_tab.url_input.as_str())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(input_block_title));
    f.render_widget(input, chunks[1]);

    // Calculate Area
    // We need to know how tall the content area is to know how many lines to grab.
    let content_area_height = chunks[2].height as usize;

    // Calculate Slice Boundaries
    let start_index = active_tab.scroll;
    let total_lines = active_tab.rendered_content.len();
    let end_index = (start_index + content_area_height).min(total_lines);


    let viewport_content = if start_index < total_lines {
        // We clone the Line objects (which are just pointers to Heap data)
        // This is very cheap compared to parsing text
        active_tab.rendered_content[start_index..end_index].to_vec()
    } else {
        Vec::new()
    };

    // Draw
    let content = Paragraph::new(viewport_content) // Pass the Vec<Line> directly!
        .wrap(Wrap { trim: true })
        .scroll((0, 0))
        .block(Block::default().borders(Borders::ALL).title("Browser"));

    f.render_widget(Clear, chunks[2]);
    f.render_widget(content, chunks[2]);
}
