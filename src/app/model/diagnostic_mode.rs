use std::fmt::Display;

#[derive(Debug, Default, Clone, PartialEq)]
pub enum DiagnosticDisplayMode {
    Summary,
    #[default]
    First,
    Full,
}

impl DiagnosticDisplayMode {
    pub fn next(&self) -> Self {
        match self {
            DiagnosticDisplayMode::Summary => DiagnosticDisplayMode::First,
            DiagnosticDisplayMode::First => DiagnosticDisplayMode::Full,
            DiagnosticDisplayMode::Full => DiagnosticDisplayMode::Summary,
        }
    }

    fn as_str(&self) -> &str {
        match self {
            DiagnosticDisplayMode::Summary => "Summary",
            DiagnosticDisplayMode::First => "First",
            DiagnosticDisplayMode::Full => "Full",
        }
    }
}

impl Display for DiagnosticDisplayMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())?;
        Ok(())
    }
}
