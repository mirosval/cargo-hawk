use ansi_to_tui::IntoText;
use bon::Builder;
use ratatui::{
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

use crate::app::model::{
    DiagnosticDisplayMode, OutputLine,
    cargo::{CargoMessage, DiagnosticLevel},
};

#[derive(Debug, Builder)]
pub struct Output<'a> {
    diagnostic_display_mode: &'a DiagnosticDisplayMode,
    output: &'a Vec<OutputLine>,
    scroll_offset: u16,
}

impl Widget for &Output<'_> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let sorted = self
            .output
            .iter()
            .fold(SortedOutput::default(), |mut acc, line| match line {
                cargo @ OutputLine::Cargo(cargo_message) => {
                    match cargo_message {
                        CargoMessage::CompilerMessage { message, target: _ } => {
                            match message.level {
                                DiagnosticLevel::Error => acc.errors.push(cargo),
                                DiagnosticLevel::Warning => acc.warnings.push(cargo),
                                _ => acc.plain.push(cargo),
                            }
                        }
                        _ => acc.plain.push(cargo),
                    };
                    acc
                }
                other @ OutputLine::Other(_) => {
                    acc.plain.push(other);
                    acc
                }
            });

        // Parse ANSI color codes in the output
        let visible_output: Vec<Line> = sorted
            .errors
            .iter()
            .chain(sorted.warnings.iter())
            .chain(sorted.plain.iter())
            .enumerate()
            .flat_map(|(idx, line)| line.render(self.diagnostic_display_mode, idx == 0))
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

        output_panel.render(area, buf);
    }
}

#[derive(Debug, Default)]
struct SortedOutput<'a> {
    plain: Vec<&'a OutputLine>,
    warnings: Vec<&'a OutputLine>,
    errors: Vec<&'a OutputLine>,
}
