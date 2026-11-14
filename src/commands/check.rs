//! `cv check` (plan 2.4): validate only, no PDF written.

use std::path::{Path, PathBuf};

use crate::cli::CheckArgs;
use crate::error::Result;

pub fn run(args: &CheckArgs, font_paths: &[PathBuf]) -> Result<()> {
    let loaded = super::load_cv(Path::new(&args.file), font_paths)?;
    super::report_warnings(&loaded.warnings);

    let count = loaded.warnings.len();
    super::out_line(&format!(
        "{} is valid ({count} warning{})",
        args.file.display(),
        if count == 1 { "" } else { "s" },
    ));
    Ok(())
}
