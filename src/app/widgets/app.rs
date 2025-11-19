use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::{
    App,
    app::widgets::{
        footer::Footer,
        header::{Header, HeaderTab},
        output::Output,
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

        let tabs = self
            .plan
            .commands
            .iter()
            .enumerate()
            .map(|(id, step)| {
                HeaderTab::builder()
                    .id(id)
                    .name(&step.name)
                    .status(&step.status)
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

        Output::builder()
            .diagnostic_display_mode(&self.diagnostic_display_mode)
            .scroll_offset(self.scroll_offset)
            .output(&self.output)
            .build()
            .render(chunks[1], buf);

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
