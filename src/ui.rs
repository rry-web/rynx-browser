use crate::app::App;
use crate::constants::{TAB_BAR_HEIGHT, URL_BAR_HEIGHT};
use crate::models::{InputMode, LinkRegion};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Tabs},
};
use crate::constants::*;

/// Render the tab bar showing all open tabs
fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = app
        .tabs
        .iter()
        .map(|t| Line::from(format!(" {} ", t.page_title)))
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.active_tab_index)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

/// Render the URL input bar with mode styling
fn render_url_bar(f: &mut Frame, app: &App, area: Rect) {
    let active_tab = &app.tabs[app.active_tab_index];
    let input_style = match active_tab.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Editing => Style::default().fg(Color::Yellow),
        InputMode::Visual => Style::default().fg(Color::Blue),
        InputMode::Search => Style::default().fg(Color::Magenta),
    };

    let mode_text = if app.i2p_mode {
        " [I2P MODE ON] "
    } else {
        " [Clearweb] "
    };

    // In Search mode, show the search query instead of the URL
    let (display_text, title) = match active_tab.input_mode {
        InputMode::Search => {
            let query = active_tab
                .search_state
                .as_ref()
                .map(|s| s.query.as_str())
                .unwrap_or("");
            let match_count = active_tab
                .search_state
                .as_ref()
                .map(|s| s.matches.len())
                .unwrap_or(0);
            let current_index = active_tab
                .search_state
                .as_ref()
                .map(|s| s.current_match_index + 1)
                .unwrap_or(0);

            (
                query,
                format!(
                    "SEARCH - {} [{}/{}] {}",
                    mode_text.trim(),
                    current_index,
                    match_count,
                    mode_text
                ),
            )
        }
        _ => (
            active_tab.url_input.as_str(),
            format!("URL - {}", mode_text),
        ),
    };

    let input = Paragraph::new(display_text)
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(input, area);
}

/// Apply visual mode selection highlighting to content lines
fn apply_visual_highlights(
    lines: &mut [Line],
    selection: &crate::models::Selection,
    start_index: usize,
) {
    // Normalize bounds for rendering
    let (s_line, s_char, e_line, e_char) = if (selection.start_line, selection.start_char)
        <= (selection.end_line, selection.end_char)
    {
        (
            selection.start_line,
            selection.start_char,
            selection.end_line,
            selection.end_char,
        )
    } else {
        (
            selection.end_line,
            selection.end_char,
            selection.start_line,
            selection.start_char,
        )
    };

    for (i, line) in lines.iter_mut().enumerate() {
        let current_line_idx = start_index + i;

        // Skip lines outside the selection range
        if current_line_idx < s_line || current_line_idx > e_line {
            continue;
        }

        let mut current_x = 0;
        for span in line.spans.iter_mut() {
            let span_width = span.width();
            let span_end = current_x + span_width;

            // Determine if this specific span falls within the selection boundaries
            let is_selected = if current_line_idx == s_line && current_line_idx == e_line {
                current_x < e_char && span_end > s_char
            } else if current_line_idx == s_line {
                span_end > s_char
            } else if current_line_idx == e_line {
                current_x < e_char
            } else {
                true
            };

            if is_selected {
                span.style = span.style.bg(Color::Blue).fg(Color::White);
            }
            current_x = span_end;
        }
    }
}

/// Apply highlighting to search results
fn apply_search_highlights(
    lines: &mut [Line],
    search_state: Option<&crate::models::SearchState>,
    start_index: usize,
    end_index: usize,
) {
    if let Some(search_state) = search_state {
        for search_match in &search_state.matches {
            // Check if the search match is within the lines we are currently displaying
            if search_match.line_index >= start_index && search_match.line_index < end_index {
                let relative_line_idx = search_match.line_index - start_index;

                // Boundary check to prevent panic
                if let Some(line) = lines.get_mut(relative_line_idx) {
                    let mut current_x = 0;
                    for span in line.spans.iter_mut() {
                        let span_width = span.width();
                        let span_end = current_x + span_width;

                        // Check if this span overlaps with the search match
                        if current_x < search_match.end_char && span_end > search_match.start_char {
                            // Apply search highlighting (yellow background, black text)
                            span.style = span.style.bg(Color::Yellow).fg(Color::Black);
                        }
                        current_x = span_end;
                    }
                }
            }
        }

        // Highlight current search match with different color (green background)
        if let Some(current_match) = search_state.matches.get(search_state.current_match_index) {
            if current_match.line_index >= start_index && current_match.line_index < end_index {
                let relative_line_idx = current_match.line_index - start_index;

                if let Some(line) = lines.get_mut(relative_line_idx) {
                    let mut current_x = 0;
                    for span in line.spans.iter_mut() {
                        let span_width = span.width();
                        let span_end = current_x + span_width;

                        if current_x < current_match.end_char && span_end > current_match.start_char
                        {
                            // Current match gets green background
                            span.style = span.style.bg(Color::Green).fg(Color::Black);
                        }
                        current_x = span_end;
                    }
                }
            }
        }
    }
}

/// Apply highlighting to the currently selected link
fn apply_link_highlights(
    lines: &mut [Line],
    link_regions: &[LinkRegion],
    selected_link_index: usize,
    start_index: usize,
    end_index: usize,
) {
    if link_regions.is_empty() {
        return;
    }

    let selected_link = &link_regions[selected_link_index];

    // Check if the link is within the lines we are currently displaying
    if selected_link.line_index >= start_index && selected_link.line_index < end_index {
        let relative_line_idx = selected_link.line_index - start_index;

        // Boundary check to prevent panic if viewport_content is smaller than expected
        if let Some(line) = lines.get_mut(relative_line_idx) {
            let mut current_x = 0;
            for span in line.spans.iter_mut() {
                let span_width = span.width();
                let span_end = current_x + span_width;

                if current_x < selected_link.x_end && span_end > selected_link.x_start {
                    span.style = span.style.bg(Color::Yellow).fg(Color::Black);
                }
                current_x = span_end;
            }
        }
    }
}

/// Apply cursor highlighting for Normal and Visual modes
fn apply_cursor_highlight(
    lines: &mut [Line],
    cursor_line: usize,
    cursor_char: usize,
    start_index: usize,
    end_index: usize,
) {
    if cursor_line >= start_index && cursor_line < end_index {
        let relative_line_idx = cursor_line - start_index;
        if let Some(line) = lines.get_mut(relative_line_idx) {
            let mut current_x = 0;
            for span in line.spans.iter_mut() {
                let span_width = span.width();
                let span_end = current_x + span_width;

                // Check if the cursor_char falls within this span
                if cursor_char >= current_x && cursor_char < span_end {
                    // Apply REVERSED style to the span containing the cursor
                    span.style = span.style.add_modifier(Modifier::REVERSED);
                    break; // Stop looking once the cursor position is styled
                }
                current_x = span_end;
            }
        }
    }
}

/// Render the main browser content area with all highlighting applied
fn render_browser_content(f: &mut Frame, app: &App, area: Rect) {
    let active_tab = &app.tabs[app.active_tab_index];
    let content_area_height = area.height as usize;
    let start_index = active_tab.scroll;
    let total_lines = active_tab.rendered_content.len();
    let end_index = (start_index + content_area_height).min(total_lines);

    let mut viewport_content = if start_index < total_lines {
        active_tab.rendered_content[start_index..end_index].to_vec()
    } else {
        Vec::new()
    };

    // Apply visual mode highlighting
    if let (InputMode::Visual, Some(sel)) = (active_tab.input_mode, &active_tab.selection) {
        apply_visual_highlights(&mut viewport_content, sel, start_index);
    }

    // Apply link highlighting
    apply_link_highlights(
        &mut viewport_content,
        &active_tab.link_regions,
        active_tab.selected_link_index,
        start_index,
        end_index,
    );

    // Apply search result highlighting
    apply_search_highlights(
        &mut viewport_content,
        active_tab.search_state.as_ref(),
        start_index,
        end_index,
    );

    // Apply cursor highlighting for Normal and Visual modes
    if active_tab.input_mode == InputMode::Normal || active_tab.input_mode == InputMode::Visual {
        apply_cursor_highlight(
            &mut viewport_content,
            active_tab.cursor_line,
            active_tab.cursor_char,
            start_index,
            end_index,
        );
    }

    let status_text = format!("Status: {}", active_tab.status_message);
    let content = Paragraph::new(viewport_content).scroll((0, 0)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Browser - [{}]", status_text)),
    );

    f.render_widget(Clear, area);
    f.render_widget(content, area);
    render_download_overlay(f, app, area);
    render_download_prompt(f, app);
}

pub fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(TAB_BAR_HEIGHT), // Tab Bar
                Constraint::Length(URL_BAR_HEIGHT), // URL Input
                Constraint::Min(0),                 // Content area
            ]
            .as_ref(),
        )
        .split(f.area());

    // Render each UI component
    render_tabs(f, app, chunks[0]);
    render_url_bar(f, app, chunks[1]);
    render_browser_content(f, app, chunks[2]);
}

fn render_download_overlay(f: &mut Frame, app: &App, area: Rect) {
    if let Some(state) = &app.tabs[app.active_tab_index].download_state {
        let popup_area = Rect {
            x: area.width / 4,
            y: area.height / 2 - 2,
            width: area.width / 2,
            height: 5,
        };

        // Clear the background to prevent content bleed
        f.render_widget(Clear, popup_area);

        // Handle all three DownloadStatus variants
        match &state.status {
            // 1. ACTIVE STATE: Normal progress bar
            crate::models::DownloadStatus::Active => match state.total_size {
                Some(total) => {
                    let percentage = (state.bytes_downloaded as f64 / total as f64 * 100.0) as u16;
                    let gauge = Gauge::default()
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(format!(" Downloading: {} ", state.filename)),
                        )
                        .gauge_style(Style::default().fg(Color::Yellow))
                        .percent(percentage)
                        .label(format!("{:.1}%", percentage));
                    f.render_widget(gauge, popup_area);
                }
                None => {
                    let gauge = Gauge::default()
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(format!(" Downloading: {} ", state.filename)),
                        )
                        .gauge_style(Style::default().fg(Color::Cyan))
                        .percent(100)
                        .label(format!("{} bytes downloaded", state.bytes_downloaded));
                    f.render_widget(gauge, popup_area);
                }
            },

            // 2. FAILED STATE: Red bar with error message
            crate::models::DownloadStatus::Failed(error_msg) => {
                let gauge = Gauge::default()
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Download Failed ")
                            .title_bottom(" Press ESC to clear "),
                    ) // Satisfies "unused" fields
                    .gauge_style(Style::default().fg(Color::Red))
                    .percent(0) // Show as empty/failed
                    .label(format!("Error: {}", error_msg)); // Displays the error string
                f.render_widget(gauge, popup_area);
            }

            // 3. COMPLETED STATE: Green success bar
            crate::models::DownloadStatus::Completed => {
                let gauge = Gauge::default()
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Download Finished ")
                            .title_bottom(" Press ESC to clear "),
                    )
                    .gauge_style(Style::default().fg(Color::Green))
                    .percent(100)
                    .label(format!("Saved: {}", state.filename));
                f.render_widget(gauge, popup_area);
            }
        }
    }
}

fn render_download_prompt(f: &mut Frame, app: &App) {
    if let Some(prompt) = &app.tabs[app.active_tab_index].download_prompt {
        let area = f.area();

        let block = Block::default()
            .title(" Confirm Download ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));

        // Center the pop-up (50% width, fixed height)
        let popup_area = Rect {
            x: area.width / DOWNLOAD_PROMPT_X_DIVISOR,
            y: (area.height / DOWNLOAD_PROMPT_Y_DIVISOR).saturating_sub(DOWNLOAD_PROMPT_Y_OFFSET),
            width: area.width / DOWNLOAD_PROMPT_WIDTH_DIVISOR,
            height: DOWNLOAD_PROMPT_HEIGHT,
        };

        f.render_widget(Clear, popup_area);

        let mut text = vec![
            Line::from(format!("File: {}", prompt.filename)),
            Line::from(""),
        ];

        if prompt.file_exists {
            text.push(Line::from(vec![
                Span::styled("WARNING: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::from("File exists and will be overwritten!"),
            ]));
        } else {
            text.push(Line::from("Save to Downloads folder?"));
        }

        // DYNAMIC SPACING: Fill lines until we reach the button row
        while (text.len() as u16) < DOWNLOAD_PROMPT_BUTTON_ROW_OFFSET - 1 {
            text.push(Line::from(""));
        }
        text.push(Line::from(" [Y] Yes   /   [N] No "));

        let paragraph = Paragraph::new(text)
            .block(block)
            .alignment(ratatui::layout::Alignment::Center);

        f.render_widget(paragraph, popup_area);
    }
}
