use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::Widget,
};

use crate::{
    App,
    app::widgets::{
        footer::Footer,
        header::{Header, HeaderTab},
        output::OutputWidget,
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
                Constraint::Length(1),
            ])
            .split(area);

        let selected_idx = self.plan.selected_idx();

        let tabs = self
            .plan
            .commands()
            .enumerate()
            .map(|(id, step)| {
                HeaderTab::builder()
                    .id(id)
                    .name(&step.name)
                    .exec(&step.exec)
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

        let output = self.plan.current_output();
        OutputWidget::builder()
            .diagnostic_display_mode(&self.diagnostic_display_mode)
            .scroll_offset(self.scroll_offset)
            .output(output)
            .build()
            .render(chunks[1], buf);

        Footer::builder()
            .diagnostic_mode(self.diagnostic_display_mode.clone())
            .build()
            .render(chunks[2], buf)
    }
}
