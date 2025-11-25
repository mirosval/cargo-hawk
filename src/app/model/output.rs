use crate::app::model::{
    cargo::{CargoMessage, DiagnosticLevel},
    output_line::OutputLine,
};

#[derive(Debug, Default, Clone)]
pub struct Output {
    lines: Vec<OutputLine>,
}

impl Output {
    pub fn from_line(line: OutputLine) -> Self {
        Self { lines: vec![line] }
    }

    pub fn push(&mut self, line: OutputLine) {
        self.lines.push(line)
    }

    pub fn count_warnings(&self) -> usize {
        self.lines
            .iter()
            .map(|line| match line {
                OutputLine::Cargo(cargo_message) => match cargo_message {
                    CargoMessage::CompilerMessage { message, target: _ }
                        if message.level == DiagnosticLevel::Warning =>
                    {
                        1
                    }
                    _ => 0,
                },
                OutputLine::Other(_) => 0,
            })
            .sum()
    }

    pub fn count_errors(&self) -> usize {
        self.lines
            .iter()
            .map(|line| match line {
                OutputLine::Cargo(cargo_message) => match cargo_message {
                    CargoMessage::CompilerMessage { message, target: _ }
                        if message.level == DiagnosticLevel::Error =>
                    {
                        1
                    }
                    _ => 0,
                },
                OutputLine::Other(_) => 0,
            })
            .sum()
    }

    pub fn has_success(&self) -> Option<bool> {
        self.lines
            .iter()
            .flat_map(|line| match line {
                OutputLine::Cargo(CargoMessage::BuildFinished { success }) => Some(*success),
                _ => None,
            })
            .last()
    }

    pub fn as_sorted(&self) -> SortedOutput<'_> {
        self.lines
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
            })
    }
}

#[derive(Debug, Default)]
pub struct SortedOutput<'a> {
    pub plain: Vec<&'a OutputLine>,
    pub warnings: Vec<&'a OutputLine>,
    pub errors: Vec<&'a OutputLine>,
}
