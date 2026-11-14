//! CLI definitions (spec §3): the command and flag wording users read in `--help`,
//! kept free of internal references; the spec mapping stays in this comment.
//! Dispatches subcommands to `crate::commands`.

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
    /// Extra directory with .ttf/.otf fonts. Repeatable.
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
    /// Write a starter `cv.yaml` in the current directory.
    Init(InitArgs),
    /// Build a YAML file into a one-page PDF next to it.
    Build(BuildArgs),
    /// Rebuild whenever the YAML or its template files change.
    Watch(WatchArgs),
    /// Validate a YAML file without producing a PDF.
    Check(CheckArgs),
    /// List the built-in templates with one-line descriptions.
    Templates,
    /// Print the JSON Schema of the file format to stdout.
    Schema,
    /// List the fonts the tool can use: bundled, `--font-path` and system.
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

    /// Disable the one-page fitting loop for this run.
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
        commands::err_line(&format!(
            "cv: verbose mode on; font paths: {:?}",
            cli.font_path
        ));
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
