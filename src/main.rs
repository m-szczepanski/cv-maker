//! `cv` — code-first YAML to one-page PDF CV tool. Entry point.

mod cli;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let parsed = cli::Cli::parse();
    match cli::run(parsed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Error: {message}");
            ExitCode::from(1)
        }
    }
}
