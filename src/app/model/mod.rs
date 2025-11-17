use std::fmt::Display;

pub mod cargo;

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub name: String,
    pub commands: Vec<PlanStep>,
}

impl Plan {
    pub fn from_string(s: &str) -> Plan {
        let steps: Vec<PlanStep> = s
            .split("\n")
            .into_iter()
            .filter(|step| step.trim() != "")
            .map(|cmd| {
                let name = cmd
                    .strip_prefix("cargo ")
                    .and_then(|s| s.split_whitespace().next())
                    .unwrap_or("custom")
                    .to_string();
                PlanStep {
                    name,
                    cmd: cmd.to_string(),
                    status: Status::NotRun,
                }
            })
            .collect();
        Plan {
            name: "default".to_string(),
            commands: steps,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanStep {
    pub name: String,
    pub cmd: String,
    pub status: Status,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    NotRun,
    Running,
    Warning,
    Failure,
    Success,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticDisplayMode {
    Summary,
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

impl Default for DiagnosticDisplayMode {
    fn default() -> Self {
        Self::First
    }
}
