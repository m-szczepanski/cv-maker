//! `cv` — code-first YAML to one-page PDF CV tool. Entry point.

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let parsed = cv::cli::Cli::parse();
    match cv::cli::run(parsed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Error: {message}");
            ExitCode::from(1)
        }
    }
}
