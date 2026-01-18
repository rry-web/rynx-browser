use crate::app::App;
use crate::models::InputMode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Tabs},
};

pub fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3), // Tab Bar
                Constraint::Length(3), // URL Input
                Constraint::Min(0),    // Content area
            ]
            .as_ref(),
        )
        .split(f.area());

    // 1. RENDER TABS
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

    f.render_widget(tabs, chunks[0]);

    // 2. RENDER URL BAR
    let active_tab = &app.tabs[app.active_tab_index];
    let input_style = match active_tab.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Editing => Style::default().fg(Color::Yellow),
        InputMode::Visual => Style::default().fg(Color::Blue),
    };

    let mode_text = if app.i2p_mode {
        " [I2P MODE ON] "
    } else {
        " [Clearweb] "
    };
    let input = Paragraph::new(active_tab.url_input.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("URL - {}", mode_text)),
        );
    f.render_widget(input, chunks[1]);

    // 3. RENDER CONTENT
    let content_area_height = chunks[2].height as usize;
    let start_index = active_tab.scroll;
    let total_lines = active_tab.rendered_content.len();
    let end_index = (start_index + content_area_height).min(total_lines);

    let mut viewport_content = if start_index < total_lines {
        active_tab.rendered_content[start_index..end_index].to_vec()
    } else {
        Vec::new()
    };

    // --- VISUAL MODE HIGHLIGHTING ---
    if let (InputMode::Visual, Some(sel)) = (active_tab.input_mode, &active_tab.selection) {
        // Normalize bounds for rendering
        let (s_line, s_char, e_line, e_char) =
            if (sel.start_line, sel.start_char) <= (sel.end_line, sel.end_char) {
                (sel.start_line, sel.start_char, sel.end_line, sel.end_char)
            } else {
                (sel.end_line, sel.end_char, sel.start_line, sel.start_char)
            };

        for (i, line) in viewport_content.iter_mut().enumerate() {
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

    if !active_tab.link_regions.is_empty() {
        let selected_link = &active_tab.link_regions[active_tab.selected_link_index];

        // Check if the link is within the lines we are currently displaying
        if selected_link.line_index >= start_index && selected_link.line_index < end_index {
            let relative_line_idx = selected_link.line_index - start_index;

            // Boundary check to prevent panic if viewport_content is smaller than expected
            if let Some(line) = viewport_content.get_mut(relative_line_idx) {
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

    // This allows the user to see their starting point before/during selection
    if active_tab.input_mode == InputMode::Normal || active_tab.input_mode == InputMode::Visual {
        if active_tab.cursor_line >= start_index && active_tab.cursor_line < end_index {
            let relative_line_idx = active_tab.cursor_line - start_index;
            if let Some(line) = viewport_content.get_mut(relative_line_idx) {
                let mut current_x = 0;
                for span in line.spans.iter_mut() {
                    let span_width = span.width();
                    let span_end = current_x + span_width;

                    // Check if the cursor_char falls within this span
                    if active_tab.cursor_char >= current_x && active_tab.cursor_char < span_end {
                        // Apply REVERSED style to the span containing the cursor
                        span.style = span.style.add_modifier(Modifier::REVERSED);
                        break; // Stop looking once the cursor position is styled
                    }
                    current_x = span_end;
                }
            }
        }
    }
    let status_text = format!("Status: {}", active_tab.status_message);
    let content = Paragraph::new(viewport_content).scroll((0, 0)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Browser - [{}]", status_text)),
    );

    f.render_widget(Clear, chunks[2]);
    f.render_widget(content, chunks[2]);
    render_download_overlay(f, app, chunks[2])
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
