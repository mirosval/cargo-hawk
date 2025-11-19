use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "cargo-hawk")]
#[command(about = "A file watcher with interactive command runner for Rust projects", long_about = None)]
pub struct Args {
    /// Directory to watch (defaults to current directory)
    #[arg(short, long, default_value = ".")]
    pub path: PathBuf,
}
