use tokio::{
    io::{BufReader, Lines},
    process::{Child, ChildStderr, ChildStdout},
};
use tracing::{error, info};

use crate::app::model::{Output, output_line::OutputLine};

#[derive(Debug)]
pub enum PlanStepExecution {
    NotRun {
        output: Output,
    },
    Running(Box<Running>),
    Error {
        output: Output,
    },
    Warning {
        warnings: usize,
        output: Output,
    },
    Failure {
        warnings: usize,
        failures: usize,
        output: Output,
    },
    Success {
        output: Output,
    },
}

impl PlanStepExecution {
    pub fn not_run() -> Self {
        PlanStepExecution::NotRun {
            output: Output::from_line(OutputLine::Other("Not started".to_string())),
        }
    }

    pub fn error(err: String) -> Self {
        PlanStepExecution::Error {
            output: Output::from_line(OutputLine::Other(err)),
        }
    }
}

#[derive(Debug)]
pub struct Running {
    pub original_command: String,
    pub child: Child,
    pub out_stream: Lines<BufReader<ChildStdout>>,
    pub err_stream: Lines<BufReader<ChildStderr>>,
    pub partial_output: Output,
}

impl Running {
    pub fn kill(&mut self) {
        info!(?self.original_command, "kill child process");
        if let Err(err) = self.child.start_kill() {
            error!(?err, "failed to kill child process");
        }
    }
}
