use crate::app::{CommandResult, model::cargo::CargoMessage};
use color_eyre::eyre::Result;
use std::fmt::Display;
use tokio::task::JoinHandle;

pub mod cargo;

#[derive(Debug, Default)]
pub struct Plan {
    pub commands: Vec<PlanStep>,
}

impl Plan {
    pub fn from_string(s: &str) -> Plan {
        let steps: Vec<PlanStep> = s
            .split("\n")
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
                    exec: PlanStepExecution::NotRun,
                }
            })
            .collect();
        Plan { commands: steps }
    }
}

#[derive(Debug)]
pub struct PlanStep {
    pub name: String,
    pub cmd: String,
    pub exec: PlanStepExecution,
}

#[derive(Debug)]
pub enum PlanStepExecution {
    NotRun,
    Running {
        task: Option<JoinHandle<Result<CommandResult>>>,
    },
    Error {
        output: Vec<OutputLine>,
    },
    Warning {
        warnings: usize,
        output: Vec<OutputLine>,
    },
    Failure {
        warnings: usize,
        failures: usize,
        output: Vec<OutputLine>,
    },
    Success {
        output: Vec<OutputLine>,
    },
}

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

#[derive(Debug)]
pub enum OutputLine {
    Cargo(CargoMessage),
    Other(String),
}

impl OutputLine {
    pub fn render(&self, diagnostic_mode: &DiagnosticDisplayMode, is_first: bool) -> Vec<String> {
        match self {
            OutputLine::Cargo(cargo_message) => cargo_message.render(diagnostic_mode, is_first),
            OutputLine::Other(line) => vec![line.to_string()],
        }
    }
}
