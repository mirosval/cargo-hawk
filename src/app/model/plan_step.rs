use std::{mem::swap, path::PathBuf, process::Stdio, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    time::Instant,
};
use tracing::{debug, error, info};

use crate::app::model::{
    Output, Plan, PlanStepExecution, output_line::OutputLine, plan_step_execution::Running,
};

#[derive(Debug)]
pub struct PlanStep {
    pub name: String,
    pub cmd: String,
    pub path: PathBuf,
    pub exec: PlanStepExecution,
    pub previous_exec: Option<PlanStepExecution>,
}

impl PlanStep {
    pub fn start(&mut self) {
        info!(?self.cmd, "start command");

        // Move current execution (if any) into previous_exec
        self.move_exec_to_previous();

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
            .current_dir(&self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(mut child) => {
                if let Some(stdout) = child.stdout.take()
                    && let Some(stderr) = child.stderr.take()
                {
                    self.exec = PlanStepExecution::Running(Box::new(Running {
                        original_command: self.cmd.to_string(),
                        child,
                        out_stream: BufReader::new(stdout).lines(),
                        err_stream: BufReader::new(stderr).lines(),
                        partial_output: Output::default(),
                    }));
                } else {
                    self.exec = PlanStepExecution::Error {
                        output: Output::from_line(OutputLine::Other(format!(
                            "child process did not have stdout: {program:?}"
                        ))),
                    }
                }
            }
            Err(err) => {
                self.exec = PlanStepExecution::Error {
                    output: Output::from_line(OutputLine::Other(format!(
                        "failed to spawn child process: {err:?}"
                    ))),
                }
            }
        }
    }

    pub async fn check(&mut self) {
        if let PlanStepExecution::Running(running) = &mut self.exec {
            let deadline = Instant::now() + Duration::from_millis(5);
            loop {
                tokio::select! {
                    res = running.out_stream.next_line() => {
                        match res {
                            Ok(Some(line)) => {
                                // read_stdout_lines += 1;
                                // running.out_buf.push_str(&line);
                                let line = OutputLine::parse(&line);
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
                                let line = OutputLine::parse(&line);
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
                        info!(?status, "child exited successfully");
                        let is_success = running.partial_output.has_success();
                        match is_success {
                            Some(succ) => {
                                if succ {
                                    self.exec = PlanStepExecution::Success {
                                        output: running.partial_output.to_owned(),
                                    };
                                } else {
                                    self.exec = PlanStepExecution::Failure {
                                        warnings: running.partial_output.count_warnings(),
                                        failures: running.partial_output.count_errors(),
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

    pub fn output(&self) -> &Output {
        match &self.exec {
            PlanStepExecution::NotRun { output } => output,
            PlanStepExecution::Running(running) => self
                .previous_exec
                .as_ref()
                .and_then(|pe| pe.maybe_output())
                .unwrap_or(&running.partial_output),
            //&running.partial_output,
            PlanStepExecution::Error { output } => output,
            PlanStepExecution::Warning { output, .. } => output,
            PlanStepExecution::Failure { output, .. } => output,
            PlanStepExecution::Success { output } => output,
        }
    }

    pub fn reset(&mut self) {
        if let PlanStepExecution::Running(running) = &mut self.exec {
            running.kill();
        }
        self.move_exec_to_previous();
    }

    fn move_exec_to_previous(&mut self) {
        match self.exec {
            PlanStepExecution::NotRun { .. } | PlanStepExecution::Running(_) => {}
            PlanStepExecution::Error { .. }
            | PlanStepExecution::Warning { .. }
            | PlanStepExecution::Failure { .. }
            | PlanStepExecution::Success { .. } => {
                debug!("moving into previous_exec");
                let mut exec = PlanStepExecution::not_run();
                swap(&mut self.exec, &mut exec);
                self.previous_exec = Some(exec);
            }
        }
    }

    pub fn has_been_started(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun { .. } => false,
            PlanStepExecution::Running { .. } => true,
            PlanStepExecution::Error { .. } => true,
            PlanStepExecution::Warning { .. } => true,
            PlanStepExecution::Failure { .. } => true,
            PlanStepExecution::Success { .. } => true,
        }
    }

    pub fn is_running(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun { .. } => false,
            PlanStepExecution::Running { .. } => true,
            PlanStepExecution::Error { .. } => false,
            PlanStepExecution::Warning { .. } => false,
            PlanStepExecution::Failure { .. } => false,
            PlanStepExecution::Success { .. } => false,
        }
    }

    pub fn is_finished(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun { .. } => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => true,
            PlanStepExecution::Warning { .. } => true,
            PlanStepExecution::Failure { .. } => true,
            PlanStepExecution::Success { .. } => true,
        }
    }

    pub fn has_errors(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun { .. } => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => true,
            PlanStepExecution::Warning { .. } => false,
            PlanStepExecution::Failure { .. } => false,
            PlanStepExecution::Success { .. } => false,
        }
    }

    pub fn has_failures(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun { .. } => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => false,
            PlanStepExecution::Warning { .. } => false,
            PlanStepExecution::Failure { .. } => true,
            PlanStepExecution::Success { .. } => false,
        }
    }

    pub fn has_warnings(&self) -> bool {
        match self.exec {
            PlanStepExecution::NotRun { .. } => false,
            PlanStepExecution::Running { .. } => false,
            PlanStepExecution::Error { .. } => false,
            PlanStepExecution::Warning { .. } => true,
            PlanStepExecution::Failure { .. } => false,
            PlanStepExecution::Success { .. } => false,
        }
    }
}
