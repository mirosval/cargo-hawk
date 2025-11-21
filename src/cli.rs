use std::path::PathBuf;

use clap::{Parser, Subcommand};

pub fn parse() -> Args {
    HawkArgs::parse().into_args()
}

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct HawkArgs {
    #[command(subcommand)]
    hawk: HawkCommand,
}

impl HawkArgs {
    fn into_args(self) -> Args {
        let HawkCommand::Hawk(args) = self.hawk;
        args
    }
}

#[derive(Subcommand, Debug)]
pub enum HawkCommand {
    Hawk(Args),
}

#[derive(Debug, Parser)]
#[command(version, about = "A file watcher with interactive command runner for Rust projects", long_about = None)]
pub struct Args {
    /// Directory to watch (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    pub path: PathBuf,

    /// Enable logging into cargo_hawk.log
    #[arg(short, long, default_value = "false")]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(command: &str) -> Result<Args, clap::Error> {
        let command_args = command.split_whitespace();
        dbg!(HawkArgs::try_parse_from(command_args).map(HawkArgs::into_args))
    }

    #[test]
    fn test_argparsing() {
        assert!(parse("cargo hawk").is_ok());
        assert!(parse("cargo-hawk hawk").is_ok());
    }
}
