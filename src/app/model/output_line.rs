use crate::app::model::{cargo::CargoMessage, diagnostic_mode::DiagnosticDisplayMode};

#[derive(Debug, Clone)]
pub enum OutputLine {
    Cargo(CargoMessage),
    Other(String),
}

impl OutputLine {
    pub fn parse(line: &str) -> Self {
        // Try to parse as JSON cargo message
        if let Some(msg) = CargoMessage::parse(line) {
            OutputLine::Cargo(msg)
        } else {
            OutputLine::Other(line.to_string())
        }
    }

    pub fn render(&self, diagnostic_mode: &DiagnosticDisplayMode, is_first: bool) -> Vec<String> {
        match self {
            OutputLine::Cargo(cargo_message) => cargo_message.render(diagnostic_mode, is_first),
            OutputLine::Other(line) => vec![line.to_string()],
        }
    }
}
