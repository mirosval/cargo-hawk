use crate::app::{CommandResult, model::cargo::CargoMessage};
use color_eyre::eyre::Result;
use std::{fmt::Display, process::Stdio};
use tokio::{process::Command, task::JoinHandle};

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

impl Plan {
    pub fn start_next(&mut self) {
        if let Some(next) = self.next_step() {
            next.start();
        }
    }

    fn next_step<'a>(&'a mut self) -> Option<&'a mut PlanStep> {
        self.commands.iter_mut().find(|step| step.is_ready())
    }

    fn reset(&mut self) {
        self.commands.iter_mut().for_each(|step| {
            step.reset();
        });
    }
}

#[derive(Debug)]
pub struct PlanStep {
    pub name: String,
    pub cmd: String,
    pub exec: PlanStepExecution,
}

impl PlanStep {
    fn start(&mut self) {
        // Parse the command string into program and args
        let parts: Vec<String> = self.cmd.split_whitespace().map(|s| s.to_string()).collect();

        let program = parts[0].clone();
        let mut args: Vec<String> = parts[1..].to_vec();

        // Inject JSON format for cargo commands
        if program == "cargo" && !args.is_empty() {
            let cargo_subcommand = &args[0];
            let known_commands = ["check", "build", "test", "run", "clippy"];

            if known_commands.contains(&cargo_subcommand.as_str()) {
                // Insert --message-format after the subcommand
                args.insert(1, "--message-format".to_string());
                args.insert(2, "json-diagnostic-rendered-ansi".to_string());
            }
        }

        // Spawn the command execution as a background task
        let task = tokio::spawn(async move {
            let child = Command::new(&program)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;

            let output = child.wait_with_output().await?;

            // Keep ANSI color codes in the output for colored display
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            Ok(CommandResult {
                stdout,
                stderr,
                success: output.status.success(),
            })
        });

        self.exec = PlanStepExecution::Running { task: Some(task) };
    }

    fn reset(&mut self) {
        self.exec = PlanStepExecution::NotRun;
    }

    fn is_ready(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun => true,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => false,
            PlanStepExecution::Warning { .. } => false,
            PlanStepExecution::Failure { .. } => false,
            PlanStepExecution::Success { .. } => false,
        }
    }

    fn is_finished(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => true,
            PlanStepExecution::Warning { .. } => true,
            PlanStepExecution::Failure { .. } => true,
            PlanStepExecution::Success { .. } => true,
        }
    }
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
