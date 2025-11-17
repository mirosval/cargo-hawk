use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::{app::model::Status, App};

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(10),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        // Top line: tabs on left, "Cargo Hawk" on right, with dark background
        let selected_idx = self.selected.selected().unwrap_or(0);
        let selected_color = get_tab_color(selected_idx);

        let mut tab_spans = vec![];

        // Add auto mode indicator if enabled
        if self.auto_mode {
            tab_spans.push(Span::styled(
                " » ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let mut step_idx = 0;
        for plan in &self.plans {
            for step in &plan.commands {
                let status_indicator = get_status_indicator(&step.status);
                let tab_text = if status_indicator.is_empty() {
                    format!(" {} {} ", step_idx + 1, step.name)
                } else {
                    format!(" {} {} {} ", step_idx + 1, step.name, status_indicator)
                };
                let tab_color = get_tab_color(step_idx);
                let style = if step_idx == selected_idx {
                    Style::default()
                        .fg(Color::Black)
                        .bg(tab_color)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(tab_color).bg(Color::DarkGray)
                };
                tab_spans.push(Span::styled(tab_text, style));
                step_idx += 1;
            }
        }

        let tab_line = Line::from(tab_spans);
        let tabs_widget = Paragraph::new(tab_line).style(Style::default().bg(Color::DarkGray));
        tabs_widget.render(chunks[0], buf);

        // Render "Cargo Hawk" on the right side of the same line
        let title_text = "Cargo Hawk";
        let title_x = chunks[0].width.saturating_sub(title_text.len() as u16);
        let title_area = ratatui::layout::Rect {
            x: chunks[0].x + title_x,
            y: chunks[0].y,
            width: title_text.len() as u16,
            height: 1,
        };
        let title =
            Paragraph::new(title_text).style(Style::default().fg(Color::White).bg(Color::DarkGray));
        title.render(title_area, buf);

        // Colored separator line using upper half block character (▀) to touch the tab bar
        let separator_line = "▀".repeat(chunks[1].width as usize);
        let separator = Paragraph::new(separator_line).style(Style::default().fg(selected_color));
        let separator_area = ratatui::layout::Rect {
            x: chunks[1].x,
            y: chunks[0].y + chunks[0].height,
            width: chunks[1].width,
            height: 1,
        };
        separator.render(separator_area, buf);

        // Output panel - positioned right after separator
        let output_area = ratatui::layout::Rect {
            x: chunks[1].x,
            y: chunks[0].y + chunks[0].height + 1,
            width: chunks[1].width,
            height: chunks[1].height.saturating_sub(1),
        };

        // Parse ANSI color codes in the output
        let visible_output: Vec<Line> = self
            .output
            .iter()
            .map(|line| {
                // Parse ANSI codes and convert to styled text
                match line.as_bytes().into_text() {
                    Ok(text) => {
                        // Extract the first line from the parsed text
                        if text.lines.is_empty() {
                            Line::from("")
                        } else {
                            text.lines[0].clone()
                        }
                    }
                    Err(_) => Line::from(line.clone()),
                }
            })
            .collect();

        let output_panel = Paragraph::new(visible_output)
            .block(Block::default().borders(Borders::NONE))
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        output_panel.render(output_area, buf);

        // Input field with focus indicator
        let input_text = &self.command_inputs[selected_idx];
        let (input_style, input_title) = if self.input_focused {
            (
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                "Command [EDITING]",
            )
        } else {
            (Style::default().fg(Color::DarkGray), "Command")
        };
        let input = Paragraph::new(input_text.as_str())
            .block(Block::default().borders(Borders::ALL).title(input_title))
            .style(input_style);
        input.render(chunks[2], buf);

        // Footer with mode-specific shortcuts
        let footer_text = if self.input_focused {
            format!("Esc: Exit edit mode | Enter: Run | ←/→/Home/End: Navigate | Ctrl+C: Quit")
        } else {
            format!(
                "q/Esc: Quit | i: Edit command | p: Edit plan | Enter/r: Run | a: Auto advance | 1-9: Switch tab | j/k/↑/↓: Scroll | s: Diag ({diag})",
                diag = self.diagnostic_display_mode
            )
        };
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().fg(Color::Gray));
        footer.render(chunks[3], buf);
    }
}

fn get_tab_color(idx: usize) -> Color {
    let colors = [
        Color::Cyan,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Blue,
        Color::Red,
    ];
    colors[idx % colors.len()]
}

fn get_status_indicator(status: &Status) -> &str {
    match status {
        Status::NotRun => "",
        Status::Running => "...",
        Status::Warning => "!",
        Status::Failure => "✗",
        Status::Success => "✓",
    }
}
