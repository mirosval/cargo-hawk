use bon::Builder;
use ratatui::{
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

use crate::app::model::DiagnosticDisplayMode;

#[derive(Debug, Builder)]
pub struct Footer {
    diagnostic_mode: DiagnosticDisplayMode,
}

impl Widget for &Footer {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let footer_text = format!(
            "q/Esc: Quit | p: Edit plan | a: Auto advance | 1-9: Switch tab | j/k/↑/↓: Scroll | s: View Mode ({diag})",
            diag = self.diagnostic_mode
        );
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::NONE))
            .style(Style::default().fg(Color::Gray));
        footer.render(area, buf);
    }
}
