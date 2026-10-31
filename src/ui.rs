use crate::app::App;
use crate::constants::*;
use crate::constants::{TAB_BAR_HEIGHT, URL_BAR_HEIGHT};
use crate::models::{InputMode, LinkRegion};
use crate::renderer::sanitize_for_terminal;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, Paragraph, Tabs},
};

fn text_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

/// `text` cut to at most `width` columns, ending in "…" when cut
fn fit(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_string();
    }
    let mut shown = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        shown.push(ch);
        used += w;
    }
    if width > 0 {
        shown.push('…');
    }
    shown
}

/// The end of `text` within `width` columns, starting with "…" when cut: for text being typed,
/// whose end is where the typing happens
fn fit_end(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_string();
    }
    let mut kept = Vec::new();
    let mut used = 0;
    for ch in text.chars().rev() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        kept.push(ch);
        used += w;
    }
    let mut shown = String::from(if width > 0 { "…" } else { "" });
    shown.extend(kept.into_iter().rev());
    shown
}

/// Tab titles shorter than this are cut no further; past it, only some tabs are shown
const MIN_TAB_TITLE: usize = 12;

/// Fit tab titles into `width` columns, each drawn as " title " with a divider between: as they
/// are if they fit, else each shortened (down to MIN_TAB_TITLE), else the run of tabs around the
/// selected one that fits. Returns the first tab shown and the titles shown.
pub fn fit_tabs(titles: &[String], selected: usize, width: usize) -> (usize, Vec<String>) {
    let cost = |titles: &[String]| -> usize {
        titles.iter().map(|t| text_width(t) + 2).sum::<usize>() + titles.len().saturating_sub(1)
    };
    let longest = titles.iter().map(|t| text_width(t)).max().unwrap_or(0);
    let mut capped: Vec<String> = titles.to_vec();
    for cap in (MIN_TAB_TITLE.min(longest)..=longest).rev() {
        capped = titles.iter().map(|t| fit(t, cap)).collect();
        if cost(&capped) <= width {
            return (0, capped);
        }
    }
    // Too many tabs even shortened: the selected one, then its neighbours while they fit
    let selected = selected.min(capped.len().saturating_sub(1));
    let (mut first, mut last) = (selected, selected);
    loop {
        let grew_right = last + 1 < capped.len() && cost(&capped[first..=last + 1]) <= width;
        if grew_right {
            last += 1;
        }
        let grew_left = first > 0 && cost(&capped[first - 1..=last]) <= width;
        if grew_left {
            first -= 1;
        }
        if !grew_right && !grew_left {
            break;
        }
    }
    let mut shown = capped[first..=last].to_vec();
    if shown.len() == 1 {
        shown[0] = fit(&shown[0], width.saturating_sub(2));
    }
    (first, shown)
}

/// Render the tab bar: every tab when they fit, else shortened titles, else the tabs around the
/// current one with its number in the title
fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<String> = app.tabs.iter().map(|t| sanitize_for_terminal(&t.page_title).into_owned()).collect();
    let (first, shown) = fit_tabs(&titles, app.active_tab_index, area.width.saturating_sub(2) as usize);
    let title = if shown.len() < titles.len() {
        format!("Tabs {}/{}", app.active_tab_index + 1, titles.len())
    } else {
        String::from("Tabs")
    };
    // Each title gets a space either side (the widget's padding), as fit_tabs counts
    let tabs = Tabs::new(shown.into_iter().map(Line::from))
        .select(app.active_tab_index - first)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    f.render_widget(tabs, area);
}

/// A mode's label in the address bar: bold black on `color`
fn badge(text: &str, color: Color) -> Span<'static> {
    Span::styled(format!(" {} ", text), Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD))
}

/// The address bar title's parts, each with a shorter form for narrow screens
struct BarLabel {
    badge: Option<(&'static str, &'static str, Color)>,
    hint: String,
    short_hint: String,
}

/// The address bar's title: the mode on the left and the network on the right, shortened step by
/// step to fit `width`. The I2P label always stays; "Clearweb", the default, goes first.
fn bar_title(label: &BarLabel, i2p: bool, width: usize) -> (Line<'static>, Line<'static>) {
    let network_style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    let (network, short_network) = if i2p {
        (Span::styled(" I2P mode ", network_style), Span::styled(" I2P ", network_style))
    } else {
        (Span::raw(" Clearweb "), Span::raw(""))
    };
    let left = |long_badge: bool, hint: &str| -> Line<'static> {
        let mut spans = Vec::new();
        if let Some((long, short, color)) = label.badge {
            spans.push(badge(if long_badge { long } else { short }, color));
        }
        spans.push(Span::raw(hint.to_string()));
        Line::from(spans)
    };
    let tries = [
        (left(true, &label.hint), network.clone()),
        (left(true, &label.short_hint), network.clone()),
        (left(true, &label.short_hint), short_network.clone()),
        (left(false, &label.short_hint), short_network.clone()),
    ];
    let fits = |(l, r): &(Line, Span)| l.width() + r.width() + usize::from(r.width() > 0) <= width;
    let (l, r) = tries.iter().find(|t| fits(t)).unwrap_or(&tries[3]).clone();
    (l, Line::from(r))
}

/// Render the address bar: the address (or what the current mode needs), with the mode as a
/// label on the left of its title and the network on the right, all fitted to the width
fn render_url_bar(f: &mut Frame, app: &App, area: Rect) {
    let active_tab = &app.tabs[app.active_tab_index];
    let key = |text: &'static str| Span::styled(text, Style::default().add_modifier(Modifier::BOLD));
    let width = area.width.saturating_sub(2) as usize;

    let (label, body): (BarLabel, Line) = match active_tab.input_mode {
        InputMode::Search => {
            let (query, count, current) = active_tab
                .search_state
                .as_ref()
                .map(|s| {
                    (
                        s.query.as_str(),
                        s.matches.len(),
                        if s.matches.is_empty() { 0 } else { s.current_match_index + 1 },
                    )
                })
                .unwrap_or(("", 0, 0));
            let (hint, short_hint) = if count == 0 {
                (String::from(" no matches "), String::from(" 0 "))
            } else {
                (format!(" match {} of {} ", current, count), format!(" {}/{} ", current, count))
            };
            (
                BarLabel { badge: Some(("FIND", "FIND", Color::Magenta)), hint, short_hint },
                Line::from(fit_end(&sanitize_for_terminal(query), width)),
            )
        }
        InputMode::Visual => {
            let selected = active_tab.extract_text_from_selection().chars().count();
            let full = Line::from(vec![
                Span::raw("Move with "),
                key("h j k l"),
                Span::raw(" or arrows to select · "),
                key("y"),
                Span::raw(" copies · "),
                key("Esc"),
                Span::raw(" cancels"),
            ]);
            let body = if full.width() <= width {
                full
            } else {
                Line::from(vec![key("hjkl"), Span::raw(" select · "), key("y"), Span::raw(" copy · "), key("Esc")])
            };
            (
                BarLabel {
                    badge: Some(("COPY MODE", "COPY", Color::Cyan)),
                    hint: format!(" {} characters selected ", selected),
                    short_hint: format!(" {} ", selected),
                },
                body,
            )
        }
        mode => {
            // A password typed into the address is hidden except while editing it
            let address = if mode == InputMode::Editing {
                active_tab.url_input.clone()
            } else {
                crate::network::mask_password(&active_tab.url_input)
            };
            let (badge, hint) = match mode {
                InputMode::Editing => (Some(("EDITING ADDRESS", "EDIT", Color::Yellow)), " Enter goes, Esc cancels "),
                InputMode::FormEdit => (Some(("TYPING INTO FORM", "FORM", Color::Green)), " Enter sends, Esc stops "),
                _ if active_tab.reader_mode => {
                    (Some(("READER", "READER", Color::LightBlue)), " R shows the whole page ")
                }
                _ => (None, " Address "),
            };
            let short_hint = if badge.is_some() { "" } else { " Address " };
            let label = BarLabel { badge, hint: hint.to_string(), short_hint: short_hint.to_string() };
            let style = if mode == InputMode::Editing { Style::default().fg(Color::Yellow) } else { Style::default() };
            // Shown as typed while editing, its end in view; otherwise with its site's name
            // readable when safe, and any warning kept in view
            let line = if mode == InputMode::Editing {
                Line::styled(fit_end(&sanitize_for_terminal(&address), width), style)
            } else {
                let (shown, lookalike) = crate::names::show_address(&address);
                let warning = lookalike_warning(lookalike);
                let room = width.saturating_sub(warning.as_ref().map_or(0, |w| w.width()));
                let mut spans = vec![Span::styled(fit(&sanitize_for_terminal(&shown), room), style)];
                spans.extend(warning);
                Line::from(spans)
            };
            (label, line)
        }
    };

    let (title, network) = bar_title(&label, app.i2p_mode, width);
    let border = match active_tab.input_mode {
        InputMode::Visual => Style::default().fg(Color::Cyan),
        InputMode::Editing => Style::default().fg(Color::Yellow),
        InputMode::Search => Style::default().fg(Color::Magenta),
        InputMode::FormEdit => Style::default().fg(Color::Green),
        InputMode::Normal => Style::default(),
    };
    let block = Block::default().borders(Borders::ALL).border_style(border).title(title).title(network.right_aligned());
    f.render_widget(Paragraph::new(body).block(block), area);
}

/// Apply visual mode selection highlighting to content lines
fn apply_visual_highlights(lines: &mut [Line], selection: &crate::models::Selection, start_index: usize) {
    // Normalize bounds for rendering
    let (s_line, s_char, e_line, e_char) =
        if (selection.start_line, selection.start_char) <= (selection.end_line, selection.end_char) {
            (selection.start_line, selection.start_char, selection.end_line, selection.end_char)
        } else {
            (selection.end_line, selection.end_char, selection.start_line, selection.start_char)
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
        if let Some(current_match) = search_state.matches.get(search_state.current_match_index)
            && current_match.line_index >= start_index
            && current_match.line_index < end_index
        {
            let relative_line_idx = current_match.line_index - start_index;

            if let Some(line) = lines.get_mut(relative_line_idx) {
                let mut current_x = 0;
                for span in line.spans.iter_mut() {
                    let span_width = span.width();
                    let span_end = current_x + span_width;

                    if current_x < current_match.end_char && span_end > current_match.start_char {
                        // Current match gets green background
                        span.style = span.style.bg(Color::Green).fg(Color::Black);
                    }
                    current_x = span_end;
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
                    return; // Stop looking once the cursor position is styled
                }
                current_x = span_end;
            }
            // Past the end of the line (an empty line, between paragraphs): a reversed space
            // there, so the cursor can still be seen
            if cursor_char > current_x {
                line.spans.push(Span::raw(" ".repeat(cursor_char - current_x)));
            }
            line.spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
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
        // A button's label never changes, and a <button>'s is the page's own text, laid out by
        // the renderer with no width reserved: redrawing it would cut it short
        if control.kind == crate::forms::ControlKind::Submit {
            continue;
        }
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
/// A site name that could pass for another (mixed alphabets, look-alike letters): what it
/// looks like, kept apart from the encoded name shown
fn lookalike_warning(lookalike: Option<String>) -> Option<Span<'static>> {
    lookalike.map(|name| {
        Span::styled(
            format!(" ⚠ look-alike name, written to pass for {} ", sanitize_for_terminal(&name)),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    })
}

/// A destination as shown in the link preview: its site's name readable when safe, and warnings
fn destination(target: &str, style: Style) -> Vec<Span<'static>> {
    let (shown, lookalike) = crate::names::show_address(target);
    let mut spans = vec![Span::styled(sanitize_for_terminal(&shown).into_owned(), style)];
    spans.extend(lookalike_warning(lookalike));
    spans.extend(hidden_destination_warning(target));
    spans
}

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
            spans.extend(destination(&target, link_style));
        }
        spans.push(Span::raw(" "));
        return Line::from(spans);
    }
    match tab.link_regions.get(tab.selected_link_index) {
        Some(link) => {
            let target = crate::network::resolve_url(tab.link_base(), &link.url);
            let mut spans = vec![Span::raw(" Link: ")];
            spans.extend(destination(&target, link_style));
            spans.push(Span::raw(" "));
            Line::from(spans)
        }
        None => Line::from(""),
    }
}

/// The page border's title: "Browser - [Status: ...]" when it fits, else the status alone, cut
/// short if need be
fn status_title(status: &str, width: usize) -> String {
    let full = format!("Browser - [Status: {}]", status);
    if text_width(&full) <= width { full } else { fit(status, width) }
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
        active_tab.rendered_content[start_index..end_index].iter().map(split_into_characters).collect()
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
    apply_search_highlights(&mut viewport_content, active_tab.search_state.as_ref(), start_index, end_index);

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

    let content = Paragraph::new(viewport_content).scroll((0, 0)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(status_title(
                &sanitize_for_terminal(&active_tab.status_message),
                area.width.saturating_sub(2) as usize,
            ))
            .title_bottom(link_preview(active_tab)),
    );

    f.render_widget(Clear, area);
    f.render_widget(content, area);
    render_inline_images(f, app, area, start_index, end_index);
    render_download_overlay(f, app, area);
    render_download_prompt(f, app);
    render_tool_prompt(f, app);
    render_image_viewer(f, app, area);
}

/// Draw the page's loaded images over the space reserved for them, when it's entirely on screen
fn render_inline_images(f: &mut Frame, app: &App, area: Rect, start: usize, end: usize) {
    let tab = &app.tabs[app.active_tab_index];
    for block in &tab.image_blocks {
        let last = block.line_index + block.rows as usize;
        if block.line_index < start || last > end {
            continue;
        }
        let Some(image) = tab.page_images.get(block.image) else { continue };
        let url = crate::network::resolve_url(tab.link_base(), &image.src);
        // Inside the page's border
        let rect = Rect {
            x: area.x + 1 + block.x as u16,
            y: area.y + 1 + (block.line_index - start) as u16,
            width: block.columns,
            height: block.rows,
        }
        .intersection(area);
        app.with_drawn_image(&url, |state| {
            // Scaled to its block, larger than its pixels if the page draws it larger
            let image = ratatui_image::StatefulImage::default().resize(ratatui_image::Resize::Scale(None));
            f.render_stateful_widget(image, rect, state);
        });
    }
}

/// The full-screen image viewer, over the page
fn render_image_viewer(f: &mut Frame, app: &App, area: Rect) {
    let Some((_, alt)) = &app.images.viewer else { return };
    f.render_widget(Clear, area);
    // The image first, inside the border: drawing it measures the zoom the foot shows
    app.draw_viewer_image(f, Block::default().borders(Borders::ALL).inner(area));
    let note = match app.images.viewer_note.as_str() {
        "" => String::new(),
        note => format!("{} · ", sanitize_for_terminal(note)),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Line::from(vec![Span::styled(
            format!(" {} ", sanitize_for_terminal(alt)),
            Style::default().add_modifier(Modifier::BOLD),
        )]))
        .title_bottom(format!(
            " {}{} · j/k scroll · +/- zoom · n/p next/previous · Esc closes ",
            note,
            app.viewer_position()
        ));
    f.render_widget(block, area);
}

pub fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(TAB_BAR_HEIGHT), // Tab Bar
            Constraint::Length(URL_BAR_HEIGHT), // URL Input
            Constraint::Min(0),                 // Content area
        ])
        .split(f.area());

    // Render each UI component
    render_tabs(f, app, chunks[0]);
    render_url_bar(f, app, chunks[1]);
    render_browser_content(f, app, chunks[2]);
}

fn render_download_overlay(f: &mut Frame, app: &App, area: Rect) {
    if let Some(state) = &app.tabs[app.active_tab_index].download_state {
        // Clip to the frame so small terminals don't underflow or draw out of bounds
        let popup_area =
            Rect { x: area.width / 4, y: (area.height / 2).saturating_sub(2), width: area.width / 2, height: 5 }
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

    let mut text = vec![Line::from(Span::styled(
        format!("Run \"{}\" on {}?", sanitize_for_terminal(&prompt.title), sanitize_for_terminal(&host)),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
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
    let popup_area = Rect { x: (area.width - width) / 2, y: (area.height - height) / 2, width, height };
    f.render_widget(Clear, popup_area);
    let block =
        Block::default().title(" Run a tool ").borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow));
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

        let mut text = vec![Line::from(format!("File: {}", sanitize_for_terminal(&prompt.filename))), Line::from("")];

        if prompt.file_exists {
            text.push(Line::from(vec![
                Span::styled("WARNING: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::from("File exists and will be overwritten!"),
            ]));
        } else if let Some(saved) = prompt.resume_from {
            text.push(Line::from(format!("Resumes the earlier download ({:.1} MB saved)", saved as f64 / 1048576.0)));
        } else {
            text.push(Line::from("Save to Downloads folder?"));
        }

        // DYNAMIC SPACING: Fill lines until we reach the button row
        while (text.len() as u16) < DOWNLOAD_PROMPT_BUTTON_ROW_OFFSET - 1 {
            text.push(Line::from(""));
        }
        text.push(Line::from(" [Y] Yes   /   [N] No "));

        let paragraph = Paragraph::new(text).block(block).alignment(ratatui::layout::Alignment::Center);

        f.render_widget(paragraph, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cut_to_fit_with_an_ellipsis() {
        assert_eq!(fit("Ready", 10), "Ready");
        assert_eq!(fit("Images loaded", 8), "Images …");
        assert_eq!(fit_end("https://example.com/long/path", 10), "…long/path");
        assert_eq!(fit_end("short", 10), "short");
        // Wide characters count as two columns
        assert_eq!(fit("日本語のページ", 7), "日本語…");
        assert_eq!(fit("anything", 0), "");
    }

    #[test]
    fn tabs_are_shortened_then_windowed_around_the_current_one() {
        let titles: Vec<String> =
            ["Potato - Wikipedia", "Rust Programming Language", "Start"].iter().map(|t| t.to_string()).collect();
        // Room for all as they are
        assert_eq!(fit_tabs(&titles, 0, 80), (0, titles.clone()));
        // Shortened to fit
        let (first, shown) = fit_tabs(&titles, 1, 50);
        assert_eq!(first, 0);
        assert_eq!(shown.len(), 3);
        assert!(shown.iter().map(|t| text_width(t) + 2).sum::<usize>() + 2 <= 50, "{shown:?}");
        assert!(shown[1].ends_with('…'), "{shown:?}");
        // Too narrow for all: the current tab is always among those shown
        for selected in 0..3 {
            let (first, shown) = fit_tabs(&titles, selected, 20);
            assert!(first <= selected && selected < first + shown.len(), "{selected}: {first} {shown:?}");
        }
    }

    #[test]
    fn the_address_bar_title_shortens_but_keeps_i2p() {
        let editing = BarLabel {
            badge: Some(("EDITING ADDRESS", "EDIT", Color::Yellow)),
            hint: String::from(" Enter goes, Esc cancels "),
            short_hint: String::new(),
        };
        let (left, right) = bar_title(&editing, false, 80);
        assert_eq!(left.to_string(), " EDITING ADDRESS  Enter goes, Esc cancels ");
        assert_eq!(right.to_string(), " Clearweb ");
        let (left, right) = bar_title(&editing, true, 20);
        assert_eq!((left.to_string().as_str(), right.to_string().as_str()), (" EDIT ", " I2P "));
        let (left, right) = bar_title(&editing, false, 20);
        assert_eq!((left.to_string().as_str(), right.to_string().as_str()), (" EDITING ADDRESS ", ""));
    }

    #[test]
    fn the_status_drops_its_frame_before_its_words() {
        assert_eq!(status_title("Ready", 80), "Browser - [Status: Ready]");
        assert_eq!(status_title("Loaded 3 images", 20), "Loaded 3 images");
        assert_eq!(status_title("Couldn't connect to example.com", 20), "Couldn't connect to…");
    }
}
