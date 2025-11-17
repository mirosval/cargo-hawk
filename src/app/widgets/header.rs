use bon::Builder;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::app::model::Status;

#[derive(Debug, Builder)]
pub struct Header {
    tabs: Vec<HeaderTab>,
    selected_tab: usize,
    auto_mode: bool,
}

impl Widget for &Header {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        // Top line: tabs on left, "Cargo Hawk" on right, with dark background
        let selected_color = get_tab_color(self.selected_tab);

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

        tab_spans.extend(self.tabs.iter().map(|tab| tab.span()));

        let tab_line = Line::from(tab_spans);

        let tabs_widget = Paragraph::new(tab_line).style(Style::default().bg(Color::DarkGray));
        tabs_widget.render(area, buf);

        // Render "Cargo Hawk" on the right side of the same line
        let title_text = "Cargo Hawk";
        let title_x = area.width.saturating_sub(title_text.len() as u16);
        let title_area = ratatui::layout::Rect {
            x: area.x + title_x,
            y: area.y,
            width: title_text.len() as u16,
            height: 1,
        };
        let title =
            Paragraph::new(title_text).style(Style::default().fg(Color::White).bg(Color::DarkGray));
        title.render(title_area, buf);

        // Colored separator line using upper half block character (▀) to touch the tab bar
        let separator_line = "▀".repeat(area.width as usize);
        let separator = Paragraph::new(separator_line)
            .style(Style::default().fg(selected_color).bg(Color::Black));
        let separator_area = ratatui::layout::Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        };
        separator.render(separator_area, buf);
    }
}

#[derive(Debug, Builder)]
pub struct HeaderTab {
    id: usize,
    name: String,
    status: Status,
    selected: bool,
}

impl HeaderTab {
    fn span<'a>(&'a self) -> Span<'a> {
        let status_indicator = get_status_indicator(&self.status);
        let step_idx = self.id;
        let name = self.name.as_str();
        let tab_text = if status_indicator.is_empty() {
            format!(" {} {} ", step_idx + 1, name)
        } else {
            format!(" {} {} {} ", step_idx + 1, name, status_indicator)
        };
        let tab_color = get_tab_color(step_idx);
        let style = if self.selected {
            Style::default()
                .fg(Color::Black)
                .bg(tab_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(tab_color).bg(Color::DarkGray)
        };
        Span::styled(tab_text, style)
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
