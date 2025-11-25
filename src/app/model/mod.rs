use crate::app::model::cargo::{CargoMessage, DiagnosticLevel};
use std::{fmt::Display, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{Child, ChildStderr, ChildStdout, Command},
    time::Instant,
};
use tracing::{error, info};

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

    pub async fn check(&mut self) {
        for cmd in self.commands.iter_mut() {
            cmd.check().await;
        }
    }

    pub fn reset(&mut self) {
        self.commands.iter_mut().for_each(|step| {
            step.reset();
        });
    }

    pub fn is_running(&self) -> bool {
        self.commands.iter().any(|step| step.is_running())
    }

    pub fn is_finished(&self) -> bool {
        self.commands.iter().any(|step| step.is_finished())
    }

    pub fn best_step_to_present_idx(&self) -> Option<usize> {
        let first_running_step = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.is_running() { Some(idx) } else { None });
        let first_step_with_errors = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.has_errors() { Some(idx) } else { None });
        let first_step_with_failures = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.has_failures() { Some(idx) } else { None });
        let first_step_with_warnings = self
            .commands
            .iter()
            .enumerate()
            .find_map(|(idx, step)| if step.has_warnings() { Some(idx) } else { None });
        let last_successful_step = if self.is_running() {
            self.commands.iter().enumerate().find_map(|(idx, step)| {
                if step.is_running() && idx > 0 {
                    Some(idx - 1)
                } else {
                    None
                }
            })
        } else {
            None
        };
        let last_step = if !self.commands.is_empty() && self.is_finished() {
            Some(self.commands.len() - 1)
        } else {
            None
        };
        let first_step = if !self.commands.is_empty() {
            Some(0)
        } else {
            None
        };
        first_running_step
            .or(first_step_with_errors)
            .or(first_step_with_failures)
            .or(first_step_with_warnings)
            .or(last_successful_step)
            .or(last_step)
            .or(first_step)
    }

    fn next_step(&mut self) -> Option<&mut PlanStep> {
        self.commands.iter_mut().find(|step| step.is_ready())
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

        let child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        info!(?child, "spawned child");
        match child {
            Ok(mut child) => {
                if let Some(stdout) = child.stdout.take()
                    && let Some(stderr) = child.stderr.take()
                {
                    self.exec = PlanStepExecution::Running(Box::new(Running {
                        child,
                        out_stream: BufReader::new(stdout).lines(),
                        err_stream: BufReader::new(stderr).lines(),
                        partial_output: vec![],
                    }));
                } else {
                    self.exec = PlanStepExecution::Error {
                        output: vec![OutputLine::Other(format!(
                            "child process did not have stdout: {program:?}"
                        ))],
                    }
                }
            }
            Err(err) => {
                self.exec = PlanStepExecution::Error {
                    output: vec![OutputLine::Other(format!(
                        "failed to spawn child process: {err:?}"
                    ))],
                }
            }
        }
    }

    async fn check(&mut self) {
        if let PlanStepExecution::Running(running) = &mut self.exec {
            let deadline = Instant::now() + Duration::from_millis(20);
            loop {
                tokio::select! {
                    res = running.out_stream.next_line() => {
                        match res {
                            Ok(Some(line)) => {
                                // read_stdout_lines += 1;
                                // running.out_buf.push_str(&line);
                                let line = parse_output_line(&line);
                                running.partial_output.push(line);
                            },
                            Ok(None) => {},
                            Err(err) => {
                                error!( ?err, "error reading stdout");
                            }
                        }
                    }
                    res = running.err_stream.next_line() => {
                        match res {
                            Ok(Some(line)) => {
                                // read_stderr_lines += 1;
                                // running.err_buf.push_str(&line);
                                let line = parse_output_line(&line);
                                running.partial_output.push(line);
                            }
                            Ok(None) => {},
                            Err(err) => {
                                error!( ?err, "error reading stderr");
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        break;
                    }
                }
            }

            match running.child.try_wait() {
                Ok(Some(status)) => {
                    if status.success() {
                        let is_success = running
                            .partial_output
                            .iter()
                            .flat_map(|line| match line {
                                OutputLine::Cargo(CargoMessage::BuildFinished { success }) => {
                                    Some(*success)
                                }
                                _ => None,
                            })
                            .last();
                        match is_success {
                            Some(succ) => {
                                if succ {
                                    self.exec = PlanStepExecution::Success {
                                        output: running.partial_output.to_owned(),
                                    };
                                } else {
                                    let n_warnings: usize = running
                                        .partial_output
                                        .iter()
                                        .map(|line| match line {
                                            OutputLine::Cargo(cargo_message) => match cargo_message
                                            {
                                                CargoMessage::CompilerMessage {
                                                    message,
                                                    target: _,
                                                } if message.level == DiagnosticLevel::Warning => 1,
                                                _ => 0,
                                            },
                                            OutputLine::Other(_) => 0,
                                        })
                                        .sum();

                                    let n_errors: usize = running
                                        .partial_output
                                        .iter()
                                        .map(|line| match line {
                                            OutputLine::Cargo(cargo_message) => match cargo_message
                                            {
                                                CargoMessage::CompilerMessage {
                                                    message,
                                                    target: _,
                                                } if message.level == DiagnosticLevel::Error => 1,
                                                _ => 0,
                                            },
                                            OutputLine::Other(_) => 0,
                                        })
                                        .sum();
                                    self.exec = PlanStepExecution::Failure {
                                        warnings: n_warnings,
                                        failures: n_errors,
                                        output: running.partial_output.to_owned(),
                                    };
                                }
                            }
                            None => {
                                self.exec = PlanStepExecution::Success {
                                    output: running.partial_output.to_owned(),
                                };
                            }
                        }
                    } else {
                        error!(?status, "child exited with error");
                        self.exec = PlanStepExecution::Error {
                            output: running.partial_output.to_owned(),
                        };
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    error!(?err, "error checking child status");
                    self.exec = PlanStepExecution::Error {
                        output: running.partial_output.to_owned(),
                    };
                }
            }
        }
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

    fn is_running(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun => false,
            PlanStepExecution::Running { .. } => true,
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

    fn has_errors(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => true,
            PlanStepExecution::Warning { .. } => false,
            PlanStepExecution::Failure { .. } => false,
            PlanStepExecution::Success { .. } => false,
        }
    }

    fn has_failures(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => false,
            PlanStepExecution::Warning { .. } => false,
            PlanStepExecution::Failure { .. } => true,
            PlanStepExecution::Success { .. } => false,
        }
    }

    fn has_warnings(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => false,
            PlanStepExecution::Warning { .. } => true,
            PlanStepExecution::Failure { .. } => false,
            PlanStepExecution::Success { .. } => false,
        }
    }
}

#[derive(Debug)]
pub enum PlanStepExecution {
    NotRun,
    Running(Box<Running>),
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

#[derive(Debug)]
pub struct Running {
    child: Child,
    out_stream: Lines<BufReader<ChildStdout>>,
    err_stream: Lines<BufReader<ChildStderr>>,
    pub partial_output: Vec<OutputLine>,
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

#[derive(Debug, Clone)]
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

fn parse_output_line(line: &str) -> OutputLine {
    // Try to parse as JSON cargo message
    if let Some(msg) = CargoMessage::parse(line) {
        OutputLine::Cargo(msg)
    } else {
        OutputLine::Other(line.to_string())
    }
}
