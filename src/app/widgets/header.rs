use bon::Builder;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::app::model::PlanStepExecution;

#[derive(Debug, Builder)]
pub struct Header<'a> {
    tabs: Vec<HeaderTab<'a>>,
    selected_tab: usize,
    auto_mode: bool,
}

impl<'a> Widget for &Header<'a> {
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
pub struct HeaderTab<'a> {
    id: usize,
    name: &'a str,
    exec: &'a PlanStepExecution,
    selected: bool,
}

impl<'a> HeaderTab<'a> {
    fn span(&self) -> Span<'a> {
        let status_indicator = get_status_indicator(self.exec);
        let step_idx = self.id;
        let name = self.name;
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

fn get_status_indicator(exec: &PlanStepExecution) -> String {
    match exec {
        PlanStepExecution::NotRun { .. } => "".to_string(),
        PlanStepExecution::Running { .. } => "...".to_string(),
        PlanStepExecution::Warning { warnings, .. } => format!("{warnings}W"),
        PlanStepExecution::Failure {
            warnings, failures, ..
        } => format!("{failures}E {warnings}W"),
        PlanStepExecution::Error { .. } => "Error".to_string(),
        PlanStepExecution::Success { .. } => "✓".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};
    use testresult::TestResult;

    use super::*;

    #[test]
    fn test_header() -> TestResult {
        let not_run = PlanStepExecution::not_run();
        let tab1 = HeaderTab::builder()
            .id(0)
            .name("tab 1")
            .exec(&not_run)
            .selected(true)
            .build();
        let tab2 = HeaderTab::builder()
            .id(1)
            .name("tab 2")
            .exec(&not_run)
            .selected(true)
            .build();
        let tabs = vec![tab1, tab2];
        let header = Header::builder()
            .tabs(tabs)
            .selected_tab(0)
            .auto_mode(true)
            .build();
        let mut terminal = Terminal::new(TestBackend::new(80, 20))?;
        terminal.draw(|frame| frame.render_widget(&header, frame.area()))?;
        assert_snapshot!(terminal.backend());
        Ok(())
    }
}
