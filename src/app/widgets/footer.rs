use bon::Builder;
use ratatui::{
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::model::DiagnosticDisplayMode;

#[derive(Debug, Builder)]
pub struct Footer {
    input_focused: bool,
    diagnostic_mode: DiagnosticDisplayMode,
}

impl Widget for &Footer {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        // Footer with mode-specific shortcuts
        let footer_text = if self.input_focused {
            "Esc: Exit edit mode | Enter: Run | ←/→/Home/End: Navigate | Ctrl+C: Quit".to_string()
        } else {
            format!(
                "q/Esc: Quit | i: Edit command | p: Edit plan | Enter/r: Run | a: Auto advance | 1-9: Switch tab | j/k/↑/↓: Scroll | s: Diag ({diag})",
                diag = self.diagnostic_mode
            )
        };
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().fg(Color::Gray));
        footer.render(area, buf);
    }
}
