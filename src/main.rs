//! `cv` — code-first YAML to one-page PDF CV tool. Entry point.

use clap::Parser;
use std::io::IsTerminal;
use std::process::ExitCode;

fn main() -> ExitCode {
    let parsed = cv::cli::Cli::parse();
    match cv::cli::run(parsed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Colored when stderr is a terminal; snapshots and redirected output stay
            // plain (ADR-0018). Best-effort write: the reader may already be gone.
            cv::commands::err_text(&cv::error::report(&error, std::io::stderr().is_terminal()));
            ExitCode::from(1)
        }
    }
}
