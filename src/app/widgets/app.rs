use ansi_to_tui::IntoText;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::{
    App,
    app::{
        model::Status,
        widgets::{
            footer::Footer,
            header::{Header, HeaderTab},
        },
    },
};

impl Widget for &App {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(10),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        let selected_idx = self.selected.selected().unwrap_or(0);
        let plan = self.plans.get(0);

        let tabs = plan
            .map(|plan| plan.commands.clone())
            .unwrap_or(vec![])
            .into_iter()
            .enumerate()
            .map(|(id, step)| {
                HeaderTab::builder()
                    .id(id)
                    .name(step.name)
                    .status(step.status)
                    .selected(id == selected_idx)
                    .build()
            })
            .collect();

        Header::builder()
            .tabs(tabs)
            .selected_tab(selected_idx)
            .auto_mode(self.auto_mode)
            .build()
            .render(chunks[0], buf);

        // Output panel - positioned right after separator
        let output_area = ratatui::layout::Rect {
            x: chunks[1].x,
            y: chunks[0].y + chunks[0].height,
            width: chunks[1].width,
            height: chunks[1].height,
        };

        // Parse ANSI color codes in the output
        let visible_output: Vec<Line> = self
            .output
            .iter()
            .enumerate()
            .flat_map(|(idx, line)| line.render(&self.diagnostic_display_mode, idx == 0))
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
            .style(Style::default().bg(Color::Black))
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

        Footer::builder()
            .input_focused(self.input_focused)
            .diagnostic_mode(self.diagnostic_display_mode.clone())
            .build()
            .render(chunks[3], buf)
    }
}
