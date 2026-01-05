use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Tabs, Clear},
    Frame,
};
use crate::app::App;
use crate::models::InputMode;

pub fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab Bar
            Constraint::Length(3), // URL Input
            Constraint::Min(0),    // Content area
        ].as_ref())
        .split(f.area());

    // 1. RENDER TABS
    let titles: Vec<Line> = app.tabs
        .iter()
        .map(|t| Line::from(format!(" {} ", t.page_title)))
        .collect();

    let tabs = Tabs::new(titles)
        .select(app.active_tab_index)
        .block(Block::default().borders(Borders::ALL).title("Tabs"))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    f.render_widget(tabs, chunks[0]);

    // 2. RENDER URL BAR
    let active_tab = &app.tabs[app.active_tab_index];
    let input_style = match active_tab.input_mode {
        InputMode::Normal => Style::default(),
        InputMode::Editing => Style::default().fg(Color::Yellow),
    };

    let mode_text = if app.i2p_mode { " [I2P MODE ON] " } else { " [Clearweb] " };
    let input = Paragraph::new(active_tab.url_input.as_str())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(format!("URL - {}", mode_text)));
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
    //if !active_tab.link_regions.is_empty() {
        //let selected_link = &active_tab.link_regions[active_tab.selected_link_index];
        
        // Only apply highlight if the selected link is in the current scroll view
        /*
        if selected_link.line_index >= start_index && selected_link.line_index < end_index {
            let relative_line_idx = selected_link.line_index - start_index;
            let line = &mut viewport_content[relative_line_idx];
            
            // Find the spans that fall within the x_start and x_end of the selected link
            let mut current_x = 0;
            for span in line.spans.iter_mut() {
                let span_width = span.width();
                let span_end = current_x + span_width;
                
                // If this span overlaps with the selected link coordinates, highlight it
                if current_x < selected_link.x_end && span_end > selected_link.x_start {
                    span.style = span.style.bg(Color::Yellow).fg(Color::Black);
                }
                current_x = span_end;
            }
        }
        */
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
    //}
    let status_text = format!("Status: {}", active_tab.status_message);
    let content = Paragraph::new(viewport_content)
        .scroll((0, 0))
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("Browser - [{}]", status_text)));

    f.render_widget(Clear, chunks[2]);
    f.render_widget(content, chunks[2]);
}
