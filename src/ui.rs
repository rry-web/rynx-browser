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
use crate::renderer::sanitize_for_terminal;

/// Render the tab bar showing all open tabs
fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = app
        .tabs
        .iter()
        .map(|t| Line::from(format!(" {} ", sanitize_for_terminal(&t.page_title))))
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

/// A mode's label in the address bar: bold black on `color`
fn badge(text: &str, color: Color) -> Span<'static> {
    Span::styled(format!(" {} ", text), Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD))
}

/// Render the address bar: the address (or what the current mode needs), with the mode as a
/// label on the left of its title and the network on the right
fn render_url_bar(f: &mut Frame, app: &App, area: Rect) {
    let active_tab = &app.tabs[app.active_tab_index];
    let key = |text: &'static str| Span::styled(text, Style::default().add_modifier(Modifier::BOLD));

    let (label, body): (Line, Line) = match active_tab.input_mode {
        InputMode::Search => {
            let (query, count, current) = active_tab
                .search_state
                .as_ref()
                .map(|s| (s.query.as_str(), s.matches.len(), if s.matches.is_empty() { 0 } else { s.current_match_index + 1 }))
                .unwrap_or(("", 0, 0));
            (
                Line::from(vec![
                    badge("FIND", Color::Magenta),
                    Span::raw(if count == 0 { String::from(" no matches ") } else { format!(" match {} of {} ", current, count) }),
                ]),
                Line::from(sanitize_for_terminal(query).into_owned()),
            )
        }
        InputMode::Visual => {
            let selected = active_tab.extract_text_from_selection().chars().count();
            (
                Line::from(vec![badge("COPY MODE", Color::Cyan), Span::raw(format!(" {} characters selected ", selected))]),
                Line::from(vec![
                    Span::raw("Move with "),
                    key("h j k l"),
                    Span::raw(" or arrows to select · "),
                    key("y"),
                    Span::raw(" copies · "),
                    key("Esc"),
                    Span::raw(" cancels"),
                ]),
            )
        }
        mode => {
            // A password typed into the address is hidden except while editing it
            let address = if mode == InputMode::Editing {
                active_tab.url_input.clone()
            } else {
                crate::network::mask_password(&active_tab.url_input)
            };
            let label = match mode {
                InputMode::Editing => Line::from(vec![badge("EDITING ADDRESS", Color::Yellow), Span::raw(" Enter goes, Esc cancels ")]),
                InputMode::FormEdit => Line::from(vec![badge("TYPING INTO FORM", Color::Green), Span::raw(" Enter sends, Esc stops ")]),
                _ => Line::from(" Address "),
            };
            let style = if mode == InputMode::Editing { Style::default().fg(Color::Yellow) } else { Style::default() };
            (label, Line::styled(sanitize_for_terminal(&address).into_owned(), style))
        }
    };

    let network = if app.i2p_mode {
        Line::from(Span::styled(" I2P mode ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
    } else {
        Line::from(" Clearweb ")
    };
    let border = match active_tab.input_mode {
        InputMode::Visual => Style::default().fg(Color::Cyan),
        InputMode::Editing => Style::default().fg(Color::Yellow),
        InputMode::Search => Style::default().fg(Color::Magenta),
        InputMode::FormEdit => Style::default().fg(Color::Green),
        InputMode::Normal => Style::default(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(label)
        .title(network.right_aligned());
    f.render_widget(Paragraph::new(body).block(block), area);
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
                // Black on cyan reads well on dark and light terminals, and differs from the
                // yellow of the selected link and the find highlights
                span.style = span.style.bg(Color::Cyan).fg(Color::Black);
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
    let Some(selected_link) = link_regions.get(selected_link_index) else {
        return;
    };

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

/// Draw each visible form control's current value (what the user typed, a checkbox's mark, the
/// chosen option) inside the box the renderer reserved for it
fn overlay_form_controls(lines: &mut [Line], tab: &crate::app::BrowserTab, start_index: usize) {
    let mut drawn = std::collections::HashSet::new();
    for region in &tab.link_regions {
        let Some(index) = region.control else {
            continue;
        };
        // A control split across lines (very narrow screens) is drawn once
        if !drawn.insert(index) || region.line_index < start_index || region.x_end < region.x_start + 2 {
            continue;
        }
        let (Some(control), Some(value)) = (tab.controls.get(index), tab.control_values.get(index)) else {
            continue;
        };
        let Some(line) = lines.get_mut(region.line_index - start_index) else {
            continue;
        };
        let editing = tab.input_mode == InputMode::FormEdit && tab.editing_control == Some(index);
        let text = sanitize_for_terminal(&control.display(value, editing)).into_owned();
        replace_columns(line, region.x_start + 1, region.x_end - 1, &text);
    }
}

/// Replace the characters in columns `from..to` of a line split into characters with `text`,
/// keeping the style of the first replaced character
fn replace_columns(line: &mut Line, from: usize, to: usize, text: &str) {
    let mut column = 0;
    let mut first = None;
    let mut last = None;
    for (i, span) in line.spans.iter().enumerate() {
        let width = span.width();
        if column >= from && column + width <= to {
            first.get_or_insert(i);
            last = Some(i);
        }
        column += width;
    }
    let (Some(first), Some(last)) = (first, last) else {
        return;
    };
    let style = line.spans[first].style;
    let mut available = to - from;
    let replacement: Vec<Span> = text
        .chars()
        .take_while(|ch| {
            let width = unicode_width::UnicodeWidthChar::width(*ch).unwrap_or(0);
            let fits = width <= available;
            available = available.saturating_sub(width);
            fits
        })
        .map(|ch| Span::styled(ch.to_string(), style))
        .collect();
    line.spans.splice(first..=last, replacement);
}

fn split_into_characters<'a>(line: &'a Line<'a>) -> Line<'a> {
    let spans: Vec<Span<'a>> = line
        .spans
        .iter()
        .flat_map(|span| {
            span.content
                .char_indices()
                .map(move |(i, ch)| Span::styled(&span.content[i..i + ch.len_utf8()], span.style))
        })
        .collect();
    Line::from(spans)
}

/// A warning for addresses that disguise their real site with a user name
fn hidden_destination_warning(target: &str) -> Option<Span<'static>> {
    crate::network::hidden_destination(target).map(|host| {
        Span::styled(
            format!(" ⚠ really goes to {} ", sanitize_for_terminal(&host)),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    })
}

/// Where the selected link really goes, so link text can't disguise its destination
fn link_preview(tab: &crate::app::BrowserTab) -> Line<'static> {
    let link_style = Style::default().fg(Color::Cyan);
    if let Some(index) = tab.link_regions.get(tab.selected_link_index).and_then(|r| r.control) {
        // For a form control, say what it is and where its form sends what you enter
        let Some(control) = tab.controls.get(index) else {
            return Line::from("");
        };
        let mut spans = vec![Span::raw(format!(" {}", sanitize_for_terminal(&control.describe())))];
        if let Some(form) = control.form.and_then(|f| tab.forms.get(f)) {
            let method = match form.method {
                crate::forms::FormMethod::Get => "",
                crate::forms::FormMethod::Post => " (POST)",
            };
            let target = crate::forms::form_target(form, tab.link_base());
            spans.push(Span::raw(format!(" → sends to{} ", method)));
            spans.push(Span::styled(sanitize_for_terminal(&target).into_owned(), link_style));
            spans.extend(hidden_destination_warning(&target));
        }
        spans.push(Span::raw(" "));
        return Line::from(spans);
    }
    match tab.link_regions.get(tab.selected_link_index) {
        Some(link) => {
            let target = crate::network::resolve_url(tab.link_base(), &link.url);
            let mut spans = vec![
                Span::raw(" Link: "),
                Span::styled(
                    sanitize_for_terminal(&target).into_owned(),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" "),
            ];
            spans.extend(hidden_destination_warning(&target));
            Line::from(spans)
        }
        None => Line::from(""),
    }
}

/// Render the main browser content area with all highlighting applied
fn render_browser_content(f: &mut Frame, app: &App, area: Rect) {
    let active_tab = &app.tabs[app.active_tab_index];
    let content_area_height = area.height as usize;
    let start_index = active_tab.scroll;
    let total_lines = active_tab.rendered_content.len();
    let end_index = (start_index + content_area_height).min(total_lines);

    // Pages are stored with one span per run of same-styled text; split just the visible lines
    // into one span per character so highlights can style exactly the characters they cover
    let mut viewport_content: Vec<Line> = if start_index < total_lines {
        active_tab.rendered_content[start_index..end_index]
            .iter()
            .map(split_into_characters)
            .collect()
    } else {
        Vec::new()
    };
    overlay_form_controls(&mut viewport_content, active_tab, start_index);

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

    let status_text = format!("Status: {}", sanitize_for_terminal(&active_tab.status_message));
    let content = Paragraph::new(viewport_content).scroll((0, 0)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Browser - [{}]", status_text))
            .title_bottom(link_preview(active_tab)),
    );

    f.render_widget(Clear, area);
    f.render_widget(content, area);
    render_download_overlay(f, app, area);
    render_download_prompt(f, app);
    render_tool_prompt(f, app);
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
        // Clip to the frame so small terminals don't underflow or draw out of bounds
        let popup_area = Rect {
            x: area.width / 4,
            y: (area.height / 2).saturating_sub(2),
            width: area.width / 2,
            height: 5,
        }
        .intersection(f.area());

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
                                .title(format!(" Downloading: {} ", sanitize_for_terminal(&state.filename))),
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
                                .title(format!(" Downloading: {} ", sanitize_for_terminal(&state.filename))),
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
                    .label(format!("Error: {}", sanitize_for_terminal(error_msg))); // Displays the error string
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
                    .label(format!("Saved: {}", sanitize_for_terminal(&state.filename)));
                f.render_widget(gauge, popup_area);
            }
        }
    }
}

/// The box asking to confirm a tool call, showing exactly what will be sent
fn render_tool_prompt(f: &mut Frame, app: &App) {
    let Some(prompt) = &app.tabs[app.active_tab_index].tool_prompt else {
        return;
    };
    let area = f.area();
    let width = (area.width.saturating_mul(4) / 5).max(20).min(area.width);
    let host = url::Url::parse(&prompt.server).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let inner = width.saturating_sub(4) as usize;

    let mut text = vec![
        Line::from(Span::styled(
            format!("Run \"{}\" on {}?", sanitize_for_terminal(&prompt.title), sanitize_for_terminal(&host)),
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];
    if let Some(warning) = prompt.warning {
        text.push(Line::from(Span::styled(warning, Style::default().fg(Color::Red))));
    }
    text.push(Line::from(""));
    text.push(Line::from("It will send:"));
    let sent = serde_json::to_string_pretty(&prompt.arguments).unwrap_or_default();
    // Leave room for the frame, the lines above and the buttons
    let room = (area.height as usize).saturating_sub(text.len() + 6).max(1);
    let lines: Vec<&str> = sent.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if i + 1 == room && lines.len() > room {
            text.push(Line::from(format!("  … and {} more lines", lines.len() - i)));
            break;
        }
        let line = sanitize_for_terminal(line);
        let shown: String = if line.chars().count() > inner.saturating_sub(2) {
            line.chars().take(inner.saturating_sub(3)).chain(std::iter::once('…')).collect()
        } else {
            line.into_owned()
        };
        text.push(Line::from(format!("  {}", shown)));
    }
    text.push(Line::from(""));
    text.push(Line::from(" [Y] Run   /   [N] Cancel "));

    let height = (text.len() as u16 + 2).min(area.height);
    let popup_area = Rect {
        x: (area.width - width) / 2,
        y: (area.height - height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, popup_area);
    let block = Block::default()
        .title(" Run a tool ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    f.render_widget(Paragraph::new(text).block(block), popup_area);
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
        }
        .intersection(area);

        f.render_widget(Clear, popup_area);

        let mut text = vec![
            Line::from(format!("File: {}", sanitize_for_terminal(&prompt.filename))),
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
