//! One tab: its page, where it's scrolled to, what's typed into its forms, and its own history.

use super::*;

pub struct BrowserTab {
    pub id: usize,
    pub url_input: String,
    pub rendered_content: Vec<Line<'static>>,
    pub link_regions: Vec<LinkRegion>,
    pub page_title: String,
    pub scroll: usize,
    pub history: Vec<HistoryEntry>,
    /// Pages left by going back, for going forward again
    pub forward_history: Vec<HistoryEntry>,
    pub selected_link_index: usize,
    pub input_mode: InputMode,
    pub status_message: String,
    pub html_source: Arc<str>,
    pub is_source_view: bool,
    /// Show only pages' main text; stays on while following links, until turned off
    pub reader_mode: bool,
    pub cursor_line: usize,
    pub cursor_char: usize,
    pub selection: Option<Selection>,
    pub download_state: Option<crate::models::Download>,
    pub search_state: Option<SearchState>,
    pub download_prompt: Option<crate::models::DownloadPrompt>,
    pub tool_prompt: Option<crate::models::ToolPrompt>,
    /// How the next load may use the cache (Back/Forward show the cached copy; Reload asks)
    pub(super) pending_cache_use: fetch::CacheUse,
    /// The page's images, and where loaded ones are drawn in it
    pub page_images: Vec<crate::renderer::PageImage>,
    pub image_blocks: Vec<crate::renderer::ImageBlock>,
    /// Space to reserve for each loaded image, by its address on the page
    pub image_sizes: Arc<HashMap<String, (u16, u16)>>,
    /// A page just arrived: load the images it may load by itself once it's laid out
    pub load_images_after_render: bool,
    /// A page just arrived by following a link or typing (not Back, Forward or reloading):
    /// follow its meta refresh, if it has a quick one, once it's laid out
    pub follow_refresh_after_render: bool,
    /// The displayed page's meta refresh, and how many in a row were followed to reach it
    pub refresh: Option<crate::renderer::Refresh>,
    pub refreshes_followed: u8,
    /// Address bar contents when Edit mode was entered, for history and cancelling
    pub url_before_edit: Option<String>,
    /// Line index of each in-page link target on the displayed page
    pub anchors: HashMap<String, usize>,
    /// URL of the displayed document after redirects; None for the help page and error pages
    pub page_url: Option<String>,
    /// #fragment to jump to once the page being loaded arrives
    pub pending_fragment: Option<String>,
    /// Whether html_source holds HTML or plain text
    pub page_kind: PageKind,
    /// Bumped whenever what's shown changes (new page, source view toggle), so a background
    /// render of older content is never applied
    pub content_version: u64,
    /// Version of the background render currently running, if any
    pub(super) render_in_flight: Option<u64>,
    /// Width for one more render once the running one finishes (coalesces rapid resizes)
    pub(super) render_pending_width: Option<u16>,
    /// The page just loaded goes into the history once laid out, when its <title> is known
    pub record_visit_after_render: bool,
    /// A jump within the page selected the first link after it: Tab goes there, not past it
    pub tab_from_jump: bool,
    /// #fragment to jump to once the page has been laid out
    jump_after_render: Option<String>,
    /// Scroll and cursor position to return to when the page being loaded arrives (going back,
    /// forward or reloading)
    pub(super) pending_restore: Option<(usize, usize)>,
    /// That position, once the page has arrived and is waiting to be laid out
    restore_after_render: Option<(usize, usize)>,
    /// The page's forms and controls, and what the user has entered into them
    pub forms: Vec<Form>,
    pub controls: Vec<FormControl>,
    pub control_values: Vec<ControlValue>,
    /// The field being typed into (in FormEdit mode)
    pub editing_control: Option<usize>,
    /// Form data to POST with the next request
    pub(super) pending_post: Option<String>,
    /// A Referer to send with the request for one address, and only that address: confirming an
    /// I2P address helper (App::address_helper_referer)
    pub(super) pending_referer: Option<(String, String)>,
}

impl BrowserTab {
    pub fn enter_visual_mode(&mut self) {
        self.input_mode = InputMode::Visual;
        self.status_message = String::from("Copy mode: select text, then press y to copy it");
        // Anchor the selection to current cursor position
        self.selection = Some(Selection {
            start_line: self.cursor_line,
            start_char: self.cursor_char,
            end_line: self.cursor_line,
            end_char: self.cursor_char,
        });
    }
    /// Clear state that refers to positions in the previous page's content
    pub fn reset_page_state(&mut self) {
        self.scroll = 0;
        self.selected_link_index = 0;
        self.cursor_line = 0;
        self.cursor_char = 0;
        self.selection = None;
        self.search_state = None;
        // Leave Edit mode alone so a page finishing in the background doesn't eat typing
        if matches!(self.input_mode, InputMode::Visual | InputMode::Search | InputMode::FormEdit) {
            self.input_mode = InputMode::Normal;
        }
        self.editing_control = None;
        self.record_visit_after_render = false;
    }

    /// Scroll the minimum amount needed to bring `line` into a viewport of `viewport_height` rows
    pub fn scroll_to_line(&mut self, line: usize, viewport_height: usize) {
        let viewport_height = viewport_height.max(1);
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll + viewport_height {
            self.scroll = line + 1 - viewport_height;
        }
    }

    /// Scroll the view by `delta` lines without running past either end of the page.
    /// Like vim, the cursor is carried along so it stays on screen and j/k continue from there.
    pub fn scroll_by(&mut self, delta: isize, viewport_height: usize) {
        let viewport_height = viewport_height.max(1);
        let last_line = self.rendered_content.len().saturating_sub(1);
        let max_scroll = self.rendered_content.len().saturating_sub(viewport_height);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max_scroll);

        let bottom = (self.scroll + viewport_height - 1).min(last_line);
        let cursor_line = self.cursor_line.max(self.scroll).min(bottom);
        if cursor_line != self.cursor_line {
            self.cursor_line = cursor_line;
            if self.input_mode == InputMode::Visual
                && let Some(ref mut sel) = self.selection
            {
                sel.end_line = self.cursor_line;
                sel.end_char = self.cursor_char;
            }
        }
    }

    /// Base URL for links on the displayed page
    pub fn link_base(&self) -> &str {
        self.page_url.as_deref().unwrap_or(&self.url_input)
    }

    /// Whether `url` refers to the displayed document, ignoring any #fragment
    pub fn is_current_document(&self, url: &str) -> bool {
        let page = self.page_url.as_deref().and_then(|p| Url::parse(p).ok());
        match (page, Url::parse(url)) {
            (Some(mut page), Ok(mut target)) => {
                page.set_fragment(None);
                target.set_fragment(None);
                page == target
            }
            _ => false,
        }
    }

    /// Scroll to the element named by `fragment`; returns false if the page has no such anchor
    pub fn jump_to_fragment(&mut self, fragment: &str) -> bool {
        let decoded = percent_encoding::percent_decode_str(fragment).decode_utf8_lossy();
        let line = match self.anchors.get(decoded.as_ref()).or_else(|| self.anchors.get(fragment)) {
            Some(&line) => line,
            // "#" and "#top" mean the top of the page unless something is named that
            None if decoded.is_empty() || decoded.eq_ignore_ascii_case("top") => 0,
            None => {
                self.status_message = format!("Anchor #{} not found on this page", decoded);
                return false;
            }
        };
        self.scroll = line;
        self.cursor_line = line.min(self.rendered_content.len().saturating_sub(1));
        self.cursor_char = 0;
        // Tab continues from the jump target rather than wherever the selection was
        if let Some(i) = self.link_regions.iter().position(|l| l.line_index >= line) {
            self.selected_link_index = i;
            self.tab_from_jump = true;
        }
        true
    }

    /// Replace what the tab shows. The old layout is cleared at once, so links from the previous
    /// page can't be followed while the new one is being laid out.
    pub fn set_content(&mut self, source: impl Into<Arc<str>>, kind: PageKind) {
        self.html_source = source.into();
        self.page_kind = kind;
        // A new page: no images of it are laid out yet
        self.image_sizes = Arc::default();
        self.content_changed();
    }

    /// Mark the shown content as changed (e.g. toggling source view)
    pub fn content_changed(&mut self) {
        self.tab_from_jump = false;
        self.content_version += 1;
        self.rendered_content.clear();
        self.link_regions.clear();
        self.anchors.clear();
        self.forms.clear();
        self.controls.clear();
        self.control_values.clear();
        self.editing_control = None;
        if self.input_mode == InputMode::FormEdit {
            self.input_mode = InputMode::Normal;
        }
    }

    /// Change the text of the field being edited
    pub fn edit_field(&mut self, edit: impl FnOnce(&mut String)) {
        let Some(index) = self.editing_control else {
            return;
        };
        let max_chars = self.controls.get(index).map_or(0, |c| c.max_chars());
        if let Some(ControlValue::Text(text)) = self.control_values.get_mut(index) {
            edit(text);
            if text.chars().count() > max_chars {
                *text = text.chars().take(max_chars).collect();
            }
        }
    }

    /// Stop typing into a field, keeping what was typed
    pub fn stop_editing(&mut self) {
        self.editing_control = None;
        if self.input_mode == InputMode::FormEdit {
            self.input_mode = InputMode::Normal;
        }
    }

    /// The current page as a history entry, at `url`
    pub fn history_entry(&self, url: String) -> HistoryEntry {
        HistoryEntry { url, scroll: self.scroll, cursor_line: self.cursor_line }
    }

    /// Jump to `fragment` once the page being laid out is ready
    pub fn jump_after_render(&mut self, fragment: Option<String>) {
        self.jump_after_render = fragment;
    }

    /// A new page has arrived: if it was loaded by going back, forward or reloading, return to
    /// the saved position once it's laid out (instead of any #fragment)
    /// Whether the page being loaded is one returned to (Back, Forward, Reload), which restores
    /// where it was
    pub fn is_returning(&self) -> bool {
        self.pending_restore.is_some()
    }

    pub fn restore_position_after_render(&mut self) {
        self.restore_after_render = self.pending_restore.take();
    }

    /// Forget a saved position, e.g. when the load failed
    pub fn cancel_restore(&mut self) {
        self.pending_restore = None;
        self.restore_after_render = None;
    }

    pub(super) fn restore_position(&mut self, (scroll, cursor_line): (usize, usize)) {
        let last_line = self.rendered_content.len().saturating_sub(1);
        self.scroll = scroll.min(last_line);
        self.cursor_line = cursor_line.min(last_line);
        self.cursor_char = 0;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
        self.cursor_line = 0;
        self.cursor_char = 0;
    }

    pub fn scroll_to_bottom(&mut self, viewport_height: usize) {
        self.scroll = self.rendered_content.len().saturating_sub(viewport_height.max(1));
        self.cursor_line = self.rendered_content.len().saturating_sub(1);
        self.cursor_char = 0;
    }

    pub(super) fn apply_render(&mut self, page: RenderedPage) {
        self.rendered_content = page.lines;
        self.link_regions = page.links;
        self.anchors = page.anchors;
        self.page_images = page.images;
        self.image_blocks = page.image_blocks;
        self.refresh = page.refresh;
        // A re-layout of the same page (e.g. a resize) keeps what was typed
        if self.control_values.len() != page.controls.len() {
            self.control_values = page.controls.iter().map(FormControl::initial_value).collect();
            self.editing_control = None;
        }
        self.forms = page.forms;
        self.controls = page.controls;
        if let Some(title) = page.title {
            self.page_title = title;
        }
        if page.reader_found == Some(false) {
            self.status_message = String::from("Reader mode: no article found here, so the whole page is shown");
        }
        self.clamp_positions();
        let fragment = self.jump_after_render.take();
        if let Some(position) = self.restore_after_render.take() {
            self.restore_position(position);
        } else if let Some(fragment) = fragment {
            self.jump_to_fragment(&fragment);
        }
    }

    /// Keep the selected link and cursor inside the current content after a re-render
    fn clamp_positions(&mut self) {
        if self.selected_link_index >= self.link_regions.len() {
            self.selected_link_index = 0;
        }
        self.cursor_line = self.cursor_line.min(self.rendered_content.len().saturating_sub(1));
    }

    pub fn extract_text_from_selection(&self) -> String {
        match &self.selection {
            Some(sel) => sel.extract_text(&self.rendered_content),
            None => String::new(),
        }
    }
    pub fn new(id: usize, initial_url: String) -> Self {
        let help_html = include_str!("../../assets/help.html");
        let document = Html::parse_document(help_html);
        let mut renderer = DomRenderer::new(100);
        renderer.render(&document);

        Self {
            id,
            url_input: initial_url,
            rendered_content: renderer.lines,
            link_regions: renderer.links,
            page_title: String::from("New Tab"),
            scroll: 0,
            history: Vec::new(),
            forward_history: Vec::new(),
            selected_link_index: 0,
            input_mode: InputMode::Normal,
            status_message: String::from("Ready"),
            // Kept so the help page can be laid out again at other widths
            html_source: Arc::from(help_html),
            is_source_view: false,
            reader_mode: false,
            cursor_line: 0,
            cursor_char: 0,
            selection: None,
            download_state: None,
            search_state: None,
            download_prompt: None,
            tool_prompt: None,
            pending_cache_use: fetch::CacheUse::Normal,
            page_images: Vec::new(),
            image_blocks: Vec::new(),
            image_sizes: Arc::default(),
            load_images_after_render: false,
            follow_refresh_after_render: false,
            refresh: None,
            refreshes_followed: 0,
            url_before_edit: None,
            anchors: renderer.anchors,
            page_url: None,
            pending_fragment: None,
            page_kind: PageKind::Html,
            content_version: 0,
            render_in_flight: None,
            render_pending_width: None,
            record_visit_after_render: false,
            tab_from_jump: false,
            jump_after_render: None,
            pending_restore: None,
            restore_after_render: None,
            forms: Vec::new(),
            controls: Vec::new(),
            control_values: Vec::new(),
            editing_control: None,
            pending_post: None,
            pending_referer: None,
        }
    }

    /// Find `query` on the page. Smart case: matching ignores case unless the query has a capital
    /// letter. Matches are stored as screen columns, which is how they're highlighted.
    pub fn perform_search(&mut self, query: &str) {
        if query.is_empty() {
            self.search_state = None;
            return;
        }

        let ignore_case = !query.chars().any(char::is_uppercase);
        let fold = |c: char| if ignore_case { c.to_lowercase().next().unwrap_or(c) } else { c };
        let query_chars: Vec<char> = query.chars().map(fold).collect();
        let mut matches = Vec::new();

        for (line_idx, line) in self.rendered_content.iter().enumerate() {
            let line_chars: Vec<char> = line.to_string().chars().collect();
            // Column where each character starts (wide characters take two columns)
            let mut columns = Vec::with_capacity(line_chars.len() + 1);
            let mut column = 0;
            for ch in &line_chars {
                columns.push(column);
                column += unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
            }
            columns.push(column);

            let mut char_idx = 0;
            while char_idx + query_chars.len() <= line_chars.len() {
                let found = query_chars.iter().enumerate().all(|(i, q)| fold(line_chars[char_idx + i]) == *q);
                if found {
                    matches.push(crate::models::SearchMatch {
                        line_index: line_idx,
                        start_char: columns[char_idx],
                        end_char: columns[char_idx + query_chars.len()],
                    });
                    // Move past this match to avoid overlapping matches
                    char_idx += query_chars.len();
                } else {
                    char_idx += 1;
                }
            }
        }

        // Keep the state even with no matches so the query survives and can be edited
        self.search_state = Some(SearchState { query: query.to_string(), matches, current_match_index: 0 });
    }

    pub fn next_search_match(&mut self) {
        if let Some(search_state) = &mut self.search_state
            && !search_state.matches.is_empty()
        {
            search_state.current_match_index = (search_state.current_match_index + 1) % search_state.matches.len();
        }
    }

    pub fn previous_search_match(&mut self) {
        if let Some(search_state) = &mut self.search_state
            && !search_state.matches.is_empty()
        {
            search_state.current_match_index = if search_state.current_match_index == 0 {
                search_state.matches.len() - 1
            } else {
                search_state.current_match_index - 1
            };
        }
    }

    pub fn clear_search(&mut self) {
        self.search_state = None;
        self.input_mode = InputMode::Normal;
        self.status_message = String::from("Ready");
    }

    pub fn initiate_download_request(&mut self, url: String, download_dir: &Path) {
        // Each tab tracks one download; a second would fight over the same overlay
        if let Some(crate::models::Download { status: crate::models::DownloadStatus::Active, .. }) = self.download_state
        {
            self.status_message = String::from("A download is already in progress in this tab");
            return;
        }
        if let Some(scheme) = crate::network::unsupported_link_scheme(&url) {
            self.status_message = format!("Unsupported link type: {}", scheme);
            return;
        }
        if crate::network::is_small_web(&url) {
            self.status_message = String::from("Downloading from Gemini and Gopher isn't supported yet");
            return;
        }

        let sanitized_name = App::download_filename(&url);
        let target_path = download_dir.join(&sanitized_name);
        let file_exists = App::path_taken(&target_path);
        let resume_from = downloads::partial_download(&target_path, &url);

        self.download_prompt = Some(crate::models::DownloadPrompt {
            url,
            filename: sanitized_name,
            target_path,
            file_exists,
            resume_from,
        });
    }
}
