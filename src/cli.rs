//! CLI definitions (spec §3). M0: full command surface with stub handlers.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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
    /// Write a starter `cv.yaml` in the current directory (M1).
    Init(InitArgs),
    /// Build a YAML file into a one-page PDF next to it (M1).
    Build(BuildArgs),
    /// Rebuild on change of the YAML or its template files (M5).
    Watch(WatchArgs),
    /// Validate a YAML file without producing a PDF (M2).
    Check(CheckArgs),
    /// List built-in templates with one-line descriptions (M5).
    Templates,
    /// Print the JSON Schema (derived from the Rust model) to stdout (M2).
    Schema,
    /// List fonts available to the tool: bundled and discovered (M3).
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

/// Dispatch a parsed CLI to its command handler.
///
/// M0: every command is an honest stub — it exists, parses its arguments, and reports that it
/// is not implemented yet, exiting non-zero (plan step 0.4). Handlers land with their milestone.
pub fn run(cli: Cli) -> Result<(), String> {
    if cli.verbose {
        eprintln!("cv: verbose mode on; font paths: {:?}", cli.font_path);
    }

    match &cli.command {
        Command::Init(_) => Err(not_yet("init")),
        Command::Build(_) => Err(not_yet("build")),
        Command::Watch(_) => Err(not_yet("watch")),
        Command::Check(_) => Err(not_yet("check")),
        Command::Templates => Err(not_yet("templates")),
        Command::Schema => Err(not_yet("schema")),
        Command::Fonts => Err(not_yet("fonts")),
    }
}

fn not_yet(name: &str) -> String {
    format!(
        "cv {name} is not implemented yet (see docs/implementation-plan.md for the milestone schedule)"
    )
}
