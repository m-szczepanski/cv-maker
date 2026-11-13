//! CLI definitions (spec §3). Dispatches subcommands to `crate::commands`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::commands;

/// `cv` — code-first YAML to one-page PDF CV tool.
#[derive(Parser, Debug)]
#[command(
    name = "cv",
    version,
    about,
    after_help = "Run `cv <command> --help` for command-specific flags."
)]
pub struct Cli {
    /// Extra directory with .ttf/.otf fonts. Repeatable (spec §3 global flags).
    #[arg(long = "font-path", value_name = "DIR", global = true)]
    pub font_path: Vec<PathBuf>,

    /// Suppress progress output; print only essentials.
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Print additional diagnostic output.
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Write a starter `cv.yaml` in the current directory (spec §3).
    Init(InitArgs),
    /// Build a YAML file into a one-page PDF next to it.
    Build(BuildArgs),
    /// Rebuild on change of the YAML or its template files (M5).
    Watch(WatchArgs),
    /// Validate a YAML file without producing a PDF (M2).
    Check(CheckArgs),
    /// List built-in templates with one-line descriptions (M5).
    Templates,
    /// Print the JSON Schema (derived from the Rust model) to stdout (M2).
    Schema,
    /// List fonts available to the tool: bundled, `--font-path` and system (spec §3).
    Fonts,
}

/// `cv init` arguments.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Built-in template to base the starter file on.
    #[arg(long, value_name = "NAME", default_value = "compact")]
    pub template: String,

    /// Output file path, relative to the current directory.
    #[arg(long, value_name = "FILE", default_value = "cv.yaml")]
    pub out: PathBuf,

    /// Overwrite an existing file.
    #[arg(long)]
    pub force: bool,
}

/// `cv build` arguments.
#[derive(Args, Debug)]
pub struct BuildArgs {
    /// Input YAML file. Defaults to `cv.yaml` in the current directory.
    #[arg(value_name = "FILE", default_value = "cv.yaml")]
    pub file: PathBuf,

    /// Override the output PDF location (default: `<file-stem>.pdf` next to the input).
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Disable the one-page fitting loop for this run (spec §7, §3).
    #[arg(long)]
    pub no_fit: bool,
}

/// `cv watch` arguments.
#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Input YAML file. Defaults to `cv.yaml` in the current directory.
    #[arg(value_name = "FILE", default_value = "cv.yaml")]
    pub file: PathBuf,
}

/// `cv check` arguments.
#[derive(Args, Debug)]
pub struct CheckArgs {
    /// Input YAML file. Defaults to `cv.yaml` in the current directory.
    #[arg(value_name = "FILE", default_value = "cv.yaml")]
    pub file: PathBuf,
}

/// Dispatch a parsed CLI to its command handler. Errors are returned typed; rendering
/// happens in `main` (miette graphical output for input diagnostics, ADR-0018).
pub fn run(cli: Cli) -> crate::error::Result<()> {
    if cli.verbose {
        eprintln!("cv: verbose mode on; font paths: {:?}", cli.font_path);
    }

    match &cli.command {
        Command::Init(args) => commands::init::run(args),
        Command::Build(args) => commands::build::run(args, &cli),
        Command::Watch(args) => commands::watch::run(args, &cli),
        Command::Check(args) => commands::check::run(args, &cli.font_path),
        Command::Templates => commands::templates::run(),
        Command::Schema => commands::schema::run(),
        Command::Fonts => commands::fonts::run(&cli.font_path),
    }
}
